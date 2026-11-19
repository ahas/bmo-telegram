// What is sent through the owner's account, in the owner's own voice or the avatar's (docs/telegram.md, 「The
// owner's voice」): one part of words, and one album of files, each answering an earlier message where it
// names one, in the markup the service gives as HTML; the typing mark; a message deleted; and the owner's own
// reaction on a message, a message pinned and his own line edited; and the account's own number for a line the
// service names by its stamp. The service divides a send into the parts and
// albums Telegram takes, and puts the avatar's prefix before its words, before it asks; this is what the account
// is told for each, and what each went out as.

import { createHash } from "node:crypto";

import { accountMessageId } from "./numbering";
import { resolveEntity } from "./history";
import { peerOf } from "./media";
import { mtproto, mtprotoUploads } from "./mtproto";

import type { TelegramClient } from "telegram";
import type { Peers } from "./media";
import type { OwnSends } from "./sends";

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

// One file sent in the owner's voice.
export interface OwnerFile {
  name: string;
  mime: string;
  bytes: Uint8Array;
}

// One part of words on its way into a room in the owner's voice: the plain words, and the dialect's HTML
// where they carry marks.
export interface OwnerWords {
  chatId: number;
  text: string;
  markup: string | null;
  silent: boolean;
  replyToMessageId: number | null;
}

// One album — or one file alone — on its way into a room in the owner's voice, its caption plain and in the
// dialect's HTML where it carries marks.
export interface OwnerAlbum {
  chatId: number;
  files: OwnerFile[];
  caption: string;
  captionMarkup: string | null;
  replyToMessageId: number | null;
}

// One message a send went out as: the number the account gave it, where Telegram named it, and the words
// Telegram holds for it.
export interface OwnerSent {
  id: number | null;
  text: string;
}

// A line of the bot account's as the service names it: who sent it, when to the second in epoch milliseconds, and
// the SHA-256 of its words.
export interface LineStamp {
  chatId: number;
  senderId: number;
  sentAt: number;
  words: string;
}

/** The SHA-256 of a line's words, in lowercase hex, as the service reckons it too. */
export function wordsHash(words: string): string {
  return createHash("sha256").update(words, "utf8").digest("hex");
}

// The owner's own line given new words, the plain words and the dialect's HTML where they carry marks.
export interface OwnerEdit {
  chatId: number;
  messageId: number;
  text: string;
  markup: string | null;
}

