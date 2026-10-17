// What is sent in the owner's own voice, through the owner's account (docs/telegram.md, 「The owner's
// voice」): one piece of words, and one album of files, each answering an earlier message where it names
// one. The service divides a send into the pieces and albums Telegram takes before it asks; this is what
// the account is told for each.

import { historyMessageId, isHistoryAligned, resolveEntity } from "./history";
import { peerOf } from "./media";
import { mtproto, mtprotoUploads } from "./mtproto";

import type { TelegramClient } from "telegram";
import type { Peers } from "./media";

// The kinds a room receives a file as.
export type AttachmentKind = "photo" | "animation" | "video" | "audio" | "document";

// The image types Telegram shows as a photo. Everything else that is an image — a WebP, an HEIC, an SVG —
// goes as a file instead: Telegram would either recompress it into something else or refuse it, and the
// owner attached the file they meant.
const PHOTO_TYPES = new Set(["image/jpeg", "image/jpg", "image/png"]);
const PHOTO_EXTENSIONS: Record<string, string> = { "image/jpeg": ".jpg", "image/jpg": ".jpg", "image/png": ".png" };
const ANIMATION_TYPE = "image/gif";

// A file with no type of its own — some browsers send none for an unusual extension — is read off its name
// instead, which is the only other thing the upload carries.
const NAME_TYPES: Record<string, string> = {
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  png: "image/png",
  gif: "image/gif",
  webp: "image/webp",
  mp4: "video/mp4",
  mov: "video/quicktime",
  webm: "video/webm",
  mp3: "audio/mpeg",
  m4a: "audio/mp4",
  ogg: "audio/ogg",
  wav: "audio/wav",
};

function typeFromName(name: string): string {
  const extension = name.toLowerCase().split(".").pop() ?? "";

  return NAME_TYPES[extension] ?? "";
}

/** What Telegram makes of a file: the kind follows the file's own type rather than the room it goes to. */
export function attachmentKind(mime: string, name: string): AttachmentKind {
  const type = mime.trim().toLowerCase();
  const guessed = type === "" ? typeFromName(name) : type;

  if (guessed === ANIMATION_TYPE) {
    return "animation";
  }

  if (PHOTO_TYPES.has(guessed)) {
    return "photo";
  }

  if (guessed.startsWith("video/")) {
    return "video";
  }

  if (guessed.startsWith("audio/")) {
    return "audio";
  }

  return "document";
}

// The name the file travels under. A photo is only shown as one when its name says what it is, so a JPEG
// that arrived as "clipboard" is given the ending it already had by type.
export function attachmentName(kind: AttachmentKind, name: string, mime: string): string {
  const trimmed = name.trim() === "" ? "attachment" : name.trim();

  if (kind !== "photo") {
    return trimmed;
  }

  const wanted = PHOTO_EXTENSIONS[mime.trim().toLowerCase()] ?? ".jpg";

  return /\.(jpe?g|png)$/i.test(trimmed) ? trimmed : `${trimmed}${wanted}`;
}

// What the account is told about the file: whether to keep it whole as a document, whether the room may
// stream it, and the one attribute a moving picture needs to move.
export interface AttachmentSendOptions {
  forceDocument: boolean;
  supportsStreaming: boolean;
  animated: boolean;
}

export function attachmentSendOptions(kind: AttachmentKind): AttachmentSendOptions {
  return {
    forceDocument: kind === "document",
    supportsStreaming: kind === "video",
    animated: kind === "animation",
  };
}

// What the account is told about an album of several: kept whole where it is one of documents, and streamed
// where it carries a video. An animation never stands in one.
export function albumSendOptions(kinds: readonly AttachmentKind[]): AttachmentSendOptions {
  return {
    forceDocument: kinds.includes("document"),
    supportsStreaming: kinds.includes("video"),
    animated: false,
  };
}

// The message a reply answers, as the receiver holds it: its number, when it was said and what it says.
export interface ReplyTarget {
  messageId: number;
  sentAt: number;
  text: string;
}

// What of the account's history a reply needs: messages by number, or the newest ones before an instant.
export interface HistoryReader {
  byIds: (ids: number[]) => Promise<HistoryLine[]>;
  before: (date: number, limit: number) => Promise<HistoryLine[]>;
}

export interface HistoryLine {
  id: number;
  date: number;
  message: string;
}

// How far apart the two clocks may be and still name one message. Both are Telegram's own, so the slack is
// only for a second's rounding.
const DATE_SLACK_SECONDS = 2;

// How many lines around the instant are read looking for it. Messages said in the same few seconds are few;
// a room that says more than this in two seconds is one where the words decide.
const AROUND_LIMIT = 30;

// Where the offset of the account's numbering starts, as the history walk hands it over.
const FOREIGN_FLOOR = historyMessageId(-1, 0);

function near(left: number, right: number): boolean {
  return Math.abs(left - right) <= DATE_SLACK_SECONDS;
}

