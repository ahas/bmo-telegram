import { describe, expect, test } from "bun:test";

import { gatherPeople, PresenceTeller, presenceOf, RoomTells, tellsKeptMs, typingOf } from "../src/presence";
import { BOT_ID, keptService, OWNER_ID } from "./support";

import type { Clock, PeopleSources, PresenceHeard } from "../src/presence";

// Who is writing and who is reachable, as the owner's account hears them (docs/reader.md, 「Typing and presence」).

const peers = { ownerId: OWNER_ID, botId: BOT_ID };
const PERSON = 55;
const GROUP = -5_000_000_001;
const NOW = 1_700_000_000_000;

/** A clock that stands still until moved, running what was set to run once its moment has come. */
function standingClock(): Clock & { move(ms: number): void } {
  let now = NOW;
  let waiting: { at: number; run: () => void }[] = [];

  return {
    now: () => now,
    after: (ms, run) => {
      const one = { at: now + ms, run };

      waiting.push(one);

      return () => {
        waiting = waiting.filter((other) => other !== one);
      };
    },
    move: (ms) => {
      now += ms;
      const due = waiting.filter((one) => one.at <= now);

      waiting = waiting.filter((one) => one.at > now);
      due.forEach((one) => one.run());
    },
  };
}

const action = (className: string) => ({ className });
const fromUser = (userId: number) => ({ className: "PeerUser", userId });

describe("a typing action", () => {
  test("in a private chat is the person's mark in the room numbered by them", () => {
    expect(typingOf({ className: "UpdateUserTyping", userId: PERSON, action: action("SendMessageTypingAction") }, peers)).toEqual({
      roomId: PERSON,
      personId: PERSON,
      on: true,
    });
  });

  test("in a group or a channel names its room as the service numbers it, and a cancel is the mark off", () => {
    expect(typingOf({ className: "UpdateChatUserTyping", chatId: 5_000_000_001, fromId: fromUser(PERSON), action: action("SendMessageRecordAudioAction") }, peers)).toEqual({
      roomId: GROUP,
      personId: PERSON,
      on: true,
    });
    expect(typingOf({ className: "UpdateChannelUserTyping", channelId: 1234567890, fromId: fromUser(PERSON), action: action("SendMessageCancelAction") }, peers)).toEqual({
      roomId: -1001234567890,
      personId: PERSON,
      on: false,
    });
  });

  test("is no mark where it writes nothing, comes from no user, or is the account's own or the bot's", () => {
    expect(typingOf({ className: "UpdateUserTyping", userId: PERSON, action: action("SendMessageEmojiInteractionSeen") }, peers)).toBeNull();
    expect(typingOf({ className: "UpdateChannelUserTyping", channelId: 1, fromId: { className: "PeerChannel", channelId: 2 }, action: action("SendMessageTypingAction") }, peers)).toBeNull();
    expect(typingOf({ className: "UpdateChatUserTyping", chatId: 5_000_000_001, fromId: fromUser(OWNER_ID), action: action("SendMessageTypingAction") }, peers)).toBeNull();
    expect(typingOf({ className: "UpdateUserTyping", userId: BOT_ID, action: action("SendMessageTypingAction") }, peers)).toBeNull();
  });
});

describe("a status", () => {
  test("online until a moment to come is online, lapsing then", () => {
    expect(presenceOf({ className: "UserStatusOnline", expires: NOW / 1000 + 300 }, NOW)).toEqual({ state: "online", lastSeenAt: null, lapsesAt: NOW + 300_000 });
  });

  test("online until a moment gone is offline, last seen then", () => {
    expect(presenceOf({ className: "UserStatusOnline", expires: NOW / 1000 - 60 }, NOW)).toEqual({ state: "offline", lastSeenAt: NOW - 60_000, lapsesAt: null });
  });

  test("offline carries its moment, a coarse word none, and no status is nothing", () => {
    expect(presenceOf({ className: "UserStatusOffline", wasOnline: NOW / 1000 - 600 }, NOW)?.lastSeenAt).toBe(NOW - 600_000);

    for (const coarse of ["UserStatusRecently", "UserStatusLastWeek", "UserStatusLastMonth", "UserStatusEmpty"]) {
      expect(presenceOf({ className: coarse }, NOW)).toEqual({ state: "offline", lastSeenAt: null, lapsesAt: null });
    }

    expect(presenceOf(undefined, NOW)).toBeNull();
  });
});

