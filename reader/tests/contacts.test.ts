import { afterAll, describe, expect, test } from "bun:test";

import { accountContacts, contactLister, startChat } from "../src/contacts";
import { readerHandler } from "../src/routes";
import { BOT_ID, OWNER_ID, ROOM_ID } from "./support";

import type { TelegramClient } from "telegram";
import type { AccountContact, ContactEntry, StartAsked, Starting } from "../src/contacts";
import type { OwnerAccount } from "../src/owner";

// The owner's account's contacts and a conversation started with them: the contact list as the service lists
// it, read again only once stale, a one-to-one that sends nothing, a group made only of contacts, and the two
// routes that answer them while the account is connected.

const token = "seam-token";
const peers = { ownerId: OWNER_ID, botId: BOT_ID };
const owner: OwnerAccount = {
  say: async () => ({ id: null, text: "" }),
  album: async () => [],
  typing: async () => {},
  delete: async () => {},
  react: async () => {},
};
const client = {} as TelegramClient;

function entry(id: number, names: Partial<ContactEntry> = {}): ContactEntry {
  return { id, firstName: null, lastName: null, username: null, bot: false, self: false, deleted: false, ...names };
}

const listed: AccountContact[] = [
  { id: 501, name: "Ann Lee", bot: false, chatId: 501 },
  { id: 502, name: "@bo", bot: false, chatId: null },
  { id: BOT_ID, name: "Helper", bot: true, chatId: OWNER_ID },
];

// A start over the contacts listed, keeping each group made and whether the rooms were forgotten.
function starting(fails: string | null = null): Starting & { made: unknown[]; forgot: () => number } {
  const made: unknown[] = [];
  let forgot = 0;

  return {
    made,
    forgot: () => forgot,
    contacts: async () => listed,
    makeGroup: async (title, ids) => {
      if (fails !== null) throw new Error(fails);

      made.push({ title, ids });

      return { chatId: -4_000_001, title };
    },
    forgetRooms: () => {
      forgot += 1;
    },
  };
}

describe("the contacts of the account", () => {
  test("are named and given their one-to-one, the account itself and deleted accounts left out", () => {
    expect(
      accountContacts(
        [
          entry(OWNER_ID, { firstName: "Me", self: true }),
          entry(501, { firstName: "Ann", lastName: "Lee" }),
          entry(502, { username: "bo" }),
          entry(503),
          entry(504, { firstName: "Gone", deleted: true }),
          entry(BOT_ID, { firstName: "Helper", bot: true }),
        ],
        [
          { chatId: 501, title: "Ann Lee" },
          { chatId: OWNER_ID, title: "Helper" },
          { chatId: ROOM_ID, title: "Board" },
        ],
        peers,
      ),
    ).toEqual([
      { id: 501, name: "Ann Lee", bot: false, chatId: 501 },
      { id: 502, name: "@bo", bot: false, chatId: null },
      { id: 503, name: "503", bot: false, chatId: null },
      { id: BOT_ID, name: "Helper", bot: true, chatId: OWNER_ID },
    ]);
  });

  test("are read off the account again only once the list last read is stale, the rooms each time", async () => {
    let reads = 0;
    let roomReads = 0;
    let now = 0;
    const list = contactLister(
      peers,
      async () => {
        roomReads += 1;

        return roomReads === 1 ? [] : [{ chatId: 501, title: null }];
      },
      async () => {
        reads += 1;

        return [entry(501, { firstName: "Ann" })];
      },
      () => now,
    );

    expect(await list(client)).toEqual([{ id: 501, name: "Ann", bot: false, chatId: null }]);
    now = 29_999;
    expect(await list(client)).toEqual([{ id: 501, name: "Ann", bot: false, chatId: 501 }]);
    expect(reads).toBe(1);
    now = 30_000;
    await list(client);
    expect(reads).toBe(2);
  });
});