// The owner's account names a message by its own number, and in most rooms that is not the number the
// receiver holds it under. A supergroup numbers its messages once for everyone, so there the two agree. A
// basic group and a private chat number them per account: a line out of the account's own history is held
// under that number with an offset, and a line the bot received carries the bot's number, which the account
// has never seen. So a reply is named here by the account's number, found by the one thing both sides
// share: when the message was said, and what it says.
export async function accountReplyId(chatId: number, target: ReplyTarget, history: HistoryReader): Promise<number | null> {
  if (isHistoryAligned(chatId)) {
    return target.messageId > 0 ? target.messageId : null;
  }

  if (target.messageId < 0) {
    const own = target.messageId - FOREIGN_FLOOR;

    if (own <= 0) {
      return null;
    }

    const [line] = await history.byIds([own]);

    return line !== undefined && near(line.date, target.sentAt) ? own : null;
  }

  const around = (await history.before(target.sentAt + DATE_SLACK_SECONDS + 1, AROUND_LIMIT)).filter((line) => near(line.date, target.sentAt));

  if (around.length === 1) {
    return around[0]!.id;
  }

  // The receiver may hold a mark in front of the words — a photo's, a forward's — so the account's words are
  // looked for at the end of what the receiver holds.
  const said = around.filter((line) => line.message !== "" && target.text.endsWith(line.message));

  return said.length === 1 ? said[0]!.id : null;
}

// One file sent in the owner's voice.
export interface OwnerFile {
  name: string;
  mime: string;
  bytes: Uint8Array;
}

// One album — or one file alone — on its way into a room in the owner's voice.
export interface OwnerAlbum {
  chatId: number;
  files: OwnerFile[];
  caption: string;
  reply: ReplyTarget | null;
}

export class ReaderUnavailable extends Error {
  constructor() {
    super("the owner's account is not connected");
  }
}

export class ReplyUnreachable extends Error {
  constructor() {
    super("Telegram has no message here for the owner's account to reply to");
  }
}

/** What is sent in the owner's voice: one piece of words, and one album of files. */
export interface OwnerAccount {
  say(chatId: number, text: string, reply: ReplyTarget | null): Promise<number>;
  album(album: OwnerAlbum): Promise<(number | null)[]>;
}

function historyLines(found: ({ id: number; date: number; message?: string } | undefined)[]): HistoryLine[] {
  return found.flatMap((line) => (line === undefined ? [] : [{ id: line.id, date: line.date, message: line.message ?? "" }]));
}

// Where the account sends into a room: the account, the peer and the number of the message it answers.
// A reply that cannot be named to Telegram is refused rather than sent as an ordinary message: the owner
// asked for the reference, and a message without it would read as something else.
async function readerTarget(client: TelegramClient | null, peers: Peers, chatId: number, reply: ReplyTarget | null) {
  if (client === null) {
    throw new ReaderUnavailable();
  }

  const entity = await resolveEntity(client, peerOf(chatId, peers));

  if (reply === null) {
    return { client, entity, replyTo: undefined };
  }

  const replyTo = await accountReplyId(chatId, reply, {
    byIds: async (ids) => historyLines(await client.getMessages(entity, { ids })),
    before: async (date, limit) => historyLines(await client.getMessages(entity, { offsetDate: date, limit })),
  });

  if (replyTo === null) {
    throw new ReplyUnreachable();
  }

  return { client, entity, replyTo };
}

/** The owner's account as this process's own connection to it, asked for at each send. */
export function heldOwnerAccount(client: () => TelegramClient | null, peers: Peers): OwnerAccount {
  return {
    say: async (chatId, text, reply) => {
      const target = await readerTarget(client(), peers, chatId, reply);
      const sent = await target.client.sendMessage(target.entity, { message: text, replyTo: target.replyTo });

      return sent.id;
    },

    // An album of one is a file sent alone, which alone may be an animation.
    album: async (album) => {
      const { client: account, entity, replyTo } = await readerTarget(client(), peers, album.chatId, album.reply);
      const kinds = album.files.map((file) => attachmentKind(file.mime, file.name));
      const uploads = album.files.map(
        (file, at) => new (mtprotoUploads().CustomFile)(attachmentName(kinds[at]!, file.name, file.mime), file.bytes.length, "", Buffer.from(file.bytes)),
      );
      const caption = album.caption === "" ? undefined : album.caption;

      if (uploads.length === 1) {
        const options = attachmentSendOptions(kinds[0]!);
        const sent = await account.sendFile(entity, {
          file: uploads[0]!,
          caption,
          replyTo,
          forceDocument: options.forceDocument,
          supportsStreaming: options.supportsStreaming,
          attributes: options.animated ? [new (mtproto().Api.DocumentAttributeAnimated)()] : undefined,
          workers: 1,
        });

        return [sent.id];
      }

      const options = albumSendOptions(kinds);
      // Given several files the client sends one album and answers each of its messages, where the
      // platform's answer named it.
      const sent = (await account.sendFile(entity, {
        file: uploads,
        caption,
        replyTo,
        forceDocument: options.forceDocument,
        supportsStreaming: options.supportsStreaming,
      })) as unknown as ({ id: number } | undefined)[];

      return album.files.map((_, at) => sent[at]?.id ?? null);
    },
  };
}

/** The name a refusal of the owner's account crosses the seam under, or null where it is none. */
export function ownerRefusalName(error: unknown): "unavailable" | "unreachable" | null {
  if (error instanceof ReaderUnavailable) return "unavailable";
  if (error instanceof ReplyUnreachable) return "unreachable";

  return null;
}
