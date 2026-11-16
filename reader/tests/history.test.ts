import { describe, expect, test } from "bun:test";

import {
  FillUnnumbered,
  fillNamedRoom,
  historyMessageId,
  historyMessageText,
  historyRooms,
  historyStanding,
  HistoryUnreadable,
  isHistoryAligned,
  PeerUnresolved,
  walkAll,
  walkNamedRoom,
  walkRoom,
} from "../src/history";
import { mtproto } from "../src/mtproto";
import { ReaderStore } from "../src/store";
import { BOT_ID, keptService, OWNER_ID } from "./support";

import type { TelegramClient } from "telegram";

const peers = { ownerId: OWNER_ID, botId: BOT_ID };

describe("isHistoryAligned", () => {
  test("only a supergroup or channel numbers its messages for everyone", () => {
    expect(isHistoryAligned(-1000000000003)).toBe(true);
    expect(isHistoryAligned(-5000000002)).toBe(false);
    expect(isHistoryAligned(10000001)).toBe(false);
  });

  test("a supergroup keeps its ids; another chat's history goes below every bot id, older first", () => {
    expect(historyMessageId(-1000000000003, 6871)).toBe(6871);
    expect(historyMessageId(-5000000002, 500)).toBeLessThan(0);
    expect(historyMessageId(-5000000002, 100)).toBeLessThan(historyMessageId(-5000000002, 500));
  });
});

describe("a room's walk", () => {
  test("hands each line over as history under the number it is kept by, below the receiver's first message", async () => {
    const { Api } = mtproto();
    const asked: unknown[] = [];
    const photo = new Api.Photo({ id: 1n, accessHash: 2n, fileReference: Buffer.from([]), date: 1, sizes: [], dcId: 2 } as never);
    const lines = [
      new Api.Message({ id: 12, message: "older words", date: 1_700_000_010 } as never),
      new Api.Message({ id: 11, message: "", date: 1_700_000_005, media: new Api.MessageMediaPhoto({ photo }) } as never),
      new Api.Message({ id: 13, message: "newer than the receiver holds", date: 1_700_000_100 } as never),
    ];
    const client = {
      getEntity: async (peer: number) => ({ id: peer }),
      getDialogs: async () => [],
      iterMessages: async function* (entity: unknown, options: unknown) {
        asked.push(entity, options);
        yield* lines;
      },
    } as unknown as TelegramClient;
    const service = keptService({ kept: true });
    const store = new ReaderStore(":memory:");
    const room = { chatId: -5000000002, oldestMessageId: 900, oldestSentAt: 1_700_000_050_000 };

    const outcome = await walkRoom({ client, peers, service, store, pause: async () => undefined }, room);

    expect(outcome).toEqual({ handed: 2, kept: 2, reachedTop: true });
    expect(asked[1]).toEqual({ offsetId: 0, offsetDate: 1_700_000_050, limit: undefined });
    expect(
      service.handed.map((one) =>
        one.kind === "message" && one.message.origin === "history"
          ? [one.messageId, one.message.text, one.message.mode, one.message.sentAt]
          : null,
      ),
    ).toEqual([
      [historyMessageId(-5000000002, 12), "older words", "human-reader", 1_700_000_010_000],
      [historyMessageId(-5000000002, 11), "[Photo]", "human-reader", 1_700_000_005_000],
    ]);
    expect(store.historyProgress(-5000000002)?.doneAt).not.toBeNull();
  });

  test("progress remembers the lowest id reached and whether the top was reached", () => {
    const store = new ReaderStore(":memory:");

    expect(store.historyProgress(-1001)).toBeNull();
    store.setHistoryReached(-1001, 900);
    store.setHistoryReached(-1001, 1200);
    expect(store.historyProgress(-1001)).toEqual({ reachedId: 900, doneAt: null });
    store.markHistoryDone(-1001);
    expect(store.historyProgress(-1001)?.doneAt).not.toBeNull();
  });
});

// A picture pulled out of a room's history has no file behind it, so the only thing that says it was a
// picture is the line.
describe("what a line out of the history says it is", () => {
  test("a picture says so, whether or not it was captioned", () => {
    expect(historyMessageText(true, "")).toBe("[Photo]");
    expect(historyMessageText(true, "a diagram of the mechanism")).toBe("[Photo] a diagram of the mechanism");
  });

  test("and a message that is only words is left exactly as it was said", () => {
    expect(historyMessageText(false, "that is not quite it")).toBe("that is not quite it");
    expect(historyMessageText(false, "")).toBe("");
  });
});

