// Which messages the live reading hands over (docs/reader.md, 「What it is for」): another bot's wherever the
// room is listened to, never the provider's own bot's, a person's only where the service says the room's
// people are the reader's, and the owner's own there too, but for the messages the reader's own sends went out
// as — each under the provider's own number for it, below every bot number, unless the room numbers for
// everyone (「Numbers」).

import { describe, expect, test } from "bun:test";
import bigInt from "big-integer";

import { mtproto } from "../src/mtproto";
import { providerMessageId } from "../src/numbering";
import { handsOver, ingest } from "../src/reader";
import { OwnSends } from "../src/sends";
import { keptService, OWNER_ID } from "./support";

import type { TelegramClient } from "telegram";
import type { NewMessageEvent } from "telegram/events";
import type { AccountConfig } from "../src/config";
import type { RoomReading } from "../src/service";

const botId = 7_000_000_001;
const otherBot = { id: 9, bot: true };
const person = { id: 55, bot: false };

describe("handsOver", () => {
  test("another bot's words go wherever the room is listened to", () => {
    expect(handsOver(otherBot, { listened: true, people: false, tells: false }, botId)).toBe(true);
    expect(handsOver(otherBot, { listened: false, people: true, tells: false }, botId)).toBe(false);
  });

  test("the provider's own bot is the bot account's to read", () => {
    expect(handsOver({ id: botId, bot: true }, { listened: true, people: true, tells: false }, botId)).toBe(false);
  });

  test("a person's words go only where the room's people are the reader's", () => {
    expect(handsOver(person, { listened: true, people: true, tells: false }, botId)).toBe(true);
    expect(handsOver(person, { listened: true, people: false, tells: false }, botId)).toBe(false);
    expect(handsOver(person, { listened: false, people: true, tells: false }, botId)).toBe(false);
  });

  test("the owner's own words go where the room's people are the reader's, but for the reader's own sends", () => {
    const owner = { id: OWNER_ID, bot: false, out: true };

    expect(handsOver(owner, { listened: true, people: true, tells: false }, botId)).toBe(true);
    expect(handsOver({ ...owner, ownSend: true }, { listened: true, people: true, tells: false }, botId)).toBe(false);
    expect(handsOver(owner, { listened: true, people: false, tells: false }, botId)).toBe(false);
    expect(handsOver(owner, { listened: false, people: true, tells: false }, botId)).toBe(false);
  });
});

