import { afterAll, describe, expect, test } from "bun:test";

import { accountRooms, roomLister } from "../src/dialogs";
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
};

// An account whose dialog list is the one given, counting how often it is read.
function accountWith(dialogs: { id: number; title?: string; self?: boolean }[]): { client: TelegramClient; reads: () => number } {
  let reads = 0;
  const client = {
    getDialogs: async () => {
      reads += 1;

      return dialogs.map((dialog) => ({
        id: { toString: () => String(dialog.id) },
        title: dialog.title,
        entity: { self: dialog.self === true },
      }));
    },
  } as unknown as TelegramClient;

  return { client, reads: () => reads };
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
      avatars: () => [],
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
          { id: OWNER_ID, title: "Saved Messages", self: true },
          { id: BOT_ID, title: "Helper", self: false },
          { id: ROOM_ID, title: "Board", self: false },
          { id: 55, title: "", self: false },
        ],
        peers,
      ),
    ).toEqual([
      { chatId: OWNER_ID, title: "Helper" },
      { chatId: ROOM_ID, title: "Board" },
      { chatId: 55, title: null },
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
  });

  test("are answered while the account is connected, and 503 not-ready while it is not", async () => {
    expect(await dialogsOf(up)).toEqual([200, { rooms: [{ chatId: ROOM_ID, title: "Board" }] }]);
    expect(await dialogsOf(down)).toEqual([503, { code: "not-ready", error: "the account's values are not set" }]);
  });
});
