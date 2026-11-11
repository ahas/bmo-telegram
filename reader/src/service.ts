// The service as the reader reaches it over the seam (docs/boundary-api.md, 「The seam between the service
// and the reader」): what the owner's account read handed on to the receiver, and what the receiver says of
// its rooms, asked through the service, which is the only process that talks to another unit.

import type { ReactionsSeen, RoomDeletion } from "./aftermath";

// A message as it crosses the chat protocol (bmo-chat's `docs/contract.md`, 「Messages」). Its time is an
// integer of epoch milliseconds.
export interface HandedMessage {
  origin: "live" | "edit" | "history";
  senderId: number | null;
  senderName: string;
  sentAt: number;
  text: string;
  replyToMessageId: number | null;
  via: string;
  album: number | null;
  mode: "human-reader";
  context: HandedContext;
  room: RoomFacts;
  files: MessageFile[];
  gathering: boolean;
  gatherFailure?: string | null;
}

export interface HandedContext {
  roomTitle?: string;
  senderUsername?: string;
  replyToSenderId?: number;
  spokenText?: string;
  metadata?: unknown;
  carriesFile?: boolean;
  tag?: string;
}

/** What the service says of reading a room: whether the receiver listens to it, and whether a person's words in
 * it are the reader's to hand over — a room the bot is not in that the receiver asked to be read. */
export interface RoomReading {
  listened: boolean;
  people: boolean;
}

export interface RoomFacts {
  kind: "owner" | "group";
  identity: number | null;
  privateTo: number | null;
}

// A file as it crosses: its bytes in base64.
export interface FileItem {
  name: string;
  mime: string;
  bytes: string;
}

// A file a message carried, as it crosses: a file item and the id the same file crosses under each time.
export interface MessageFile extends FileItem {
  id: string;
  kind?: "photo" | "video" | "round-video" | "animation" | "audio" | "voice-note" | "sticker" | "document";
}

// The files a message's gathering promised, or why they could not be kept.
export interface GatheredPatch {
  files: MessageFile[];
  gatherFailure?: string;
}

export type Inbound =
  | { kind: "message"; roomId: number; messageId: number; message: HandedMessage }
  | { kind: "gathered"; roomId: number; messageId: number; patch: GatheredPatch };

// A room whose history the reader may walk, with the oldest message the receiver holds of it.
export interface HistoryRoom {
  chatId: number;
  oldestMessageId: number | null;
  oldestSentAt: number | null;
}

// What the service answers about the rooms whose history may be walked.
export type Rooms = { rooms: HistoryRoom[] } | { unanswered: string };

/** The service's routes the reader calls. */
export interface Service {
  // Whether the service answers at all, which the reader waits for before it opens the account.
  reachable(): Promise<boolean>;
  // A hand-over carried on to the receiver; a history line answers whether the receiver kept it.
  inbound(inbound: Inbound): Promise<{ kept?: boolean }>;
  listened(chatId: number): Promise<RoomReading>;
  rooms(): Promise<Rooms>;
  seen(): Promise<{ rooms: number[]; people: number[] }>;
  avatarsChanged(rooms: number[], people: number[]): Promise<void>;
  // A message's reactions as the account sees them, carried on as a reaction change.
  reactions(seen: ReactionsSeen): Promise<void>;
  // Messages of one room deleted, carried on as a deletion.
  deleted(deletion: RoomDeletion): Promise<void>;
}

/** The provider's key, written onto every message it hands over. */
export const via = "telegram";

/** The service on the loopback port, behind the seam's token. */
export function serviceAt(port: number, token: string): Service {
  const base = `http://127.0.0.1:${port}`;
  const headers = { authorization: `Bearer ${token}`, "content-type": "application/json" };

  // A call answered, however long the service takes, with no idle timer of Bun's to cut it; a failure is
  // thrown, but for a `not-ready` where the caller reads one as an answer.
  const call = async (method: "GET" | "POST", path: string, body?: unknown, notReadyAnswers = false): Promise<Response> => {
    const response = await fetch(`${base}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), timeout: false });

    if (!response.ok && !(notReadyAnswers && response.status === 503)) {
      throw new Error(`the service answered ${path} with ${response.status}: ${await response.text()}`);
    }

    return response;
  };

  return {
    reachable: async () => {
      try {
        return (await fetch(`${base}/reader/seen`, { headers, timeout: false })).ok;
      } catch {
        return false;
      }
    },
    inbound: async (inbound) => await (await call("POST", "/reader/inbound", inbound)).json() as { kept?: boolean },
    listened: async (chatId) => {
      const answer = await (await call("POST", "/reader/listened", { chatId })).json() as { listened?: boolean; people?: boolean };

      return { listened: answer.listened !== false, people: answer.people === true };
    },
    rooms: async () => {
      const response = await call("GET", "/reader/rooms", undefined, true);
      const answer = await response.json() as { rooms?: HistoryRoom[]; error?: string };

      return response.status === 503 || answer.rooms === undefined
        ? { unanswered: answer.error ?? "the service did not say which rooms" }
        : { rooms: answer.rooms };
    },
    seen: async () => await (await call("GET", "/reader/seen")).json() as { rooms: number[]; people: number[] },
    avatarsChanged: async (rooms, people) => {
      await call("POST", "/reader/avatars", { rooms, people });
    },
    reactions: async (seen) => {
      await call("POST", "/reader/reactions", seen);
    },
    deleted: async (deletion) => {
      await call("POST", "/reader/deleted", deletion);
    },
  };
}

/** What the provider knows of a room it hands a message of over. */
export function roomFacts(chatId: number, ownerId: number, botId: number | null): RoomFacts {
  return {
    kind: chatId === ownerId ? "owner" : "group",
    identity: botId,
    privateTo: chatId > 0 ? chatId : null,
  };
}

/** Bytes as they cross the wire. */
export function base64Of(bytes: Uint8Array): string {
  return Buffer.from(bytes).toString("base64");
}

/** The message a hand-over carries, with what the reader knows of it. `sentAt` is Telegram's own second; the
 * message crosses in epoch milliseconds. */
export function handedMessage(input: {
  chatId: number;
  messageId: number;
  origin: "live" | "edit" | "history";
  senderId: number | null;
  senderName: string;
  sentAt: number;
  text: string;
  replyToMessageId: number | null;
  context?: HandedContext;
  room: RoomFacts;
  files?: MessageFile[];
  gathering?: boolean;
}): HandedMessage {
  return {
    origin: input.origin,
    senderId: input.senderId,
    senderName: input.senderName,
    sentAt: input.sentAt * 1000,
    text: input.text,
    replyToMessageId: input.replyToMessageId,
    via,
    album: null,
    mode: "human-reader",
    context: input.context ?? {},
    room: input.room,
    files: input.files ?? [],
    gathering: input.gathering ?? false,
  };
}
