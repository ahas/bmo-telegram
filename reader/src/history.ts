// The history walk (docs/reader.md, 「The history walk」): the past messages of the rooms the receiver
// names, read back on the owner's account and handed to the service as history, which the receiver keeps
// where it lacks them and wakes nobody for.

import { peerOf } from "./media";
import { mtproto } from "./mtproto";
import { fill, POLICY } from "./policy";
import { roomFacts, via } from "./service";

import type { TelegramClient } from "telegram";
import type { Entity } from "telegram/define";
import type { Peers } from "./media";
import type { HistoryRoom, Service } from "./service";
import type { ReaderStore } from "./store";

export interface HistoryDeps {
  client: TelegramClient;
  peers: Peers;
  service: Service;
  store: ReaderStore;
  // The pause between pages; Telegram's own pace by default.
  pause?: () => Promise<void>;
}

// A room the receiver does not name: the walk is asked only of the rooms it says the history of is
// wanted for, so a walk asked of another room by hand is refused before the account is asked anything.
export class HistoryUnreadable extends Error {
  constructor(chatId: number) {
    super(`room ${chatId} is not one the receiver names for the history walk`);
  }
}

// A peer the account cannot name even after loading its dialogs. The MTProto library reads a missing
// peer as no peer at all, and a history asked of no peer is a search over every chat the account is in,
// so an unresolved peer is an error and never an empty answer.
export class PeerUnresolved extends Error {
  constructor(peer: number) {
    super(`the owner's account cannot resolve peer ${peer}`);
  }
}

export interface HistoryOutcome {
  handed: number;
  kept: number;
  reachedTop: boolean;
}

const pageSize = 100;
const pauseBetweenPagesMs = 1_500;
const startDelayMs = 60_000;
// Where the owner's account's own numbering is stored: far below any number the Bot API hands out, so
// nothing collides, and rising with the account's numbers, so older still sorts first.
const foreignIdBase = -1_000_000_000;

// What a line out of the history says it is. A picture out of the history has no file behind it — the
// file is not fetched on the walk — so the line itself says it was one.
export function historyMessageText(hasPhoto: boolean, text: string): string {
  if (!hasPhoto) {
    return text;
  }

  return fill(POLICY.kinds.photo, { caption: text === "" ? "" : ` ${text}` });
}

// Only a supergroup or a channel numbers its messages once for everyone. A basic group and a private
// chat number them per account, so what the owner's account sees there cannot be lined up with what the
// bot read: such a chat's history is taken only from before the receiver's first message, and handed over
// under the account's own numbering, offset below every bot number.
export function isHistoryAligned(chatId: number): boolean {
  return String(chatId).startsWith("-100");
}

export function historyMessageId(chatId: number, ownId: number): number {
  return isHistoryAligned(chatId) ? ownId : foreignIdBase + ownId;
}

// A peer the account has not met in this session cannot be resolved from its number alone; loading the
// dialog list once fills the cache with everyone the account talks to, the bot included.
export async function resolveEntity(client: TelegramClient, peer: number): Promise<Entity> {
  let entity: Entity | undefined;

  try {
    entity = await client.getEntity(peer);
  } catch {
    entity = undefined;
  }

  if (entity === undefined || entity === null) {
    await client.getDialogs({ limit: 200 });
    entity = await client.getEntity(peer);
  }

  if (entity === undefined || entity === null) {
    throw new PeerUnresolved(peer);
  }

  return entity;
}

/** The rooms whose history may be walked: those the receiver names, the owner's own among them. */
export function historyRooms(named: readonly HistoryRoom[], ownerId: number): HistoryRoom[] {
  return named.some((room) => room.chatId === ownerId)
    ? [...named]
    : [...named, { chatId: ownerId, oldestMessageId: null, oldestSentAt: null }];
}

function senderNameOf(sender: unknown, fallback: string): string {
  if (sender instanceof mtproto().Api.User) {
    return sender.firstName ?? sender.username ?? fallback;
  }

  if (sender instanceof mtproto().Api.Chat || sender instanceof mtproto().Api.Channel) {
    return sender.title;
  }

  return fallback;
}