// A room handed to the account that resolves to no peer is read by the MTProto library as a search over
// every chat the account is in, which would hand the whole account's history over under that one room.
describe("which rooms the account walks", () => {
  const otherRoomsMessages = () => {
    const { Api } = mtproto();

    return [101, 102, 103].map((id) => new Api.Message({ id, message: `another room's words ${id}`, date: 1_700_000_000 + id } as never));
  };
  // A client that resolves nothing, the way the library answers a number it has no peer for, and that
  // answers a history asked of no peer with messages from other chats.
  const unresolvingClient = (asked: unknown[]) =>
    ({
      getEntity: async () => undefined,
      getDialogs: async () => [],
      iterMessages: async function* (entity: unknown) {
        asked.push(entity);

        if (entity === undefined) {
          yield* otherRoomsMessages();
        }
      },
    }) as unknown as TelegramClient;

  test("the rooms the receiver names, the owner's own among them, and none where it names none", async () => {
    const named = [{ chatId: -1000000000003, oldestMessageId: 5, oldestSentAt: 1 }];

    expect(historyRooms(named, OWNER_ID).map((room) => room.chatId)).toEqual([-1000000000003, OWNER_ID]);

    const asked: unknown[] = [];
    const service = keptService({ rooms: "the receiver does not answer the route" });

    await walkAll({ client: unresolvingClient(asked), peers, service, store: new ReaderStore(":memory:") });
    expect(historyStanding()).toBe("the history walk stands down: the receiver does not answer the route");
    expect(asked).toEqual([]);
  });

  test("a walk asked of a room the receiver does not name is refused before the account is asked anything", async () => {
    const asked: unknown[] = [];
    const store = new ReaderStore(":memory:");

    await expect(walkNamedRoom({ client: unresolvingClient(asked), peers, service: keptService(), store }, -9_000_000_000_001)).rejects.toBeInstanceOf(
      HistoryUnreadable,
    );
    expect(asked).toEqual([]);
    expect(store.historyProgress(-9_000_000_000_001)).toBeNull();
  });

  test("a room whose peer the account cannot resolve hands nothing over rather than every chat's history", async () => {
    const asked: unknown[] = [];
    const service = keptService();
    const room = { chatId: -5000000002, oldestMessageId: null, oldestSentAt: null };

    await expect(walkRoom({ client: unresolvingClient(asked), peers, service, store: new ReaderStore(":memory:") }, room)).rejects.toBeInstanceOf(PeerUnresolved);
    expect(asked).toEqual([]);
    expect(service.handed).toEqual([]);
  });
});

// A room's recent past (docs/reader.md, 「The history walk」): handed over under the live reading's numbers,
// the owner's own lines among them, in a room the account numbers as the live reading does.
describe("a room's recent past", () => {
  const since = 1_700_000_000_000;
  const linesClient = (asked: unknown[]) => {
    const { Api } = mtproto();
    const lines = [
      new Api.Message({ id: 30, message: "the newest", date: 1_700_000_300 } as never),
      new Api.Message({ id: 29, out: true, message: "the owner's own", date: 1_700_000_200 } as never),
      new Api.Message({ id: 28, message: "", date: 1_700_000_100 } as never),
      new Api.Message({ id: 20, message: "before the span", date: 1_699_999_000 } as never),
    ];

    for (const line of lines) {
      Object.assign(line, { getSender: async () => new Api.User({ id: 1n, firstName: line.out === true ? "Owner" : "Person" } as never) });
    }

    return {
      getEntity: async (peer: number) => ({ id: peer }),
      getDialogs: async () => [],
      iterMessages: async function* (entity: unknown, options: unknown) {
        asked.push(entity, options);
        yield* lines;
      },
    } as unknown as TelegramClient;
  };
  const named = (chatId: number) => [{ chatId, oldestMessageId: -999_000_000, oldestSentAt: 1 }];

  test("a room whose people are the reader's is filled under the live numbers, the owner's lines as the owner's", async () => {
    const chatId = 5_000_000_123;
    const asked: unknown[] = [];
    const service = { ...keptService({ rooms: named(chatId) }), listened: async () => ({ listened: true, people: true, tells: false }) };
    const store = new ReaderStore(":memory:");

    const outcome = await fillNamedRoom({ client: linesClient(asked), peers, service, store }, chatId, since);

    expect(outcome).toEqual({ handed: 2, kept: 2, reachedTop: false });
    expect(
      service.handed.map((one) => (one.kind === "message" ? [one.messageId, one.message.senderId, one.message.origin] : null)),
    ).toEqual([
      [30, null, "history"],
      [29, OWNER_ID, "history"],
    ]);
    expect(store.historyProgress(chatId)).toBeNull();
  });

  test("a supergroup is filled whoever reads its people", async () => {
    const chatId = -1000000000003;
    const service = { ...keptService({ rooms: named(chatId) }), listened: async () => ({ listened: true, people: false, tells: false }) };

    const outcome = await fillNamedRoom({ client: linesClient([]), peers, service, store: new ReaderStore(":memory:") }, chatId, since);

    expect(outcome.handed).toBe(2);
  });

  test("a room the bot account numbers is refused before its past is read", async () => {
    const chatId = -5000000002;
    const asked: unknown[] = [];
    const service = { ...keptService({ rooms: named(chatId) }), listened: async () => ({ listened: true, people: false, tells: false }) };

    await expect(fillNamedRoom({ client: linesClient(asked), peers, service, store: new ReaderStore(":memory:") }, chatId, since)).rejects.toBeInstanceOf(
      FillUnnumbered,
    );
    expect(asked).toEqual([]);
    expect(service.handed).toEqual([]);
  });

  test("a room the receiver does not name is refused", async () => {
    const asked: unknown[] = [];

    await expect(fillNamedRoom({ client: linesClient(asked), peers, service: keptService(), store: new ReaderStore(":memory:") }, 5_000_000_123, since)).rejects.toBeInstanceOf(
      HistoryUnreadable,
    );
    expect(asked).toEqual([]);
  });
});