describe("a conversation started", () => {
  test("with one contact comes to its one-to-one and makes nothing", async () => {
    const start = starting();

    expect(await startChat({ contactId: 501 }, start, peers)).toEqual({ outcome: "started", chatId: 501, title: "Ann Lee" });
    expect(await startChat({ contactId: 502 }, start, peers)).toEqual({ outcome: "started", chatId: 502, title: "@bo" });
    expect(await startChat({ contactId: BOT_ID }, start, peers)).toEqual({ outcome: "started", chatId: OWNER_ID, title: "Helper" });
    expect(start.made).toEqual([]);
    expect(start.forgot()).toBe(0);
  });

  test("with a number that is no contact is answered so, and no group is made", async () => {
    const start = starting();

    expect(await startChat({ contactId: 999 }, start, peers)).toEqual({ outcome: "no-contact", contactId: 999 });
    expect(await startChat({ title: "Plans", contactIds: [501, 998, 997] }, start, peers)).toEqual({ outcome: "no-contact", contactId: 998 });
    expect(start.made).toEqual([]);
  });

  test("as a group is made of the contacts under the title, and the rooms are read again", async () => {
    const start = starting();

    expect(await startChat({ title: "Plans", contactIds: [501, BOT_ID] }, start, peers)).toEqual({ outcome: "started", chatId: -4_000_001, title: "Plans" });
    expect(start.made).toEqual([{ title: "Plans", ids: [501, BOT_ID] }]);
    expect(start.forgot()).toBe(1);
  });

  test("as a group Telegram refuses is answered with Telegram's words", async () => {
    const start = starting("USER_PRIVACY_RESTRICTED");

    expect(await startChat({ title: "Plans", contactIds: [501] }, start, peers)).toEqual({ outcome: "refused", reason: "USER_PRIVACY_RESTRICTED" });
    expect(start.forgot()).toBe(0);
  });
});

function served(connected: boolean, asked: StartAsked[]): ReturnType<typeof Bun.serve> {
  return Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch: readerHandler({
      token,
      peers,
      owner,
      client: () => (connected ? client : null),
      rooms: async () => [],
      contacts: async () => listed,
      start: async (_, one) => {
        asked.push(one);

        return { outcome: "started", chatId: 501, title: "Ann Lee" };
      },
      avatars: () => [],
      messages: async () => [],
      missing: () => "the account's values are not set",
    }),
  });
}

const asked: StartAsked[] = [];
const up = served(true, asked);
const down = served(false, asked);

afterAll(() => {
  void up.stop(true);
  void down.stop(true);
});

async function call(server: ReturnType<typeof Bun.serve>, method: string, path: string, body?: unknown): Promise<[number, unknown]> {
  const response = await fetch(`http://127.0.0.1:${server.port}${path}`, {
    method,
    headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

  return [response.status, await response.json()];
}

describe("the routes", () => {
  test("answer the contacts while the account is connected, and 503 not-ready while it is not", async () => {
    expect(await call(up, "GET", "/reader/contacts")).toEqual([200, { contacts: listed }]);
    expect(await call(down, "GET", "/reader/contacts")).toEqual([503, { code: "not-ready", error: "the account's values are not set" }]);
  });

  test("start a conversation of either shape and refuse anything else", async () => {
    expect(await call(up, "POST", "/reader/start", { contactId: 501 })).toEqual([200, { outcome: "started", chatId: 501, title: "Ann Lee" }]);
    expect((await call(up, "POST", "/reader/start", { title: "Plans", contactIds: [501, 502] }))[0]).toBe(200);
    for (const body of [{}, { contactId: "501" }, { contactId: 501, title: "Plans" }, { title: "Plans" }, { title: "Plans", contactIds: ["501"] }]) {
      expect((await call(up, "POST", "/reader/start", body))[0]).toBe(400);
    }
    expect((await call(down, "POST", "/reader/start", { contactId: 501 }))[0]).toBe(503);
    expect(asked).toEqual([{ contactId: 501 }, { title: "Plans", contactIds: [501, 502] }]);
  });
});