// Walks one room's history down to its top. Oldest-first is not possible over the API, so it walks
// newest to oldest and records how far it got, page by page; a walk cut short resumes there. In a
// supergroup it starts below the oldest message the receiver holds; in a room whose numbering is the
// account's own it starts below the receiver's oldest date instead, so nothing already held is handed
// over twice. Every message with words or a photo is handed over, whoever sent it.
export async function walkRoom(
  deps: HistoryDeps,
  room: HistoryRoom,
  options: { maxMessages?: number } = {},
): Promise<HistoryOutcome> {
  const { client, peers, service, store } = deps;
  const { chatId } = room;
  const aligned = isHistoryAligned(chatId);
  const progress = store.historyProgress(chatId);
  const oldestKnown = aligned ? room.oldestMessageId : null;
  const candidates = [progress?.reachedId, oldestKnown].filter((id): id is number => id !== undefined && id !== null && id !== 0);
  const offsetId = candidates.length === 0 ? 0 : Math.min(...candidates);
  const floorDate = aligned ? null : room.oldestSentAt;
  const entity = await resolveEntity(client, peerOf(chatId, peers));
  const names = new Map<number, string>();
  const pause = deps.pause ?? (async () => await Bun.sleep(pauseBetweenPagesMs));
  let handed = 0;
  let kept = 0;
  let seen = 0;
  let lowest = offsetId;
  let reachedTop = true;

  console.log(`history: room ${chatId} walking below ${offsetId === 0 ? (floorDate === null ? "the top" : "the receiver's first message") : `message ${offsetId}`}`);

  for await (const message of client.iterMessages(entity, {
    offsetId,
    offsetDate: offsetId === 0 && floorDate !== null ? floorDate : undefined,
    limit: undefined,
  })) {
    if (options.maxMessages !== undefined && seen >= options.maxMessages) {
      reachedTop = false;
      break;
    }

    seen += 1;
    lowest = lowest === 0 ? message.id : Math.min(lowest, message.id);

    if (!(message instanceof mtproto().Api.Message)) {
      continue;
    }

    const hasPhoto = message.photo !== undefined && message.photo !== null;
    const text = message.message ?? "";

    if ((text === "" && !hasPhoto) || (floorDate !== null && message.date >= floorDate)) {
      continue;
    }

    const senderId = message.senderId === undefined || message.senderId === null ? null : Number(message.senderId.toString());
    let senderName = senderId === null ? "unknown" : names.get(senderId);

    if (senderName === undefined && senderId !== null) {
      try {
        senderName = senderNameOf(await message.getSender(), String(senderId));
      } catch {
        senderName = String(senderId);
      }

      names.set(senderId, senderName);
    }

    const replyHeader = message.replyTo;
    const replyTo = replyHeader instanceof mtproto().Api.MessageReplyHeader ? (replyHeader.replyToMsgId ?? null) : null;
    const answer = await service.inbound({
      kind: "history",
      message: {
        chatId,
        messageId: historyMessageId(chatId, message.id),
        senderId,
        senderName: senderName ?? "unknown",
        sentAt: message.date,
        text: historyMessageText(hasPhoto, text),
        replyToMessageId: replyTo === null ? null : historyMessageId(chatId, replyTo),
        via,
        album: null,
        mode: "human-reader",
      },
      room: roomFacts(chatId, peers.ownerId, peers.botId),
    });

    handed += 1;
    kept += answer.kept === false ? 0 : 1;

    if (seen % pageSize === 0) {
      store.setHistoryReached(chatId, lowest);
      console.log(`history: room ${chatId} at message ${lowest}, ${kept} kept so far`);
      await pause();
    }
  }

  if (lowest !== 0) {
    store.setHistoryReached(chatId, lowest);
  }

  if (reachedTop) {
    store.markHistoryDone(chatId);
    console.log(`history: room ${chatId} reached the top of its history, ${kept} messages kept`);
  }

  return { handed, kept, reachedTop };
}

/** One room's walk asked by hand: refused where the receiver does not name the room. */
export async function walkNamedRoom(deps: HistoryDeps, chatId: number, options: { maxMessages?: number } = {}): Promise<HistoryOutcome> {
  const named = await deps.service.rooms();
  const room = "rooms" in named ? historyRooms(named.rooms, deps.peers.ownerId).find((one) => one.chatId === chatId) : undefined;

  if (room === undefined) {
    throw new HistoryUnreadable(chatId);
  }

  return await walkRoom(deps, room, options);
}

// Why the walk stands down, where it does; null while it walks or has walked.
let standing: string | null = null;

export function historyStanding(): string | null {
  return standing;
}

/** Every named room, one at a time, each to the top before the next begins; a room whose walk reached its top is not walked again. */
export async function walkAll(deps: HistoryDeps): Promise<void> {
  const named = await deps.service.rooms();

  if ("unanswered" in named) {
    standing = `the history walk stands down: ${named.unanswered}`;
    console.log(standing);

    return;
  }

  standing = null;

  for (const room of historyRooms(named.rooms, deps.peers.ownerId)) {
    if (deps.store.historyProgress(room.chatId)?.doneAt != null) {
      continue;
    }

    try {
      await walkRoom(deps, room);
    } catch (error) {
      console.error(`history: room ${room.chatId} stopped:`, error);
    }
  }
}

export function startHistory(deps: HistoryDeps): void {
  setTimeout(() => {
    void walkAll(deps).catch((error: unknown) => {
      console.error("history walk failed:", error);
    });
  }, startDelayMs);
}
