// What the owner's account fetches of a message: a picture or a file the Bot API will not hand the bot,
// found by the message's number and, where the account numbers the room its own way, by the second it
// was sent in (the service asks with epoch milliseconds, Telegram keeps seconds); and the name a file with
// none of its own is given.

import { accountMessageId } from "./numbering";
import { mtproto } from "./mtproto";
import { POLICY } from "./policy";

import type { Api, TelegramClient } from "telegram";
import type { MessageFile } from "./service";

// Who is asked about a room: the account sees the owner's one-to-one with the bot from its own side,
// as the chat with the bot; every other room is its own peer.
export interface Peers {
  ownerId: number;
  botId: number | null;
}

export function peerOf(chatId: number, peers: Peers): number {
  return chatId === peers.ownerId && peers.botId !== null ? peers.botId : chatId;
}

const imageFileExtensions = new Set(["jpg", "jpeg", "png", "webp", "gif"]);

// An image sent as an uncompressed file arrives as a document, not a photo. The type decides; the
// common extensions are the fallback for senders that supply none or only a generic one.
export function isImageFile(mimeType: string | undefined, fileName: string | undefined): boolean {
  const mime = mimeType?.toLowerCase();

  if (mime !== undefined && mime !== "" && mime !== "application/octet-stream") {
    return mime.startsWith("image/");
  }

  const dot = fileName?.lastIndexOf(".") ?? -1;

  return fileName !== undefined && dot > 0 && imageFileExtensions.has(fileName.slice(dot + 1).toLowerCase());
}

