import { afterEach, describe, expect, test } from "bun:test";
import bigInt from "big-integer";
import { Api } from "telegram";

import { avatarFilePath, avatarSubjects, needsFetch, photoIdOf, refreshAvatar, sweepAvatars } from "../src/avatars";
import { ReaderStore } from "../src/store";
import { BOT_ID as botId, keptService, OWNER_ID as ownerId, ROOM_ID, scratch, sweepScratch } from "./support";

import type { TelegramClient } from "telegram";
import type { AvatarDeps } from "../src/avatars";
import type { KeptService } from "./support";

afterEach(sweepScratch);

function user(id: number, photoId: number | null): Api.User {
  return new Api.User({
    id: bigInt(id),
    photo: photoId === null ? new Api.UserProfilePhotoEmpty() : new Api.UserProfilePhoto({ photoId: bigInt(photoId), dcId: 2 }),
  } as unknown as ConstructorParameters<typeof Api.User>[0]);
}

function channel(id: number, photoId: number | null): Api.Channel {
  return new Api.Channel({
    id: bigInt(id),
    title: "Room",
    photo: photoId === null ? new Api.ChatPhotoEmpty() : new Api.ChatPhoto({ photoId: bigInt(photoId), dcId: 2 }),
  } as unknown as ConstructorParameters<typeof Api.Channel>[0]);
}

// An account that answers with the entities it was given, hands over each room's members, and counts what
// was downloaded from it.
function fakeClient(entities: Map<number, unknown>, members: Map<number, Api.User[]>): { client: TelegramClient; downloads: number[] } {
  const downloads: number[] = [];
  const client = {
    getEntity: (peer: number) => {
      const entity = entities.get(Number(peer));

      return entity === undefined ? Promise.reject(new Error(`no entity ${peer}`)) : Promise.resolve(entity);
    },
    getDialogs: () => Promise.resolve([]),
    getParticipants: (peer: number) => Promise.resolve(members.get(Number(peer)) ?? []),
    downloadProfilePhoto: (entity: { id: { toString: () => string } }) => {
      downloads.push(Number(entity.id.toString()));

      return Promise.resolve(Buffer.from("picture"));
    },
  } as unknown as TelegramClient;

  return { client, downloads };
}

type Suite = AvatarDeps & { downloads: number[]; service: KeptService };

function deps(
  entities: Map<number, unknown>,
  members: Map<number, Api.User[]> = new Map(),
  seen: { rooms: number[]; people: number[] } = { rooms: [], people: [] },
): Suite {
  const { client, downloads } = fakeClient(entities, members);

  return { client, downloads, store: new ReaderStore(":memory:"), service: keptService({ seen }), avatarDir: scratch("avatars"), ownerId, botId };
}

describe("whose picture is worth having", () => {
  test("every room and everyone who has spoken, the bot among them, each named once", () => {
    const subjects = avatarSubjects({ chatIds: [ROOM_ID, ROOM_ID, ownerId], senderIds: [7, 7, ownerId], ownerId, botId });

    expect(subjects).toEqual([
      { kind: "room", subjectId: ROOM_ID, peer: ROOM_ID },
      { kind: "room", subjectId: ownerId, peer: botId },
      { kind: "person", subjectId: 7, peer: 7 },
      { kind: "person", subjectId: ownerId, peer: ownerId },
      { kind: "person", subjectId: botId, peer: botId },
    ]);
  });

  test("the owner's own chat wears the bot's picture and the owner keeps their own where they speak", () => {
    const subjects = avatarSubjects({ chatIds: [ownerId], senderIds: [ownerId], ownerId, botId });

    expect(subjects.find((subject) => subject.kind === "room")?.peer).toBe(botId);
    expect(subjects.find((subject) => subject.kind === "person")?.peer).toBe(ownerId);
  });

  test("leaves out what the provider has never seen", async () => {
    const suite = deps(new Map<number, unknown>([[ROOM_ID, channel(100, 88)], [botId, user(botId, 99)], [9, user(9, 5)]]), new Map(), {
      rooms: [ROOM_ID],
      people: [],
    });

    await sweepAvatars(suite);
    expect(suite.downloads.sort()).toEqual([100, botId].sort());
    expect(suite.store.getAvatar("person", 9)).toBeNull();
  });
});

describe("which picture Telegram is showing", () => {
  test("is the number on the account's photo, for a person and for a room alike", () => {
    expect(photoIdOf(user(5, 77))).toBe("77");
    expect(photoIdOf(channel(9, 88))).toBe("88");
  });

  test("is nothing at all when the account shows none", () => {
    expect(photoIdOf(user(5, null))).toBeNull();
    expect(photoIdOf(channel(9, null))).toBeNull();
    expect(photoIdOf(null)).toBeNull();
    expect(photoIdOf({ photo: { photoId: bigInt(1) } })).toBeNull();
  });
});

describe("whether a picture has to be taken again", () => {
  const held = { kind: "person" as const, subjectId: 7, photoId: "77", path: "/somewhere/7.jpg", updatedAt: 1 };

  test("not while it is the same picture and the file is there", () => {
    expect(needsFetch(held, "77", true)).toBe(false);
  });

  test("when it is a picture nobody has taken yet", () => {
    expect(needsFetch(null, "77", false)).toBe(true);
  });

  test("when the picture has changed", () => {
    expect(needsFetch(held, "78", true)).toBe(true);
  });

  test("when the file the record points at is gone", () => {
    expect(needsFetch(held, "77", false)).toBe(true);
    expect(needsFetch({ ...held, path: null }, "77", true)).toBe(true);
  });
});