describe("the presence told", () => {
  const teller = () => {
    const told: PresenceHeard[] = [];
    const clock = standingClock();

    return { told, clock, teller: new PresenceTeller(async (heard) => void told.push(heard), clock) };
  };

  test("is a whole list of everyone gathered, one with no status left out", async () => {
    const { told, teller: presence } = teller();

    await presence.whole(new Map<number, unknown>([[PERSON, { className: "UserStatusRecently" }], [56, undefined]]));

    expect(told).toEqual([{ people: [{ personId: PERSON, state: "offline", lastSeenAt: null }], whole: true }]);
  });

  test("tells a change of someone gathered alone, and passes over anyone else", async () => {
    const { told, teller: presence } = teller();

    await presence.whole(new Map([[PERSON, undefined]]));
    await presence.changed(PERSON, { className: "UserStatusOffline", wasOnline: NOW / 1000 });
    await presence.changed(99, { className: "UserStatusOffline", wasOnline: NOW / 1000 });

    expect(told.slice(1)).toEqual([{ people: [{ personId: PERSON, state: "offline", lastSeenAt: NOW }], whole: false }]);
  });

  test("goes offline by itself when online lapses, unless another status came first", async () => {
    const { told, clock, teller: presence } = teller();

    await presence.whole(new Map([[PERSON, { className: "UserStatusOnline", expires: NOW / 1000 + 300 }], [56, { className: "UserStatusOnline", expires: NOW / 1000 + 300 }]]));
    await presence.changed(56, { className: "UserStatusRecently" });
    clock.move(300_000);

    expect(told.at(-1)).toEqual({ people: [{ personId: PERSON, state: "offline", lastSeenAt: NOW + 300_000 }], whole: false });
    expect(told.filter((heard) => heard.people.some((one) => one.personId === 56 && one.lastSeenAt !== null))).toEqual([]);
  });
});

test("the service's word on whether a room tells is kept for a while, then asked again", async () => {
  const clock = standingClock();
  let asked = 0;
  const tells = new RoomTells(async () => {
    asked += 1;

    return true;
  }, clock);

  await tells.of(GROUP);
  clock.move(tellsKeptMs - 1);
  await tells.of(GROUP);
  clock.move(1);
  await tells.of(GROUP);

  expect(asked).toBe(2);
});

describe("the people told of", () => {
  const sources = (over: Partial<PeopleSources> = {}): PeopleSources => ({
    rooms: async () => ({ rooms: [OWNER_ID, PERSON, GROUP, -5_000_000_002].map((chatId) => ({ chatId, oldestMessageId: null, oldestSentAt: null })) }),
    tells: async (chatId) => chatId !== -5_000_000_002,
    personOf: async (chatId) => ({ id: chatId, status: "one-to-one" }),
    membersOf: async (chatId) => [
      { id: OWNER_ID, status: "owner" },
      { id: BOT_ID, status: "bot" },
      { id: 77, status: `member of ${chatId}` },
    ],
    ...over,
  });

  test("are the person of a one-to-one and a group's members, in rooms that tell, but the account and the bot", async () => {
    expect(await gatherPeople(sources(), peers)).toEqual(new Map<number, unknown>([[PERSON, "one-to-one"], [77, `member of ${GROUP}`]]));
  });

  test("are not gathered where the receiver does not name its rooms", async () => {
    const service = keptService({ rooms: "the receiver did not answer" });

    expect(await gatherPeople(sources({ rooms: service.rooms }), peers)).toBeNull();
  });
});
