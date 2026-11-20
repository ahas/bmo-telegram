// A chat's members as the owner's account lists them (docs/reader.md, 「A room's members」): everyone Telegram
// shows the account in a group, a supergroup or a channel, and the person and the owner of a one-to-one, each
// under their Telegram number and the name the account shows for them. Nothing is kept.

import { nameOf } from "./contacts";
import { resolveEntity } from "./history";
import { peerOf } from "./media";
import { ChatUnresolved } from "./messages";
import { mtproto } from "./mtproto";

import type { Api, TelegramClient } from "telegram";
import type { ContactEntry } from "./contacts";
import type { Peers } from "./media";

/** One member as the service lists them: their number and the name the account shows for them. */
export interface AccountMember {
  id: number;
  name: string;
}

/** One user Telegram listed in a chat, as the account sees them. */
export type MemberEntry = Omit<ContactEntry, "bot">;

/** The members as the service lists them: the account itself under the owner's number, a deleted account left
 * out, and each person once, in the order Telegram listed them. */
export function accountMembers(entries: readonly MemberEntry[], peers: Peers): AccountMember[] {
  const listed = new Set<number>();

  return entries.flatMap((entry) => {
    const id = entry.self ? peers.ownerId : entry.id;

    if (entry.deleted || listed.has(id)) return [];

    listed.add(id);

    return [{ id, name: nameOf(entry) }];
  });
}

function entryOf(user: Api.User): MemberEntry {
  return {
    id: Number(user.id.toString()),
    firstName: user.firstName ?? null,
    lastName: user.lastName ?? null,
    username: user.username ?? null,
    self: user.self === true,
    deleted: user.deleted === true,
  };
}

/** Everyone the account lists in a chat, read off the live account. Telegram answers a one-to-one with its person
 * alone, so the account itself is added to it. A chat number the account cannot resolve is refused by
 * `ChatUnresolved`. */
export async function chatMembers(client: TelegramClient, peers: Peers, chatId: number): Promise<AccountMember[]> {
  const { Api } = mtproto();
  let entity;

  try {
    entity = await resolveEntity(client, peerOf(chatId, peers));
  } catch (error) {
    throw new ChatUnresolved(chatId, error);
  }

  const participants = await client.getParticipants(entity, {});
  const users = participants.filter((user): user is Api.User => user instanceof Api.User);
  const listed = entity instanceof Api.User ? [...users, await client.getMe()] : users;

  return accountMembers(listed.map(entryOf), peers);
}