/** A length of time as a placeholder shows it, after a space: minutes and two-digit seconds. */
export function seconds(total: number | undefined): string {
  if (total === undefined) {
    return "";
  }

  return ` ${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

function documentFileName(document: Api.Document): string | undefined {
  for (const attribute of document.attributes) {
    if (attribute instanceof mtproto().Api.DocumentAttributeFilename) {
      return attribute.fileName;
    }
  }

  return undefined;
}

/** The id the file a message carries crosses with: Telegram's own number for its photo or its document. */
export function fileIdOf(message: Api.Message): string {
  const photo = message.photo;

  if (photo !== undefined && photo !== null) return `photo-${String(photo.id)}`;

  const document = message.document;

  return document !== undefined && document !== null ? `document-${String(document.id)}` : "file";
}

// A photo, or an image sent as a file: either counts as the picture a message carries.
export function messageHasPhoto(message: Api.Message): boolean {
  if (message.photo !== undefined && message.photo !== null) {
    return true;
  }

  const document = message.document;

  return document !== undefined && document !== null && isImageFile(document.mimeType, documentFileName(document));
}

function messageChatId(message: Api.Message): number | null {
  const id = message.chatId;

  return id === undefined || id === null ? null : Number(id.toString());
}

// The message the account holds for one named by the number the receiving side holds it under (「Numbers」 in
// docs/reader.md says when that is turned back first), found by its number and, where the numbers are not
// shared, by the second it was sent in. `carries` says whether a message is the one wanted: the message
// under the number carrying nothing of the kind is "none", and a message not found at all is null.
export async function readerMessageFor(
  client: TelegramClient,
  peers: Peers,
  chatId: number,
  messageId: number,
  sentAt: number | null,
  carries: (message: Api.Message) => boolean,
): Promise<Api.Message | "none" | null> {
  const entity = peerOf(chatId, peers);
  const [byId] = await client.getMessages(entity, { ids: [accountMessageId(chatId, messageId)] });

  if (byId instanceof mtproto().Api.Message && messageChatId(byId) === entity && (sentAt === null || byId.date === sentAt)) {
    return carries(byId) ? byId : "none";
  }

  if (sentAt === null) {
    return null;
  }

  const around = await client.getMessages(entity, { offsetDate: sentAt + 2, limit: 20 });

  return (
    around.find(
      (candidate): candidate is Api.Message => candidate instanceof mtproto().Api.Message && candidate.date === sentAt && carries(candidate),
    ) ?? null
  );
}

// What a fetch through the account came to: the file, a reason it could not be had, or no account to ask.
export type Fetched =
  | ({ outcome: "fetched" } & MessageFile)
  | { outcome: "unavailable"; reason: string }
  | { outcome: "no-reader"; reason: string };

export const noReader = "the owner's account is not connected";

async function downloaded(client: TelegramClient, message: Api.Message): Promise<Uint8Array> {
  const data = await client.downloadMedia(message);

  if (!(data instanceof Uint8Array) || data.byteLength === 0) {
    throw new Error("downloadMedia returned no data");
  }

  return data;
}

/** The picture a message the account read carries, or null where its bytes could not be had. */
export async function pictureOf(client: TelegramClient, message: Api.Message): Promise<MessageFile | null> {
  try {
    return { id: fileIdOf(message), name: `${message.id}.jpg`, mime: "image/jpeg", bytes: Buffer.from(await downloaded(client, message)).toString("base64"), kind: "photo" };
  } catch (error) {
    console.error(`reader photo download for message ${message.id} failed:`, error);

    return null;
  }
}

/** One message's picture fetched through the account. */
export async function fetchPicture(
  client: TelegramClient | null,
  peers: Peers,
  asked: { chatId: number; messageId: number; sentAt: number | null },
): Promise<Fetched> {
  if (client === null) {
    return { outcome: "no-reader", reason: noReader };
  }

  try {
    const sentAt = asked.sentAt === null ? null : Math.floor(asked.sentAt / 1000);
    const message = await readerMessageFor(client, peers, asked.chatId, asked.messageId, sentAt, messageHasPhoto);

    if (message === "none") return { outcome: "unavailable", reason: "the message carries no picture" };
    if (message === null) return { outcome: "unavailable", reason: "the owner's account found no such message" };

    const bytes = await downloaded(client, message);

    return { outcome: "fetched", id: fileIdOf(message), name: `${message.id}.jpg`, mime: "image/jpeg", bytes: Buffer.from(bytes).toString("base64"), kind: "photo" };
  } catch (error) {
    return { outcome: "unavailable", reason: error instanceof Error ? error.message : String(error) };
  }
}

/** One message's file fetched through the account: a file past the Bot API's ceiling for a bot. */
export async function fetchFile(
  client: TelegramClient | null,
  peers: Peers,
  asked: { chatId: number; messageId: number; sentAt: number },
): Promise<Fetched> {
  if (client === null) {
    return { outcome: "no-reader", reason: noReader };
  }

  try {
    const sentAt = Math.floor(asked.sentAt / 1000);
    const message = await readerMessageFor(client, peers, asked.chatId, asked.messageId, sentAt, (one) => one.document !== undefined && one.document !== null);

    if (message === null || message === "none") {
      return { outcome: "unavailable", reason: "the owner's account found no file under this message" };
    }

    const document = message.document!;
    const bytes = await downloaded(client, message);

    return {
      outcome: "fetched",
      id: fileIdOf(message),
      name: readerFileOf(document).name,
      mime: document.mimeType,
      bytes: Buffer.from(bytes).toString("base64"),
      kind: messageFileKind(document),
    };
  } catch (error) {
    return { outcome: "unavailable", reason: error instanceof Error ? error.message : String(error) };
  }
}

const gathering = POLICY.gathering;

// A sound with no name of its own is named for which it was: a note somebody spoke, or a sound file
// they attached, with the extension its type implies.
export function mediaFileName(mimeType: string | undefined, voice: boolean): string {
  const mime = mimeType?.split(";")[0]?.trim().toLowerCase() ?? "";

  return `${voice ? gathering.voice_name : gathering.sound_name}${gathering.audio_extensions[mime] ?? ""}`;
}

export type VideoKind = "video" | "video-note" | "gif";

// A video that arrived with no file name of its own is named for which of the three it was — a video, a
// round video note, or a GIF, which Telegram keeps as a silent MP4 — with the extension its type implies.
export function videoFileName(mimeType: string | undefined, what: VideoKind): string {
  const mime = mimeType?.split(";")[0]?.trim().toLowerCase() ?? "";
  const names = { video: gathering.video_name, "video-note": gathering.video_note_name, gif: gathering.gif_name };

  return `${names[what]}${gathering.video_extensions[mime] ?? gathering.video_fallback_extension}`;
}

// What a document the account read is: a sticker, a video of one of three kinds, a sound, or a file, and
// the name it is handed over under.
export interface ReaderFile {
  name: string;
  sticker: boolean;
  video: VideoKind | null;
  audio: boolean;
  voice: boolean;
  svg: boolean;
  image: boolean;
}

// What the file is as the protocol names it, as far as the account can tell.
export function messageFileKind(document: Api.Document): MessageFile["kind"] {
  const file = readerFileOf(document);

  if (file.sticker) return "sticker";
  if (file.video === "video-note") return "round-video";
  if (file.video === "gif") return "animation";
  if (file.video === "video") return "video";
  if (file.audio) return file.voice ? "voice-note" : "audio";
  if (file.image) return "photo";

  return "document";
}

export function readerFileOf(document: Api.Document): ReaderFile {
  const { Api } = mtproto();
  let documentName: string | undefined;
  let voice = false;
  let audio = false;
  let sticker = false;
  let animated = false;
  let video: VideoKind | null = null;

  for (const attribute of document.attributes) {
    if (attribute instanceof Api.DocumentAttributeFilename) documentName = attribute.fileName;
    if (attribute instanceof Api.DocumentAttributeAudio) {
      voice = attribute.voice === true;
      audio = true;
    }
    if (attribute instanceof Api.DocumentAttributeSticker) sticker = true;
    if (attribute instanceof Api.DocumentAttributeAnimated) animated = true;
    if (attribute instanceof Api.DocumentAttributeVideo) video = attribute.roundMessage === true ? "video-note" : "video";
  }

  if (video === "video" && animated) video = "gif";

  const mime = document.mimeType.toLowerCase();
  const svg = mime === "image/svg+xml" || documentName?.toLowerCase().endsWith(".svg") === true;
  const stickerExtension = mime === "video/webm" ? "webm" : mime === "application/x-tgsticker" ? "tgs" : "webp";
  const name = sticker
    ? `sticker-${String(document.id)}.${stickerExtension}`
    : documentName ?? (video === null ? mediaFileName(document.mimeType, voice) : videoFileName(document.mimeType, video));

  return { name, sticker, video, audio, voice, svg, image: !sticker && !svg && mime.startsWith("image/") };
}
