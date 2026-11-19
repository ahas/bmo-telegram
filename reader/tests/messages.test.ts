import { afterAll, describe, expect, test } from "bun:test";
import bigInt from "big-integer";

import { ChatUnresolved, placeholderOf, recentMessages } from "../src/messages";
import { mtproto } from "../src/mtproto";
import { readerHandler } from "../src/routes";
import { BOT_ID, OWNER_ID } from "./support";

import type { Api, TelegramClient } from "telegram";
import type { OwnerAccount } from "../src/owner";

// One chat's recent messages read off the live account without a room: newest first, each line with its
// sender and moment, a picture answered as its placeholder, and a chat the account cannot resolve refused.

const peers = { ownerId: OWNER_ID, botId: BOT_ID };
const token = "seam-token";

function message(input: { id: number; text: string; date: number; sender?: number; photo?: boolean }): Api.Message {
  const { Api } = mtproto();
  const read = new Api.Message({
    id: input.id,
    message: input.text,
    date: input.date,
    ...(input.photo === true
      ? { media: new Api.MessageMediaPhoto({ photo: new Api.Photo({ id: bigInt(1), accessHash: bigInt(2), fileReference: Buffer.from([]), date: input.date, sizes: [], dcId: 2 } as never) }) }
      : {}),
  } as never);

  if (input.sender !== undefined) {
    Object.assign(read, { _senderId: bigInt(input.sender), _sender: new Api.User({ id: bigInt(input.sender), firstName: "Bong" }) });
  }

  return read;
}

function accountWith(lines: Api.Message[]): TelegramClient {
  return {
    getEntity: async (peer: number) => ({ id: peer }),
    getDialogs: async () => [],
    iterMessages: async function* () {
      yield* lines;
    },
  } as unknown as TelegramClient;
}

describe("one chat's recent messages", () => {
  test("come back newest first, each with its sender, moment and words", async () => {
    const read = await recentMessages(
      accountWith([
        message({ id: 12, text: "the newest", date: 1_700_000_030, sender: 55 }),
        message({ id: 11, text: "the middle", date: 1_700_000_020, sender: 55 }),
        message({ id: 10, text: "the oldest", date: 1_700_000_010, sender: 55 }),
      ]),
      peers,
      { chatId: 55, max: 200, since: null },
    );

    expect(read.map((one) => one.messageId)).toEqual([12, 11, 10]);
    expect(read[0]).toEqual({ messageId: 12, senderId: 55, senderName: "Bong", sentAt: 1_700_000_030_000, text: "the newest" });
  });

  test("stop at the moment they are asked from and at the number asked for", async () => {
    const lines = accountWith([
      message({ id: 12, text: "the newest", date: 1_700_000_030 }),
      message({ id: 11, text: "the middle", date: 1_700_000_020 }),
      message({ id: 10, text: "the oldest", date: 1_700_000_010 }),
    ]);

    expect(
      (await recentMessages(lines, peers, { chatId: 55, max: 200, since: 1_700_000_020_000 })).map((one) => one.messageId),
    ).toEqual([12, 11]);
    expect((await recentMessages(lines, peers, { chatId: 55, max: 1, since: null })).map((one) => one.messageId)).toEqual([12]);
  });

  test("a picture comes back as its placeholder, captioned or not", () => {
    const { Api } = mtproto();
    const plain = new Api.Message({ id: 1, message: "", date: 1, media: new Api.MessageMediaPhoto({ photo: new Api.Photo({ id: bigInt(1), accessHash: bigInt(2), fileReference: Buffer.from([]), date: 1, sizes: [], dcId: 2 } as never) }) } as never);
    const captioned = new Api.Message({ id: 2, message: "a diagram", date: 1, media: new Api.MessageMediaPhoto({ photo: new Api.Photo({ id: bigInt(1), accessHash: bigInt(2), fileReference: Buffer.from([]), date: 1, sizes: [], dcId: 2 } as never) }) } as never);

    expect(placeholderOf(plain)).toBe("[Photo]");
    expect(placeholderOf(captioned)).toBe("[Photo] a diagram");
  });

  test("a chat the account cannot resolve is refused, never read as a search over every chat", async () => {
    const unresolving = {
      getEntity: async () => {
        throw new Error("no entity");
      },
      getDialogs: async () => [],
      iterMessages: async function* () {
        yield message({ id: 1, text: "another chat's words", date: 1 });
      },
    } as unknown as TelegramClient;

    await expect(recentMessages(unresolving, peers, { chatId: -9, max: 10, since: null })).rejects.toBeInstanceOf(ChatUnresolved);
  });
});

describe("the reader's route for one chat", () => {
  const owner: OwnerAccount = {
    say: async () => ({ id: null, text: "" }),
    album: async () => [],
    typing: async () => {},
    delete: async () => {},
    react: async () => {},
  };

  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch: readerHandler({
      token,
      peers,
      owner,
      client: () => ({}) as TelegramClient,
      rooms: async () => [],
      contacts: async () => [],
      start: async () => ({ outcome: "refused", reason: "never asked" }),
      avatars: () => [],
      messages: async (_client, asked) => {
        if (asked.chatId === -9) throw new ChatUnresolved(-9, new Error("no entity"));

        return [
          { messageId: 2, senderId: 55, senderName: "Bong", sentAt: 2_000, text: "newer" },
          { messageId: 1, senderId: 55, senderName: "Bong", sentAt: 1_000, text: "older" },
        ];
      },
      missing: () => null,
    }),
  });

  afterAll(() => {
    void server.stop(true);
  });

  test("answers the lines in the order they were read, under the seam's token", async () => {
    const answer = await fetch(`http://127.0.0.1:${server.port}/reader/messages?chat=55&max=2`, { headers: { authorization: `Bearer ${token}` } });
    const body = (await answer.json()) as { messages: { messageId: number }[] };

    expect(answer.status).toBe(200);
    expect(body.messages.map((one) => one.messageId)).toEqual([2, 1]);
  });

  test("refuses a chat it cannot resolve with the reason", async () => {
    const answer = await fetch(`http://127.0.0.1:${server.port}/reader/messages?chat=-9`, { headers: { authorization: `Bearer ${token}` } });
    const body = (await answer.json()) as { code: string; error: string };

    expect(answer.status).toBe(404);
    expect(body.code).toBe("not-found");
    expect(body.error).toContain("cannot resolve chat -9");
  });

  test("takes a number for the chat alone", async () => {
    const answer = await fetch(`http://127.0.0.1:${server.port}/reader/messages?chat=named`, { headers: { authorization: `Bearer ${token}` } });

    expect(answer.status).toBe(400);
  });
});
