// One chat's recent messages (docs/reader.md, 「One chat's recent messages」): a dialog read on the live
// account by its number, newest first, without a room standing for it, each line with its sender, moment,
// words, and the placeholder a picture, file, sound or other non-text stands under. The account's rooms, the
// history walk and the record of what was seen are untouched.

import { resolveEntity, senderIdOf, senderNameOf } from "./history";
import { peerOf, seconds } from "./media";
import { mtproto } from "./mtproto";
import { fill, POLICY } from "./policy";

import type { Api, TelegramClient } from "telegram";
import type { Peers } from "./media";

/** How many messages a read answers where it asks for no number, and the most it answers at all. */
export const DEFAULT_MAX = 50;
export const LARGEST_MAX = 200;

/** A chat read asked for: its number, how many messages at most, and the earliest moment it asks for. */
export interface MessagesAsked {
  chatId: number;
  max: number;
  since: number | null;
}

/** One line of a chat read: who said it, when, and what the line says. */
export interface RecentMessage {
  messageId: number;
  senderId: number | null;
  senderName: string;
  sentAt: number;
  text: string;
}

/** A chat number the account cannot resolve. It is refused rather than read, since a history asked of no
 * peer is a search over every chat the account is in. */
export class ChatUnresolved extends Error {
  constructor(chatId: number, cause: unknown) {
    super(`the owner's account cannot resolve chat ${chatId}: ${cause instanceof Error ? cause.message : String(cause)}`);
  }
}

/** What a message that is not words alone says it is, in the placeholders the bot account's reading uses. */
export function placeholderOf(message: Api.Message): string {
  const { Api } = mtproto();
  const text = message.message ?? "";
  const caption = text === "" ? "" : ` ${text}`;

  if (message.photo !== undefined && message.photo !== null) return fill(POLICY.kinds.photo, { caption });

  const document = message.document;

  if (document === undefined || document === null) return fill(POLICY.kinds.document, { name: "", caption });

  const attributes = document.attributes ?? [];
  const sticker = attributes.find((one): one is Api.DocumentAttributeSticker => one instanceof Api.DocumentAttributeSticker);

  if (sticker !== undefined) return fill(POLICY.kinds.sticker, { emoji: ` ${sticker.alt}`, set: "" });

  const video = attributes.find((one): one is Api.DocumentAttributeVideo => one instanceof Api.DocumentAttributeVideo);

  if (video !== undefined) {
    const duration = seconds(Math.round(video.duration));

    if (video.roundMessage === true) return fill(POLICY.kinds.video_note, { duration });
    if (attributes.some((one) => one instanceof Api.DocumentAttributeAnimated)) return fill(POLICY.kinds.gif, { duration });

    return fill(POLICY.kinds.video, { duration, caption });
  }

  const audio = attributes.find((one): one is Api.DocumentAttributeAudio => one instanceof Api.DocumentAttributeAudio);

  if (audio !== undefined) {
    const duration = seconds(Math.round(audio.duration));

    if (audio.voice === true) return fill(POLICY.kinds.voice, { duration, caption });

    const named = attributes.find((one): one is Api.DocumentAttributeFilename => one instanceof Api.DocumentAttributeFilename);

    return fill(POLICY.kinds.audio, { name: named === undefined ? "" : ` ${named.fileName}`, duration, caption });
  }

  const named = attributes.find((one): one is Api.DocumentAttributeFilename => one instanceof Api.DocumentAttributeFilename);

  return fill(POLICY.kinds.document, {
    name: named === undefined ? ` ${POLICY.kinds.unnamed_file}` : ` ${named.fileName}`,
    caption,
  });
}

/** One chat's recent messages, newest first, read off the live account: `max` at most, and none older than
 * `since` (epoch milliseconds) where one is named. A chat number the account cannot resolve is refused by
 * `ChatUnresolved`. */
export async function recentMessages(client: TelegramClient, peers: Peers, asked: MessagesAsked): Promise<RecentMessage[]> {
  let entity;

  try {
    entity = await resolveEntity(client, peerOf(asked.chatId, peers));
  } catch (error) {
    throw new ChatUnresolved(asked.chatId, error);
  }

  const names = new Map<number, string>();
  const messages: RecentMessage[] = [];

  for await (const read of client.iterMessages(entity, { limit: asked.max })) {
    if (!(read instanceof mtproto().Api.Message)) continue;

    if (asked.since !== null && read.date * 1000 < asked.since) break;

    const senderId = senderIdOf(read, peers);
    let senderName: string | undefined = senderId === null ? undefined : names.get(senderId);

    if (senderId !== null && senderName === undefined) {
      try {
        senderName = senderNameOf(await read.getSender(), String(senderId));
      } catch {
        senderName = String(senderId);
      }

      names.set(senderId, senderName);
    }

    const text = read.message ?? "";

    messages.push({
      messageId: read.id,
      senderId,
      senderName: senderName ?? "unknown",
      sentAt: read.date * 1000,
      text: text === "" ? placeholderOf(read) : text,
    });

    if (messages.length >= asked.max) break;
  }

  return messages;
}