describe("ingest", () => {
  const roomId = 5_000_000_123;
  const account: AccountConfig = { apiId: 1, apiHash: "hash", session: "session" };
  const client = {} as TelegramClient;

  // A message the account read in the room, from the account itself where `out`.
  const eventOf = (id: number, out: boolean): NewMessageEvent => {
    const { Api } = mtproto();
    const message = new Api.Message({ id, out, message: `words ${id}`, date: 1_700_000_000 } as never);
    const sender = out
      ? new Api.User({ id: bigInt(OWNER_ID), firstName: "Owner", self: true } as never)
      : new Api.User({ id: bigInt(55), firstName: "Person" } as never);

    Object.assign(message, { getSender: async () => sender, getChat: async () => sender });

    return { message, chatId: bigInt(roomId) } as unknown as NewMessageEvent;
  };
  const readerOf = (room: RoomReading, sends = new OwnSends()) => {
    const service = { ...keptService(), listened: async () => room };

    return { service, sends, deps: { account, peers: { ownerId: OWNER_ID, botId }, service, sends } };
  };

  test("an outgoing message in a room whose people are the reader's goes as the owner's words, under the provider's number", async () => {
    const { service, deps } = readerOf({ listened: true, people: true, tells: false });

    await ingest(deps, client, eventOf(301, true));
    await ingest(deps, client, eventOf(302, false));

    expect(
      service.handed.map((one) => (one.kind === "message" ? [one.messageId, one.message.senderId, one.message.origin, one.message.mode] : null)),
    ).toEqual([
      [providerMessageId(roomId, 301), OWNER_ID, "live", "human-reader"],
      [providerMessageId(roomId, 302), 55, "live", "human-reader"],
    ]);
  });

  test("a reply reference crosses under the provider's numbering too, and a channel's line keeps its own number", async () => {
    const { Api } = mtproto();
    const { service, deps } = readerOf({ listened: true, people: true, tells: false });
    const channel = -1_000_000_000_003;
    const withReply = (id: number, replyTo: number, chatId: number): NewMessageEvent => {
      const message = new Api.Message({
        id,
        out: false,
        message: `words ${id}`,
        date: 1_700_000_000,
        replyTo: new Api.MessageReplyHeader({ replyToMsgId: replyTo } as never),
      } as never);
      const sender = new Api.User({ id: bigInt(9), firstName: "Other", bot: true } as never);

      Object.assign(message, { getSender: async () => sender, getChat: async () => sender });

      return { message, chatId: bigInt(chatId) } as unknown as NewMessageEvent;
    };

    await ingest(deps, client, withReply(310, 305, roomId));
    await ingest(deps, client, withReply(12, 9, channel));

    expect(
      service.handed.map((one) => (one.kind === "message" ? [one.messageId, one.message.replyToMessageId] : null)),
    ).toEqual([
      [providerMessageId(roomId, 310), providerMessageId(roomId, 305)],
      [12, 9],
    ]);
  });

  test("a message the reader's own send went out as is not handed over", async () => {
    const sends = new OwnSends();
    const { service, deps } = readerOf({ listened: true, people: true, tells: false }, sends);

    sends.note(roomId, [401]);
    await ingest(deps, client, eventOf(401, true));
    await ingest(deps, client, eventOf(402, true));

    expect(service.handed.map((one) => one.messageId)).toEqual([providerMessageId(roomId, 402)]);
  });

  test("an outgoing message read while the reader's own send into the room is still going waits for it", async () => {
    const sends = new OwnSends();
    const { service, deps } = readerOf({ listened: true, people: true, tells: false }, sends);
    let answer = () => {};
    const sending = sends.sending(roomId, async () => await new Promise<number>((resolve) => (answer = () => resolve(501))), (id) => [id]);
    const read = ingest(deps, client, eventOf(501, true));

    setTimeout(() => answer(), 20);
    await sending;
    await read;

    expect(service.handed).toEqual([]);
  });

  test("a sticker goes under its placeholder with the sticker in its metadata, its scene following as a sticker", async () => {
    const { Api } = mtproto();
    const { service, deps } = readerOf({ listened: true, people: true, tells: false });
    const message = new Api.Message({
      id: 701,
      message: "",
      date: 1_700_000_000,
      media: new Api.MessageMediaDocument({
        document: new Api.Document({
          id: bigInt(900),
          accessHash: bigInt(2),
          fileReference: Buffer.from([]),
          date: 1_700_000_000,
          mimeType: "application/x-tgsticker",
          size: bigInt(20),
          dcId: 2,
          attributes: [new Api.DocumentAttributeSticker({ alt: "💭", stickerset: new Api.InputStickerSetShortName({ shortName: "Pack" }) })],
        } as never),
      }),
    } as never);
    const sender = new Api.User({ id: bigInt(55), firstName: "Person" } as never);
    const reading = { downloadMedia: async () => new Uint8Array([1, 2, 3]) } as unknown as TelegramClient;

    Object.assign(message, { getSender: async () => sender, getChat: async () => sender });
    await ingest(deps, reading, { message, chatId: bigInt(roomId) } as unknown as NewMessageEvent);

    const [said, gathered] = service.handed;

    expect(said?.kind === "message" ? [said.message.text, said.message.context?.metadata] : null).toEqual([
      "[Sticker 💭]",
      { text: "", entities: [], sticker: { emoji: "💭", set_name: "Pack", is_animated: true, is_video: false } },
    ]);
    expect(gathered?.kind === "gathered" ? gathered.patch.files?.map((file) => [file.name, file.mime, file.kind]) : null).toEqual([
      ["sticker-900.tgs", "application/x-tgsticker", "sticker"],
    ]);
  });

  test("an outgoing message in a room the bot is in is the bot account's to read", async () => {
    const { service, deps } = readerOf({ listened: true, people: false, tells: false });

    await ingest(deps, client, eventOf(601, true));

    expect(service.handed).toEqual([]);
  });
});

describe("OwnSends", () => {
  test("a number is matched once, and not after five minutes", async () => {
    let now = 0;
    const sends = new OwnSends({ now: () => now });

    sends.note(1, [10, 11]);
    expect(await sends.isOwn(1, 10)).toBe(true);
    expect(await sends.isOwn(1, 10)).toBe(false);
    expect(await sends.isOwn(2, 11)).toBe(false);

    now = 5 * 60_000 + 1;
    expect(await sends.isOwn(1, 11)).toBe(false);
  });

  test("a room keeps its last two hundred numbers", async () => {
    const sends = new OwnSends();

    sends.note(1, Array.from({ length: 250 }, (_, at) => at));
    expect(await sends.isOwn(1, 49)).toBe(false);
    expect(await sends.isOwn(1, 50)).toBe(true);
  });

  test("a send that fails settles, so nothing waits on it", async () => {
    const sends = new OwnSends();

    await expect(sends.sending(1, async () => await Promise.reject(new Error("refused")), () => [])).rejects.toThrow("refused");
    expect(await sends.isOwn(1, 1)).toBe(false);
  });
});
