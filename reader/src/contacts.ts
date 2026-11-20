// The owner's account's contacts and a conversation started with them (docs/reader.md, 「Contacts and starting a
// conversation」): the contact list, each with the one-to-one the account has with it, read again at most every
// thirty seconds; a one-to-one started by naming a contact, and a group made with contacts under a title.

import bigInt from "big-integer";

import { mtproto } from "./mtproto";

import type { Api, TelegramClient } from "telegram";
import type { AccountRoom } from "./dialogs";
import type { Peers } from "./media";

/** One contact as the service lists it: its number, its name, whether it is a bot, and its one-to-one. */
export interface AccountContact {
  id: number;
  name: string;
  bot: boolean;
  chatId: number | null;
}

/** One user of the account's contact list, as Telegram gives it. */
export interface ContactEntry {
  id: number;
  firstName: string | null;
  lastName: string | null;
  username: string | null;
  bot: boolean;
  self: boolean;
  deleted: boolean;
}

/** A conversation to start: a one-to-one with a contact, or a group of contacts under a title. */
export type StartAsked = { contactId: number } | { title: string; contactIds: number[] };

/** What a start came to: the room and its title, a number that is no contact, or Telegram's refusal. */
export type StartOutcome =
  | { outcome: "started"; chatId: number; title: string | null }
  | { outcome: "no-contact"; contactId: number }
  | { outcome: "refused"; reason: string };

// How long a contact list read off the account answers before it is read again.
const freshForMs = 30_000;

// The number the service gives the one-to-one with a user: the chat with the bot is the owner's one-to-one.
function roomOf(id: number, peers: Peers): number {
  return id === peers.botId ? peers.ownerId : id;
}

/** The name the account shows for a user: their first and last name, else their handle, else their number. */
export function nameOf(entry: Pick<ContactEntry, "id" | "firstName" | "lastName" | "username">): string {
  const name = [entry.firstName, entry.lastName].filter((part) => part !== null && part !== "").join(" ");

  if (name !== "") return name;

  return entry.username !== null && entry.username !== "" ? `@${entry.username}` : String(entry.id);
}

/** The contacts as the service lists them, each with its one-to-one where the account's rooms hold one. */
export function accountContacts(entries: readonly ContactEntry[], rooms: readonly AccountRoom[], peers: Peers): AccountContact[] {
  const held = new Set(rooms.map((room) => room.chatId));

  return entries
    .filter((entry) => !entry.self && !entry.deleted)
    .map((entry) => {
      const room = roomOf(entry.id, peers);

      return { id: entry.id, name: nameOf(entry), bot: entry.bot, chatId: held.has(room) ? room : null };
    });
}

// The users of the account's contact list, each with the user Telegram gave, to bring into a group.
async function contactUsers(client: TelegramClient): Promise<Api.User[]> {
  const Api = mtproto().Api;
  const listed = await client.invoke(new Api.contacts.GetContacts({ hash: bigInt(0) }));

  if (!(listed instanceof Api.contacts.Contacts)) return [];

  return listed.users.filter((user): user is Api.User => user instanceof Api.User);
}

function entryOf(user: Api.User): ContactEntry {
  return {
    id: Number(user.id.toString()),
    firstName: user.firstName ?? null,
    lastName: user.lastName ?? null,
    username: user.username ?? null,
    bot: user.bot === true,
    self: user.self === true,
    deleted: user.deleted === true,
  };
}

/** The account's contacts, the list last read answering while it is fresh. `now` is the clock, for tests. */
export function contactLister(
  peers: Peers,
  rooms: (client: TelegramClient) => Promise<AccountRoom[]>,
  read: (client: TelegramClient) => Promise<ContactEntry[]> = async (client) => (await contactUsers(client)).map(entryOf),
  now: () => number = Date.now,
): (client: TelegramClient) => Promise<AccountContact[]> {
  let last: { at: number; entries: ContactEntry[] } | null = null;

  return async (client) => {
    if (last === null || now() - last.at >= freshForMs) last = { at: now(), entries: await read(client) };

    return accountContacts(last.entries, await rooms(client), peers);
  };
}

// The group Telegram made, as the service numbers a basic group: its number negated.
function madeGroup(made: Api.messages.TypeInvitedUsers | Api.TypeUpdates): { chatId: number; title: string | null } | null {
  const updates = "updates" in made && !("chats" in made) ? made.updates : made;
  const chats = "chats" in updates ? updates.chats : [];
  const chat = chats.find((one): one is Api.Chat => one.className === "Chat");

  return chat === undefined ? null : { chatId: -Number(chat.id.toString()), title: chat.title ?? null };
}

/** What starts a conversation on the account: the contacts as listed, and the group made with given users. */
export interface Starting {
  contacts: () => Promise<AccountContact[]>;
  makeGroup: (title: string, contactIds: number[]) => Promise<{ chatId: number; title: string | null }>;
  // The list of rooms read again on the next page asked.
  forgetRooms: () => void;
}

/** A conversation started: a one-to-one comes to its room without a word sent, a group is made by Telegram. */
export async function startChat(asked: StartAsked, starting: Starting, peers: Peers): Promise<StartOutcome> {
  const contacts = await starting.contacts();
  const named = "contactId" in asked ? [asked.contactId] : asked.contactIds;
  const missing = named.find((id) => !contacts.some((contact) => contact.id === id));

  if (missing !== undefined) return { outcome: "no-contact", contactId: missing };

  if ("contactId" in asked) {
    const contact = contacts.find((one) => one.id === asked.contactId) as AccountContact;

    return { outcome: "started", chatId: contact.chatId ?? roomOf(contact.id, peers), title: contact.name };
  }

  try {
    const made = await starting.makeGroup(asked.title, asked.contactIds);

    starting.forgetRooms();

    return { outcome: "started", ...made };
  } catch (error) {
    return { outcome: "refused", reason: error instanceof Error ? error.message : String(error) };
  }
}

/** The group made on the connected account with the contacts named, as their users, under the title. */
export async function makeGroup(client: TelegramClient, title: string, contactIds: number[]): Promise<{ chatId: number; title: string | null }> {
  const users = (await contactUsers(client)).filter((user) => contactIds.includes(Number(user.id.toString())));
  const made = madeGroup(await client.invoke(new (mtproto().Api.messages.CreateChat)({ users, title })));

  if (made === null) throw new Error("Telegram made the group but named no group in its answer");

  return made;
}