describe("taking one subject's picture", () => {
  test("stores the file, records which picture it is, and does not take it twice", async () => {
    const suite = deps(new Map<number, unknown>([[7, user(7, 77)]]));
    const subject = { kind: "person" as const, subjectId: 7, peer: 7 };

    expect(await refreshAvatar(suite, subject)).toBe("taken");

    const held = suite.store.getAvatar("person", 7);

    expect(held?.photoId).toBe("77");
    expect(held?.path).toBe(avatarFilePath(suite.avatarDir, "person", 7));
    expect(await Bun.file(held?.path ?? "").text()).toBe("picture");
    expect(suite.downloads).toEqual([7]);

    expect(await refreshAvatar(suite, subject)).toBe("unchanged");
    expect(suite.downloads).toEqual([7]);
  });

  test("takes it again once the picture behind it has changed", async () => {
    const entities = new Map<number, unknown>([[7, user(7, 77)]]);
    const suite = deps(entities);
    const subject = { kind: "person" as const, subjectId: 7, peer: 7 };

    await refreshAvatar(suite, subject);
    entities.set(7, user(7, 78));

    expect(await refreshAvatar(suite, subject)).toBe("taken");
    expect(suite.store.getAvatar("person", 7)?.photoId).toBe("78");
    expect(suite.downloads).toEqual([7, 7]);
  });

  test("records a subject that shows none, and takes down the file it had", async () => {
    const entities = new Map<number, unknown>([[7, user(7, 77)]]);
    const suite = deps(entities);
    const subject = { kind: "person" as const, subjectId: 7, peer: 7 };

    await refreshAvatar(suite, subject);

    const path = avatarFilePath(suite.avatarDir, "person", 7);

    entities.set(7, user(7, null));

    expect(await refreshAvatar(suite, subject)).toBe("none");

    const held = suite.store.getAvatar("person", 7);

    expect(held?.photoId).toBeNull();
    expect(held?.path).toBeNull();
    expect(await Bun.file(path).exists()).toBe(false);
  });

  test("fails on its own when the account cannot resolve the subject", async () => {
    const suite = deps(new Map<number, unknown>());

    expect(await refreshAvatar(suite, { kind: "room", subjectId: ROOM_ID, peer: ROOM_ID })).toBe("failed");
    expect(suite.store.getAvatar("room", ROOM_ID)).toBeNull();
  });
});

describe("one pass over everyone", () => {
  test("takes what it can, counts the rest, keeps a room and a person of one number apart, and says what changed once", async () => {
    const entities = new Map<number, unknown>([
      [ROOM_ID, channel(100, 88)],
      [ownerId, user(ownerId, 55)],
      [botId, user(botId, 99)],
    ]);
    const suite = deps(entities, new Map(), { rooms: [ROOM_ID, ownerId], people: [ownerId, botId] });

    expect(await sweepAvatars(suite)).toEqual({ taken: 4, unchanged: 0, none: 0, failed: 0 });
    // The owner's one-to-one is numbered by the owner's own account: as a room it wears the bot's picture,
    // as a person it wears the owner's.
    expect(suite.store.getAvatar("room", ownerId)?.photoId).toBe("99");
    expect(suite.store.getAvatar("person", ownerId)?.photoId).toBe("55");
    expect(suite.store.getAvatar("room", ROOM_ID)?.photoId).toBe("88");
    expect(suite.service.changed).toEqual([{ rooms: [ROOM_ID, ownerId], people: [ownerId, botId] }]);

    expect(await sweepAvatars(suite)).toEqual({ taken: 0, unchanged: 4, none: 0, failed: 0 });
    expect(suite.service.changed).toHaveLength(1);
  });

  test("takes the picture of someone the account knows only as a member of a room", async () => {
    const stranger = user(777, 33);
    const suite = deps(
      new Map<number, unknown>([
        [ROOM_ID, channel(100, 88)],
        [botId, user(botId, 99)],
      ]),
      new Map<number, Api.User[]>([[ROOM_ID, [stranger]]]),
      { rooms: [ROOM_ID], people: [777] },
    );

    const sweep = await sweepAvatars(suite);

    expect(sweep.failed).toBe(0);
    expect(suite.store.getAvatar("person", 777)?.photoId).toBe("33");
  });
});

test("an unchanged photo repairs a moved data folder without downloading it again", async () => {
  const suite = deps(new Map<number, unknown>([[7, user(7, 77)]]));
  const subject = { kind: "person" as const, subjectId: 7, peer: 7 };

  await refreshAvatar(suite, subject);

  const held = suite.store.getAvatar("person", 7)!;

  suite.store.saveAvatar({ ...held, path: "/removed-folder/avatars/person/7.jpg" });
  expect(await refreshAvatar(suite, subject)).toBe("unchanged");
  expect(suite.store.getAvatar("person", 7)?.path).toBe(held.path);
  expect(suite.store.getAvatar("person", 7)!.updatedAt).toBeGreaterThan(held.updatedAt);
  expect(suite.downloads).toEqual([7]);
});
