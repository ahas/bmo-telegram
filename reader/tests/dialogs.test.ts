import { afterAll, describe, expect, test } from "bun:test";

import { accountRooms, roomLister } from "../src/dialogs";
import { contactLister } from "../src/contacts";
import { readerHandler } from "../src/routes";
import { BOT_ID, OWNER_ID, ROOM_ID } from "./support";

import type { TelegramClient } from "telegram";
import type { OwnerAccount } from "../src/owner";

// The rooms of the owner's account: its dialogs as the service numbers them, read again only once the list
// last read has gone stale, and the route that answers them while the account is connected.

const token = "seam-token";
const peers = { ownerId: OWNER_ID, botId: BOT_ID };
const owner: OwnerAccount = {
  say: async () => ({ id: null, text: "" }),
  album: async () => [],
  typing: async () => {},
  delete: async () => {},
  react: async () => {},
  pin: async () => null,
  edit: async () => ({ id: 0, text: "", edited: false }),
  line: async () => 0,
};

// One dialog of the stand-in account: its number and title, and what its chat says of itself.
interface Dialog {
  id: number;
  title?: string;
  self?: boolean;
  chat?: Record<string, unknown>;
}

// An account whose dialog list is the one given, counting how often it is read and keeping what each read asked.
function accountWith(dialogs: Dialog[]): { client: TelegramClient; reads: () => number; asked: unknown[] } {
  let reads = 0;
  const asked: unknown[] = [];
  const client = {
    getDialogs: async (params: unknown) => {
      reads += 1;
      asked.push(params);

      return dialogs.map((dialog) => ({
        id: { toString: () => String(dialog.id) },
        title: dialog.title,
        entity: { className: "User", self: dialog.self === true, ...dialog.chat },
      }));
    },
  } as unknown as TelegramClient;

  return { client, reads: () => reads, asked };
}

function served(connected: boolean): ReturnType<typeof Bun.serve> {
  const account = accountWith([{ id: ROOM_ID, title: "Board" }]);

  return Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch: readerHandler({
      token,
      peers,
      owner,
      client: () => (connected ? account.client : null),
      rooms: roomLister(peers),
      contacts: async () => [],
      start: async () => ({ outcome: "refused", reason: "never asked" }),
      avatars: () => [],
      messages: async () => [],
      missing: () => "the account's values are not set",
    }),
  });
}

const up = served(true);
const down = served(false);

afterAll(() => {
  void up.stop(true);
  void down.stop(true);
});

async function dialogsOf(server: ReturnType<typeof Bun.serve>): Promise<[number, unknown]> {
  const response = await fetch(`http://127.0.0.1:${server.port}/reader/dialogs`, { headers: { authorization: `Bearer ${token}` } });

  return [response.status, await response.json()];
}

describe("the rooms of the account", () => {
  test("lists the chat with the bot under the owner's number and leaves the saved messages out", () => {
    expect(
      accountRooms(
        [
          { id: OWNER_ID, title: "Saved Messages", self: true, room: true },
          { id: BOT_ID, title: "Helper", self: false, room: true },
          { id: ROOM_ID, title: "Board", self: false, room: true },
          { id: 55, title: "", self: false, room: true },
          { id: -77, title: "Old board", self: false, room: false },
        ],
        peers,
      ),
    ).toEqual([
      { chatId: OWNER_ID, title: "Helper" },
      { chatId: ROOM_ID, title: "Board" },
      { chatId: 55, title: null },
    ]);
  });

  test("lists every dialog asked whole, people, groups and the channels the account may post in", async () => {
    // A list past the hundred dialogs Telegram answers in one page of its own, every kind among it.
    const people = Array.from({ length: 140 }, (_, at) => ({ id: 1_000 + at, title: `Person ${at}` }));
    const account = accountWith([
      { id: OWNER_ID, title: "Saved Messages", self: true },
      { id: BOT_ID, title: "Helper" },
      ...people,
      { id: -5, title: "Group", chat: { className: "Chat", migratedTo: null } },
      { id: -6, title: "Became a supergroup", chat: { className: "Chat", migratedTo: { channelId: 9 } } },
      { id: -1_000_000_000_009, title: "Supergroup", chat: { className: "Channel", megagroup: true } },
      { id: -1_000_000_000_010, title: "Own channel", chat: { className: "Channel", broadcast: true, creator: true } },
      { id: -1_000_000_000_011, title: "Posting", chat: { className: "Channel", broadcast: true, adminRights: { postMessages: true } } },
      { id: -1_000_000_000_012, title: "Read only", chat: { className: "Channel", broadcast: true, adminRights: null } },
    ]);
    const rooms = await roomLister(peers)(account.client);

    expect(account.asked).toEqual([{}]);
    expect(rooms.length).toBe(1 + people.length + 4);
    expect(rooms.map((room) => room.chatId)).toEqual([
      OWNER_ID,
      ...people.map((person) => person.id),
      -5,
      -1_000_000_000_009,
      -1_000_000_000_010,
      -1_000_000_000_011,
    ]);
  });

  test("give a contact the one-to-one the account has with it", async () => {
    const account = accountWith([{ id: 1_001, title: "Someone" }, { id: ROOM_ID, title: "Board" }]);
    const entry = { firstName: null, lastName: null, username: null, bot: false, self: false, deleted: false };
    const contacts = contactLister(peers, roomLister(peers), async () => [
      { ...entry, id: 1_001, firstName: "Someone" },
      { ...entry, id: 1_002, firstName: "Nobody yet" },
    ]);

    expect((await contacts(account.client)).map((contact) => [contact.id, contact.chatId])).toEqual([
      [1_001, 1_001],
      [1_002, null],
    ]);
  });

  test("reads the account again only once the list last read is stale", async () => {
    const account = accountWith([{ id: ROOM_ID, title: "Board" }, { id: OWNER_ID, self: true }]);
    let now = 0;
    const list = roomLister(peers, () => now);

    expect(await list(account.client)).toEqual([{ chatId: ROOM_ID, title: "Board" }]);
    now = 29_999;
    await list(account.client);
    expect(account.reads()).toBe(1);
    now = 30_000;
    await list(account.client);
    expect(account.reads()).toBe(2);
    list.forget();
    await list(account.client);
    expect(account.reads()).toBe(3);
  });

  test("are answered while the account is connected, and 503 not-ready while it is not", async () => {
    expect(await dialogsOf(up)).toEqual([200, { rooms: [{ chatId: ROOM_ID, title: "Board" }] }]);
    expect(await dialogsOf(down)).toEqual([503, { code: "not-ready", error: "the account's values are not set" }]);
  });
});