// The line as an edit left it: the account's own number for it, the words Telegram now holds for it, and
// whether the edit changed them.
export interface OwnerEdited {
  id: number;
  text: string;
  edited: boolean;
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

// What the owner's account was asked to do to a message, as its refusal names it.
export type MessageAct = "delete" | "react to" | "pin" | "unpin" | "edit";

export class MessageGone extends Error {
  constructor(act: MessageAct) {
    super(`Telegram has no message here for the owner's account to ${act}`);
  }
}

// A line the service names by its stamp that the account holds not exactly once.
export class LineUnmatched extends Error {
  constructor(count: number) {
    super(`the owner's account holds ${count} lines from that sender at that second with those words`);
  }
}

export class NotOwnLine extends Error {
  constructor() {
    super("the message is not the owner's account's own, so it cannot edit it");
  }
}

// An act Telegram turned down for what it asked, in Telegram's own words.
export class TurnedDown extends Error {}

/** A refusal Telegram gives for what an act asked, its 400s, as the act turned down in Telegram's own words;
 * anything else, its flood limit and its own fault among them, as it came. */
export function turnedDown(error: unknown): unknown {
  const { code, errorMessage } = error as { code?: unknown; errorMessage?: unknown };

  return code === 400 && typeof errorMessage === "string" ? new TurnedDown(errorMessage) : error;
}

/** Whether Telegram answered that the act would change nothing: a reaction, a pin or words the message has
 * already. */
export function isUnchanged(error: unknown): boolean {
  return (error as { errorMessage?: unknown }).errorMessage === "MESSAGE_NOT_MODIFIED";
}

/** The number of the service message Telegram wrote for a pin, out of its answer to the pin; null where it wrote
 * none, as for an unpin or a message pinned already. */
export function pinServiceMessageId(answer: unknown): number | null {
  const { updates, update } = answer as { updates?: unknown; update?: unknown };
  const listed: unknown[] = Array.isArray(updates) ? updates : update === undefined ? [] : [update];

  for (const one of listed) {
    const message = (one as { message?: { className?: unknown; id?: unknown; action?: { className?: unknown } } }).message;

    if (message?.className === "MessageService" && message.action?.className === "MessageActionPinMessage" && typeof message.id === "number") {
      return message.id;
    }
  }

  return null;
}

/** What is sent through the owner's account: one part of words, one album of files, the typing mark shown or
 * taken off, a message deleted for everyone, the account's own reaction put on a message or taken off, a message
 * pinned for everyone or unpinned, answering the service message Telegram wrote for a pin, the account's own
 * line edited, and the account's own number for the one line it holds with a stamp. */
export interface OwnerAccount {
  say(words: OwnerWords): Promise<OwnerSent>;
  album(album: OwnerAlbum): Promise<OwnerSent[]>;
  typing(chatId: number, on: boolean): Promise<void>;
  delete(chatId: number, messageId: number): Promise<void>;
  react(chatId: number, messageId: number, emoji: string): Promise<void>;
  pin(chatId: number, messageId: number, on: boolean, silent: boolean): Promise<number | null>;
  edit(edit: OwnerEdit): Promise<OwnerEdited>;
  line(stamp: LineStamp): Promise<number>;
}

// How the account reads what it is given: the service's markup as HTML, plain words as they stand. The
// client's own default would read them as markdown.
function parseModeOf(markup: string | null): "html" | false {
  return markup === null ? false : "html";
}

// Where the account sends into a room: the account, the peer and the number of the message it answers. The
// number the provider holds is turned into the account's own number where the room numbers messages per
// account, and the line is read back from the account to make sure it is there. A reply that cannot be named
// to Telegram is refused rather than sent as an ordinary message: the owner asked for the reference, and a
// message without it would read as something else.
async function readerTarget(
  client: TelegramClient | null,
  peers: Peers,
  chatId: number,
  replyToMessageId: number | null,
) {
  if (client === null) {
    throw new ReaderUnavailable();
  }

  const entity = await resolveEntity(client, peerOf(chatId, peers));

  if (replyToMessageId === null) {
    return { client, entity, replyTo: undefined };
  }

  const own = await heldLine(client, entity, chatId, replyToMessageId);

  if (own === null) {
    throw new ReplyUnreachable();
  }

  return { client, entity, replyTo: own };
}

// The line the account holds under the number the provider names; null where it holds none.
async function heldMessage(client: TelegramClient, entity: Awaited<ReturnType<typeof resolveEntity>>, chatId: number, messageId: number) {
  const own = accountMessageId(chatId, messageId);

  if (own <= 0) return null;

  const [line] = await client.getMessages(entity, { ids: [own] });

  return line === undefined || line.id !== own ? null : line;
}

// The account's own number for a message the provider names, where the account holds a line under it; null
// where it holds none.
async function heldLine(client: TelegramClient, entity: Awaited<ReturnType<typeof resolveEntity>>, chatId: number, messageId: number): Promise<number | null> {
  return (await heldMessage(client, entity, chatId, messageId))?.id ?? null;
}

/** The owner's account as this process's own connection to it, asked for at each send; every message a send
 * goes out as is noted in `sends`, so the live reading does not hand it over again. */
export function heldOwnerAccount(client: () => TelegramClient | null, peers: Peers, sends: OwnSends): OwnerAccount {
  const account: OwnerAccount = {
    // The markup where there is one, and the plain words where the account refuses it.
    say: async (words) => {
      const target = await readerTarget(client(), peers, words.chatId, words.replyToMessageId);
      const send = (message: string, markup: string | null) =>
        target.client.sendMessage(target.entity, { message, replyTo: target.replyTo, parseMode: parseModeOf(markup), silent: words.silent });
      let sent;

      try {
        sent = await send(words.markup ?? words.text, words.markup);
      } catch (error) {
        if (words.markup === null) throw error;

        sent = await send(words.text, null);
      }

      return { id: sent.id, text: sent.message ?? "" };
    },

    // An album of one is a file sent alone, which alone may be an animation.
    album: async (album) => {
      const { client: account, entity, replyTo } = await readerTarget(client(), peers, album.chatId, album.replyToMessageId);
      const kinds = album.files.map((file) => attachmentKind(file.mime, file.name));
      const uploads = album.files.map(
        (file, at) => new (mtprotoUploads().CustomFile)(attachmentName(kinds[at]!, file.name, file.mime), file.bytes.length, "", Buffer.from(file.bytes)),
      );
      const caption = album.caption === "" ? undefined : (album.captionMarkup ?? album.caption);
      const parseMode = parseModeOf(album.captionMarkup);

      if (uploads.length === 1) {
        const options = attachmentSendOptions(kinds[0]!);
        const sent = await account.sendFile(entity, {
          file: uploads[0]!,
          caption,
          parseMode,
          replyTo,
          forceDocument: options.forceDocument,
          supportsStreaming: options.supportsStreaming,
          attributes: options.animated ? [new (mtproto().Api.DocumentAttributeAnimated)()] : undefined,
          workers: 1,
        });

        return [{ id: sent.id, text: sent.message ?? "" }];
      }

      const options = albumSendOptions(kinds);
      // Given several files the client sends one album and answers each of its messages, where the
      // platform's answer named it.
      const sent = (await account.sendFile(entity, {
        file: uploads,
        caption,
        parseMode,
        replyTo,
        forceDocument: options.forceDocument,
        supportsStreaming: options.supportsStreaming,
      })) as unknown as ({ id: number; message?: string } | undefined)[];

      return album.files.map((_, at) => ({ id: sent[at]?.id ?? null, text: sent[at]?.message ?? "" }));
    },

    typing: async (chatId, on) => {
      const { client: account, entity } = await readerTarget(client(), peers, chatId, null);
      const { Api } = mtproto();
      const action = on ? new Api.SendMessageTypingAction() : new Api.SendMessageCancelAction();

      await account.invoke(new Api.messages.SetTyping({ peer: entity, action }));
    },

    // Deleted for everyone in the room, as a person deleting their own message would ask.
    delete: async (chatId, messageId) => {
      const { client: account, entity } = await readerTarget(client(), peers, chatId, null);
      const own = await heldLine(account, entity, chatId, messageId);

      if (own === null) throw new MessageGone("delete");

      await account.deleteMessages(entity, [own], { revoke: true });
    },

    // The account's own reaction, as a person reacting would put it on: the emoji alone, or none to take it off.
    // A reaction the message already carries from the account is Telegram's MESSAGE_NOT_MODIFIED, and no failure.
    react: async (chatId, messageId, emoji) => {
      const { client: account, entity } = await readerTarget(client(), peers, chatId, null);
      const own = await heldLine(account, entity, chatId, messageId);

      if (own === null) throw new MessageGone("react to");

      const { Api } = mtproto();
      const reaction = emoji === "" ? undefined : [new Api.ReactionEmoji({ emoticon: emoji })];

      try {
        await account.invoke(new Api.messages.SendReaction({ peer: entity, msgId: own, reaction }));
      } catch (error) {
        if (!isUnchanged(error)) throw error;
      }
    },

    // Pinned for everyone in the room, never for the account alone, as a person pinning in the room would ask.
    // The request is made by hand: the client's own pin reads a message numbered 0 as every pin taken off.
    pin: async (chatId, messageId, on, silent) => {
      const { client: account, entity } = await readerTarget(client(), peers, chatId, null);
      const own = await heldLine(account, entity, chatId, messageId);

      if (own === null) throw new MessageGone(on ? "pin" : "unpin");

      const { Api } = mtproto();

      try {
        return pinServiceMessageId(await account.invoke(new Api.messages.UpdatePinnedMessage({ peer: entity, id: own, silent, unpin: !on, pmOneside: false })));
      } catch (error) {
        if (isUnchanged(error)) return null;

        throw turnedDown(error);
      }
    },

    // The markup where there is one, and the plain words where the account refuses it; only the account's own
    // line takes new words. Words the line already says are Telegram's MESSAGE_NOT_MODIFIED, and no failure.
    edit: async (edit) => {
      const { client: account, entity } = await readerTarget(client(), peers, edit.chatId, null);
      const line = await heldMessage(account, entity, edit.chatId, edit.messageId);

      if (line === null) throw new MessageGone("edit");
      if (line.out !== true) throw new NotOwnLine();

      const unchanged = { id: line.id, text: line.message ?? "", edited: false };
      const change = async (text: string, markup: string | null) => {
        const edited = await account.editMessage(entity, { message: line.id, text, parseMode: parseModeOf(markup) });

        return { id: line.id, text: edited.message ?? "", edited: true };
      };

      try {
        return await change(edit.markup ?? edit.text, edit.markup);
      } catch (error) {
        if (isUnchanged(error)) return unchanged;
        if (edit.markup === null) throw turnedDown(error);
      }

      try {
        return await change(edit.text, null);
      } catch (error) {
        if (isUnchanged(error)) return unchanged;

        throw turnedDown(error);
      }
    },

    // The room's lines of that second, newest first, the one from that sender with those words; the account's own
    // lines are the owner's, under the number the bot account knows him by.
    line: async (stamp) => {
      const { client: account, entity } = await readerTarget(client(), peers, stamp.chatId, null);
      const second = Math.floor(stamp.sentAt / 1000);
      const lines = await account.getMessages(entity, { offsetDate: second + 1, limit: 100 });
      const matching = lines.filter(
        (line) => line.date === second && (line.out === true ? peers.ownerId : Number(line.senderId?.toString())) === stamp.senderId && wordsHash(line.message ?? "") === stamp.words,
      );

      if (matching.length !== 1) throw new LineUnmatched(matching.length);

      return matching[0]!.id;
    },
  };

  return {
    say: async (words) => await sends.sending(words.chatId, () => account.say(words), (sent) => [sent.id]),
    album: async (album) => await sends.sending(album.chatId, () => account.album(album), (sent) => sent.map((one) => one.id)),
    typing: account.typing,
    delete: account.delete,
    react: account.react,
    pin: account.pin,
    edit: account.edit,
    line: account.line,
  };
}

/** The name a refusal of the owner's account crosses the seam under, or null where it is none. */
export function ownerRefusalName(error: unknown): "unavailable" | "unreachable" | "gone" | "not-own" | "turned-down" | null {
  if (error instanceof ReaderUnavailable) return "unavailable";
  if (error instanceof ReplyUnreachable) return "unreachable";
  if (error instanceof MessageGone || error instanceof LineUnmatched) return "gone";
  if (error instanceof NotOwnLine) return "not-own";
  if (error instanceof TurnedDown) return "turned-down";

  return null;
}
