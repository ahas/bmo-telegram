// What is sent in the owner's own voice, through the owner's account (docs/telegram.md, 「The owner's
// voice」): one part of words, and one album of files, each answering an earlier message where it names
// one, in the markup the service gives as HTML. The service divides a send into the parts and albums
// Telegram takes before it asks; this is what the account is told for each, and what each went out as.

import { accountMessageId, resolveEntity } from "./history";
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

/** What is sent in the owner's voice: one part of words, and one album of files. */
export interface OwnerAccount {
  say(words: OwnerWords): Promise<OwnerSent>;
  album(album: OwnerAlbum): Promise<OwnerSent[]>;
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

  const own = accountMessageId(chatId, replyToMessageId);

  if (own <= 0) {
    throw new ReplyUnreachable();
  }

  const [line] = await client.getMessages(entity, { ids: [own] });

  if (line === undefined || line.id !== own) {
    throw new ReplyUnreachable();
  }

  return { client, entity, replyTo: own };
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
  };

  return {
    say: async (words) => await sends.sending(words.chatId, () => account.say(words), (sent) => [sent.id]),
    album: async (album) => await sends.sending(album.chatId, () => account.album(album), (sent) => sent.map((one) => one.id)),
  };
}

/** The name a refusal of the owner's account crosses the seam under, or null where it is none. */
export function ownerRefusalName(error: unknown): "unavailable" | "unreachable" | null {
  if (error instanceof ReaderUnavailable) return "unavailable";
  if (error instanceof ReplyUnreachable) return "unreachable";

  return null;
}
