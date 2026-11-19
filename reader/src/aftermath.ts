// What happens to a message after it was said, as the owner's account sees it (docs/reader.md, 「What it hands
// over」): a message deleted, and a message's reactions. A supergroup or a channel numbers its messages for
// everyone, so such a message is named by its number; in any other room the account numbers messages apart from
// the bot account, so a message is named by its stamp — who sent it and when — from what the reader saw of it
// while it ran.

import { numbersForEveryone } from "./numbering";

import type { Api } from "telegram";

/** Who sent a message and when, as the hand-over carried them. */
export interface Stamp {
  senderId: number;
  sentAt: number;
}

/** One person's reactions on a message. */
export interface PersonReactions {
  personId: number;
  emojis: string[];
}

/** A message the reader saw, by the account's own number for it. */
interface Seen extends Stamp {
  chatId: number;
}

/** The most messages remembered at once; the oldest are let go first. */
const REMEMBERED = 50_000;

/** What the reader saw of each message while it ran, by the account's own number for it. */
export class SeenMessages {
  private readonly seen = new Map<number, Seen>();

  remember(messageId: number, chatId: number, stamp: Stamp): void {
    this.seen.delete(messageId);
    this.seen.set(messageId, { chatId, ...stamp });

    if (this.seen.size > REMEMBERED) {
      const oldest = this.seen.keys().next().value;

      if (oldest !== undefined) {
        this.seen.delete(oldest);
      }
    }
  }

  of(messageId: number): Seen | undefined {
    return this.seen.get(messageId);
  }
}

/** A room's number as the bot account knows it, from the peer the account names it by. */
export function chatIdOfPeer(peer: { className: string; channelId?: unknown; chatId?: unknown; userId?: unknown }): number | null {
  switch (peer.className) {
    case "PeerChannel":
      return Number(`-100${String(peer.channelId)}`);
    case "PeerChat":
      return -Number(String(peer.chatId));
    case "PeerUser":
      return Number(String(peer.userId));
    default:
      return null;
  }
}

/** A deletion, grouped by room: each room with its messages by number or by stamp. */
export interface RoomDeletion {
  roomId: number;
  messageIds: number[];
  stamps: Stamp[];
}

/** The deletions an update names, each room's apart; a message the reader never saw is passed over. */
export function deletionsOf(update: { className: string; channelId?: unknown; messages: number[] }, seen: SeenMessages): RoomDeletion[] {
  if (update.className === "UpdateDeleteChannelMessages") {
    return [{ roomId: Number(`-100${String(update.channelId)}`), messageIds: [...update.messages], stamps: [] }];
  }

  const rooms = new Map<number, RoomDeletion>();

  for (const id of update.messages) {
    const known = seen.of(id);

    if (known === undefined) {
      continue;
    }

    const room = rooms.get(known.chatId) ?? { roomId: known.chatId, messageIds: [], stamps: [] };

    room.stamps.push({ senderId: known.senderId, sentAt: known.sentAt });
    rooms.set(known.chatId, room);
  }

  return [...rooms.values()];
}

/** A message's reactions as the account sees them now. */
export interface ReactionsSeen {
  roomId: number;
  messageId?: number;
  stamp?: Stamp;
  reactions: PersonReactions[];
  whole: boolean;
}

/** The reactions an update says a message carries; none where the message is named by nothing the reader saw. */
export function reactionsOf(update: Api.UpdateMessageReactions, seen: SeenMessages): ReactionsSeen | null {
  const roomId = chatIdOfPeer(update.peer as never);

  if (roomId === null) {
    return null;
  }

  const byPerson = new Map<number, string[]>();

  for (const recent of update.reactions.recentReactions ?? []) {
    const personId = chatIdOfPeer(recent.peerId as never);
    const reaction = recent.reaction as { className: string; emoticon?: string };

    if (personId === null || reaction.className !== "ReactionEmoji" || reaction.emoticon === undefined) {
      continue;
    }

    byPerson.set(personId, [...(byPerson.get(personId) ?? []), reaction.emoticon]);
  }

  const counted = update.reactions.results.reduce((total, result) => total + result.count, 0);
  const listed = [...byPerson.values()].reduce((total, emojis) => total + emojis.length, 0);
  const reactions = [...byPerson.entries()].map(([personId, emojis]) => ({ personId, emojis }));
  // Only where every reaction counted is listed by its person is the list the message's whole set.
  const whole = listed === counted;

  if (numbersForEveryone(roomId)) {
    return { roomId, messageId: update.msgId, reactions, whole };
  }

  const known = seen.of(update.msgId);

  return known === undefined ? null : { roomId, stamp: { senderId: known.senderId, sentAt: known.sentAt }, reactions, whole };
}
