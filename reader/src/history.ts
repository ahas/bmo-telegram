// The history walk (docs/reader.md, 「The history walk」): the past messages of the rooms the receiver
// names, read back on the owner's account and handed to the service as history, which the receiver keeps
// where it lacks them and wakes nobody for; and a room's recent past filled the same way, under the numbers
// the live reading gives its messages.

import { peerOf } from "./media";
import { mtproto } from "./mtproto";
import { fill, POLICY } from "./policy";
import { handedMessage, roomFacts } from "./service";

import type { Api, TelegramClient } from "telegram";
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

// A room whose live numbers are the bot account's: the account's numbers for its messages cannot be lined up
// with them, so its recent past is not filled.
export class FillUnnumbered extends Error {
  constructor(chatId: number) {
    super(`room ${chatId} is numbered by the bot account's reading; its recent past cannot be lined up with it`);
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

// The account's own number for a message the provider holds, where the room numbers messages per account and
// the message was read back from the room's past. Every other number crosses as it stands.
export function accountMessageId(chatId: number, messageId: number): number {
  return !isHistoryAligned(chatId) && messageId <= foreignIdBase ? messageId - foreignIdBase : messageId;
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

export function senderNameOf(sender: unknown, fallback: string): string {
  if (sender instanceof mtproto().Api.User) {
    return sender.firstName ?? sender.username ?? fallback;
  }

  if (sender instanceof mtproto().Api.Chat || sender instanceof mtproto().Api.Channel) {
    return sender.title;
  }

  return fallback;
}

// The number of the one who said a line: the owner's for the account's own, as the bot account knows the
// owner, and the sender's own number for anyone else's.
export function senderIdOf(message: Api.Message, peers: Peers): number | null {
  if (message.out === true) {
    return peers.ownerId;
  }

  return message.senderId === undefined || message.senderId === null ? null : Number(message.senderId.toString());
}

// One line handed over as history under `messageId`, the message it answers under the number `number` gives
// it; whether the receiver kept it.
async function handLine(
  deps: HistoryDeps,
  chatId: number,
  message: Api.Message,
  messageId: number,
  number: (ownId: number) => number,
  names: Map<number, string>,
): Promise<boolean> {
  const { peers, service } = deps;
  const hasPhoto = message.photo !== undefined && message.photo !== null;
  const text = message.message ?? "";
  const senderId = senderIdOf(message, peers);
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
    kind: "message",
    roomId: chatId,
    messageId,
    message: handedMessage({
      chatId,
      messageId,
      origin: "history",
      senderId,
      senderName: senderName ?? "unknown",
      sentAt: message.date,
      text: historyMessageText(hasPhoto, text),
      replyToMessageId: replyTo === null ? null : number(replyTo),
      room: roomFacts(chatId, peers.ownerId, peers.botId),
    }),
  });

  return answer.kept !== false;
}

// Whether a line has anything to hand over: words, or a picture.
function sayable(message: Api.Message): boolean {
  return (message.message ?? "") !== "" || (message.photo !== undefined && message.photo !== null);
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
  const { client, peers, store } = deps;
  const { chatId } = room;
  const aligned = isHistoryAligned(chatId);
  const progress = store.historyProgress(chatId);
  const oldestKnown = aligned ? room.oldestMessageId : null;
  const candidates = [progress?.reachedId, oldestKnown].filter((id): id is number => id !== undefined && id !== null && id !== 0);
  const offsetId = candidates.length === 0 ? 0 : Math.min(...candidates);
  const floorDate = aligned || room.oldestSentAt === null ? null : Math.floor(room.oldestSentAt / 1000);
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

    if (!sayable(message) || (floorDate !== null && message.date >= floorDate)) {
      continue;
    }

    const numbered = (ownId: number) => historyMessageId(chatId, ownId);

    handed += 1;
    kept += (await handLine(deps, chatId, message, numbered(message.id), numbered, names)) ? 1 : 0;

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

// The room the receiver names under `chatId`, or the refusal of a room it does not.
async function namedRoom(deps: HistoryDeps, chatId: number): Promise<HistoryRoom> {
  const named = await deps.service.rooms();
  const room = "rooms" in named ? historyRooms(named.rooms, deps.peers.ownerId).find((one) => one.chatId === chatId) : undefined;

  if (room === undefined) {
    throw new HistoryUnreadable(chatId);
  }

  return room;
}

/** One room's walk asked by hand: refused where the receiver does not name the room. */
export async function walkNamedRoom(deps: HistoryDeps, chatId: number, options: { maxMessages?: number } = {}): Promise<HistoryOutcome> {
  return await walkRoom(deps, await namedRoom(deps, chatId), options);
}

/** A room's recent past, every line said since `sinceMs` (epoch milliseconds), newest first, handed over as
 * history under the number the live reading gives it, so the receiver keeps only the lines it lacks. Only a room
 * the account numbers as the live reading does is filled: a supergroup or a channel, or a room whose people are
 * the reader's; any other is refused before its past is read, as is a room the receiver does not name. The
 * walk's progress is left as it stands. */
export async function fillNamedRoom(deps: HistoryDeps, chatId: number, sinceMs: number): Promise<HistoryOutcome> {
  await namedRoom(deps, chatId);

  if (!isHistoryAligned(chatId) && !(await deps.service.listened(chatId)).people) {
    throw new FillUnnumbered(chatId);
  }

  const entity = await resolveEntity(deps.client, peerOf(chatId, deps.peers));
  const names = new Map<number, string>();
  const since = Math.floor(sinceMs / 1000);
  const numbered = (ownId: number) => ownId;
  let handed = 0;
  let kept = 0;

  console.log(`history: room ${chatId} filling what was said since ${new Date(sinceMs).toISOString()}`);

  for await (const message of deps.client.iterMessages(entity, { limit: undefined })) {
    if (message.date < since) {
      break;
    }

    if (!(message instanceof mtproto().Api.Message) || !sayable(message)) {
      continue;
    }

    handed += 1;
    kept += (await handLine(deps, chatId, message, message.id, numbered, names)) ? 1 : 0;
  }

  console.log(`history: room ${chatId} filled, ${kept} of ${handed} kept`);

  return { handed, kept, reachedTop: false };
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
