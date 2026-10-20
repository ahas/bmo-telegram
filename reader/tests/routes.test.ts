import { afterAll, describe, expect, test } from "bun:test";

import { lockOnceFree } from "../src/lock";
import { readerHandler } from "../src/routes";
import { BOT_ID, OWNER_ID } from "./support";

import type { OwnerAccount } from "../src/owner";

// The reader's routes, served in this process over an owner's account that keeps what it is asked, and
// asked as the service asks them.

const token = "seam-token";
const ownerAsked: unknown[] = [];

const owner: OwnerAccount = {
  say: async (chatId, text, replyToMessageId) => {
    ownerAsked.push({ chatId, text, replyToMessageId });

    return 900;
  },
  album: async (album) => album.files.map((_, at) => (at === 0 ? 901 : null)),
};

const server = Bun.serve({
  hostname: "127.0.0.1",
  port: 0,
  fetch: readerHandler({
    token,
    peers: { ownerId: OWNER_ID, botId: BOT_ID },
    owner,
    client: () => null,
    avatars: () => [{ kind: "person", id: 55, takenAt: 1_700_000_000_000 }],
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
    expect(await call("/owner/say", { chatId: 5, text: "words", replyToMessageId: null })).toEqual([200, { messageId: 900 }]);
    expect(ownerAsked).toEqual([{ chatId: 5, text: "words", replyToMessageId: null }]);

    const avatars = await fetch(`http://127.0.0.1:${server.port}/reader/avatars`, { headers: { authorization: `Bearer ${token}` } });

    expect(await avatars.json()).toEqual({ avatars: [{ kind: "person", id: 55, takenAt: 1_700_000_000_000 }] });
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
