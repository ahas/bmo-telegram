import { afterAll, describe, expect, test } from "bun:test";

import { lockOnceFree } from "../src/lock";
import { readerHandler } from "../src/routes";
import { BOT_ID, OWNER_ID } from "./support";

import { MessageGone } from "../src/owner";

import type { OwnerAccount } from "../src/owner";

// The reader's routes, served in this process over an owner's account that keeps what it is asked, and
// asked as the service asks them.

const token = "seam-token";
const ownerAsked: unknown[] = [];

const owner: OwnerAccount = {
  say: async (words) => {
    ownerAsked.push(words);

    return { id: 900, text: words.text };
  },
  album: async (album) => {
    ownerAsked.push({ ...album, files: album.files.map((file) => file.name) });

    return album.files.map((_, at) => (at === 0 ? { id: 901, text: album.caption } : { id: null, text: "" }));
  },
  typing: async (chatId, on) => {
    ownerAsked.push({ chatId, on });
  },
  delete: async (chatId, messageId) => {
    ownerAsked.push({ chatId, messageId });

    if (messageId !== 900) throw new MessageGone();
  },
};

const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  fetch: readerHandler({
    token,
    peers: { ownerId: OWNER_ID, botId: BOT_ID },
    owner,
    client: () => null,
    rooms: async () => [],
    contacts: async () => [],
    start: async () => ({ outcome: "refused", reason: "never asked" }),
    avatars: () => [{ kind: "person", id: 55, takenAt: 1_700_000_000_000 }],
    messages: async () => [],
    missing: () => "the account's values are not set",
  }),
});

afterAll(() => {
  void server.stop(true);
});

async function call(path: string, body: unknown, bearer = token): Promise<[number, Record<string, unknown>]> {
  const response = await fetch(`http://127.0.0.1:${server.port}${path}`, {
    method: "POST",
    headers: { authorization: `Bearer ${bearer}`, "content-type": "application/json" },
    body: JSON.stringify(body),
  });

  return [response.status, (await response.json()) as Record<string, unknown>];
}

describe("the reader's door", () => {
  test("takes the seam's token alone, and says whether the owner's account is connected", async () => {
    expect((await call("/owner/say", { chatId: 1, text: "a", replyToMessageId: null }, "other"))[0]).toBe(401);

    const health = await fetch(`http://127.0.0.1:${server.port}/health`, { headers: { authorization: `Bearer ${token}` } });

    expect(await health.json()).toEqual({ connected: false, reason: "the account's values are not set" });
    expect((await call("/elsewhere", {}))[0]).toBe(404);
    expect((await call("/owner/say", { text: "no room" }))[0]).toBe(400);
    expect((await call("/owner/say", { chatId: 5, text: "words", markup: 7, replyToMessageId: null }))[0]).toBe(400);
    expect(await call("/owner/say", { chatId: 5, text: "words", replyToMessageId: null })).toEqual([200, { messageId: 900, text: "words" }]);
    expect(await call("/owner/say", { chatId: 5, text: "bold", markup: "<b>bold</b>", silent: true, replyToMessageId: 3 })).toEqual([
      200,
      { messageId: 900, text: "bold" },
    ]);
    expect(
      await call("/owner/album", {
        chatId: 5,
        files: [
          { name: "a.jpg", mime: "image/jpeg", bytes: "AQI=" },
          { name: "b.jpg", mime: "image/jpeg", bytes: "AQI=" },
        ],
        caption: "seen",
        captionMarkup: "<i>seen</i>",
        replyToMessageId: null,
      }),
    ).toEqual([200, { ids: [901, null], texts: ["seen", ""] }]);
    expect(ownerAsked).toEqual([
      { chatId: 5, text: "words", markup: null, silent: false, replyToMessageId: null },
      { chatId: 5, text: "bold", markup: "<b>bold</b>", silent: true, replyToMessageId: 3 },
      { chatId: 5, files: ["a.jpg", "b.jpg"], caption: "seen", captionMarkup: "<i>seen</i>", replyToMessageId: null },
    ]);

    const avatars = await fetch(`http://127.0.0.1:${server.port}/reader/avatars`, { headers: { authorization: `Bearer ${token}` } });

    expect(await avatars.json()).toEqual({ avatars: [{ kind: "person", id: 55, takenAt: 1_700_000_000_000 }] });
  });

  test("carries the typing mark and a delete, and names a delete of a line the account does not hold gone", async () => {
    const before = ownerAsked.length;

    expect((await call("/owner/typing", { chatId: 5, on: "yes" }))[0]).toBe(400);
    expect((await call("/owner/delete", { chatId: 5 }))[0]).toBe(400);
    expect(await call("/owner/typing", { chatId: 5, on: true })).toEqual([200, {}]);
    expect(await call("/owner/delete", { chatId: 5, messageId: 900 })).toEqual([200, {}]);
    expect(await call("/owner/delete", { chatId: 5, messageId: 7 })).toEqual([
      200,
      { refused: "gone", error: "Telegram has no message here for the owner's account to delete" },
    ]);
    expect(ownerAsked.slice(before)).toEqual([
      { chatId: 5, on: true },
      { chatId: 5, messageId: 900 },
      { chatId: 5, messageId: 7 },
    ]);
  });
});

describe("the owner's account behind the door", () => {
  test("answers that no reader stands where none is connected", async () => {
    expect(await call("/reader/picture", { chatId: 5, messageId: 6, sentAt: 1 })).toEqual([
      200,
      { outcome: "no-reader", reason: "the owner's account is not connected" },
    ]);
    expect(await call("/reader/file", { chatId: 5, messageId: 6, sentAt: 1 })).toEqual([
      200,
      { outcome: "no-reader", reason: "the owner's account is not connected" },
    ]);
  });
});

describe("opening the account", () => {
  test("waits until the service answers and the lock is free, said once", async () => {
    const answers = [false, true, true];
    const locks = [null, { release: async () => undefined }];
    let said = 0;
    let pauses = 0;

    const held = await lockOnceFree({
      mayOpen: async () => answers.shift() ?? true,
      takeLock: async () => locks.shift() ?? null,
      waits: () => true,
      pause: async () => {
        pauses += 1;
      },
      waiting: () => {
        said += 1;
      },
    });

    expect([held !== null, pauses, said]).toEqual([true, 2, 1]);
  });

  test("opens nothing once it is stopping", async () => {
    let asked = 0;

    expect(
      await lockOnceFree({
        mayOpen: async () => true,
        takeLock: async () => {
          asked += 1;

          return null;
        },
        waits: () => asked < 1,
        pause: async () => undefined,
        waiting: () => undefined,
      }),
    ).toBeNull();
  });
});
