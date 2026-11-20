import { afterAll, describe, expect, test } from "bun:test";

import { accountMembers } from "../src/members";
import { ChatUnresolved } from "../src/messages";
import { readerHandler } from "../src/routes";
import { BOT_ID, OWNER_ID, ROOM_ID } from "./support";

import type { TelegramClient } from "telegram";
import type { MemberEntry } from "../src/members";
import type { OwnerAccount } from "../src/owner";

// A chat's members as the owner's account lists them: the account itself under the owner's number, a deleted
// account left out, each person once, and the route that answers them while the account is connected.

const token = "seam-token";
const peers = { ownerId: OWNER_ID, botId: BOT_ID };

function entry(id: number, names: Partial<MemberEntry> = {}): MemberEntry {
  return { id, firstName: null, lastName: null, username: null, self: false, deleted: false, ...names };
}

describe("accountMembers", () => {
  test("lists each person once in Telegram's order, the account under the owner's number", () => {
    const listed = accountMembers(
      [
        entry(55, { firstName: "Ann", lastName: "Lee" }),
        entry(900, { firstName: "Me", self: true }),
        entry(66, { username: "handle" }),
        entry(77, { deleted: true }),
        entry(BOT_ID, { firstName: "Helper" }),
        entry(55, { firstName: "Ann" }),
      ],
      peers,
    );

    expect(listed).toEqual([
      { id: 55, name: "Ann Lee" },
      { id: OWNER_ID, name: "Me" },
      { id: 66, name: "@handle" },
      { id: BOT_ID, name: "Helper" },
    ]);
  });
});

describe("GET /reader/members", () => {
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
  let connected = true;

  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: 0,
    fetch: readerHandler({
      token,
      peers,
      owner,
      client: () => (connected ? ({} as TelegramClient) : null),
      rooms: async () => [],
      contacts: async () => [],
      start: async () => ({ outcome: "refused", reason: "never asked" }),
      avatars: () => [],
      messages: async () => [],
      members: async (_client, chatId) => {
        if (chatId !== ROOM_ID) throw new ChatUnresolved(chatId, new Error("no entity"));

        return [
          { id: OWNER_ID, name: "Me" },
          { id: 55, name: "Ann" },
        ];
      },
      missing: () => "the account's values are not set",
    }),
  });

  afterAll(() => {
    void server.stop(true);
  });

  const asked = (query: string): Promise<Response> =>
    fetch(`http://127.0.0.1:${server.port}/reader/members${query}`, { headers: { authorization: `Bearer ${token}` } });

  test("answers the chat's members", async () => {
    const answer = await asked(`?chat=${ROOM_ID}`);

    expect(answer.status).toBe(200);
    expect(await answer.json()).toEqual({ members: [{ id: OWNER_ID, name: "Me" }, { id: 55, name: "Ann" }] });
  });

  test("refuses a chat it cannot resolve, a chat that is no number, and an account not connected", async () => {
    const unresolved = await asked("?chat=-9");

    expect(unresolved.status).toBe(404);
    expect(((await unresolved.json()) as { error: string }).error).toContain("cannot resolve chat -9");
    expect((await asked("?chat=named")).status).toBe(400);

    connected = false;

    const down = await asked(`?chat=${ROOM_ID}`);

    connected = true;

    expect(down.status).toBe(503);
    expect(await down.json()).toEqual({ code: "not-ready", error: "the account's values are not set" });
  });
});
