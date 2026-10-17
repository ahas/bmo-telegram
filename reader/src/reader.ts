// The live reading on the owner's account (docs/reader.md, 「What it is for」). Telegram never delivers
// one bot's messages to another bot, so the bot account alone cannot see what other bots say in a shared
// room. This reads every new message another bot writes in a room the receiver listens to and hands it to
// the service, which hands it to the receiver as read in the `human-reader` mode.

import { messageHasPhoto, pictureOf, readerFileOf, seconds } from "./media";
import { mtproto, mtprotoEvents, mtprotoSessions } from "./mtproto";
import { fill, POLICY } from "./policy";
import { base64Of, roomFacts, via } from "./service";

import type { Api, TelegramClient as Client } from "telegram";
import type { NewMessageEvent } from "telegram/events";
import type { AccountConfig } from "./config";
import type { Peers } from "./media";
import type { HandedContext, HandedMessage, Handover, Service } from "./service";

export interface ReaderDeps {
  account: AccountConfig;
  peers: Peers;
  service: Service;
}

const kinds = POLICY.kinds;

// The mark a wordless message from another bot is handed over under, in the words the bot account's
// reading uses for the same kinds.
export function readerMark(message: Api.Message, carriesPhoto: boolean): string {
  if (carriesPhoto) {
    return fill(kinds.photo, { caption: "" });
  }

  const { Api } = mtproto();
  const attributes = message.document?.attributes ?? [];
  const sticker = attributes.find((one): one is Api.DocumentAttributeSticker => one instanceof Api.DocumentAttributeSticker);

  if (sticker !== undefined) {
    return fill(kinds.sticker, { emoji: ` ${sticker.alt}`, set: "" });
  }

  const video = attributes.find((one): one is Api.DocumentAttributeVideo => one instanceof Api.DocumentAttributeVideo);

  if (video !== undefined) {
    const duration = seconds(Math.round(video.duration));

    if (video.roundMessage === true) {
      return fill(kinds.video_note, { duration });
    }

    return attributes.some((one) => one instanceof Api.DocumentAttributeAnimated)
      ? fill(kinds.gif, { duration })
      : fill(kinds.video, { duration, caption: "" });
  }

  const name = attributes.find((one): one is Api.DocumentAttributeFilename => one instanceof Api.DocumentAttributeFilename);

  return fill(kinds.document, { name: ` ${name?.fileName ?? kinds.unnamed_file}`, caption: "" });
}

// Telegram's own words and the custom emoji among their marks, as the bot account's reading keeps them.
function metadataOf(message: Api.Message, text: string): unknown {
  const { Api } = mtproto();

  return {
    text,
    entities: (message.entities ?? []).flatMap((entity) =>
      entity instanceof Api.MessageEntityCustomEmoji
        ? [{ type: "custom_emoji", offset: entity.offset, length: entity.length, custom_emoji_id: String(entity.documentId) }]
        : [],
    ),
  };
}

// The file a message from another bot carries, gathered and handed over once it is had, or why not.
async function gather(deps: ReaderDeps, client: Client, message: Api.Message, chatId: number): Promise<void> {
  const document = message.document!;
  const file = readerFileOf(document);
  const base = { chatId, messageId: message.id, via };

  try {
    const data = await client.downloadMedia(message);

    if (!(data instanceof Uint8Array)) {
      throw new Error("downloadMedia returned no data");
    }

    await deps.service.inbound({
      kind: "gathered",
      ...base,
      outcome: "kept",
      files: [{ name: file.name, mime: document.mimeType, bytes: base64Of(data) }],
    });
  } catch (error) {
    console.error(`reader keeping ${file.name} from chat ${chatId} failed:`, error);
    await deps.service.inbound({ kind: "gathered", ...base, outcome: "not-kept", reason: POLICY.gathering.file_not_kept });
  }
}

/** One new message the account read, handed to the service where it is another bot's in a listened room. */
export async function ingest(deps: ReaderDeps, client: Client, event: NewMessageEvent): Promise<void> {
  const { Api } = mtproto();
  const message = event.message;
  const text = message.message;
  const eventChat = event.chatId;

  if (message.out === true || eventChat === undefined) {
    return;
  }

  const chatId = Number(eventChat.toString());

  if (!(await deps.service.listened(chatId))) {
    return;
  }

  const chat = await message.getChat();
  const roomTitle = chat instanceof Api.Chat || chat instanceof Api.Channel ? chat.title : undefined;
  const sender = await message.getSender();

  if (!(sender instanceof Api.User) || sender.bot !== true) {
    return;
  }

  const senderId = Number(sender.id.toString());

  // The provider's own bot is read by the bot account, so its words are not read in again here.
  if (senderId === deps.peers.botId) {
    return;
  }

  const document = message.document;
  const file = document === undefined || document === null ? null : readerFileOf(document);
  const carriesPhoto = file?.svg !== true && messageHasPhoto(message);

  if (text === "" && !carriesPhoto && file === null) {
    return;
  }

  const replyHeader = message.replyTo;
  const handed: HandedMessage = {
    chatId,
    messageId: message.id,
    senderId,
    senderName: sender.firstName ?? sender.username ?? String(senderId),
    sentAt: message.date,
    text: text === "" ? readerMark(message, carriesPhoto) : text,
    replyToMessageId: replyHeader instanceof Api.MessageReplyHeader ? (replyHeader.replyToMsgId ?? null) : null,
    via,
    album: null,
    mode: "human-reader",
  };
  const context: HandedContext = {
    ...(roomTitle === undefined ? {} : { roomTitle }),
    ...(text === "" ? {} : { spokenText: text }),
    metadata: metadataOf(message, text),
  };
  const handover: Handover = { message: handed, context, room: roomFacts(chatId, deps.peers.ownerId, deps.peers.botId) };

  // A photo, or an image sent as an uncompressed file: handed over with its bytes where they could be had.
  if (carriesPhoto) {
    const picture = await pictureOf(client, message);

    if (picture === null) {
      await deps.service.inbound({ kind: "message", ...handover });
    } else {
      await deps.service.inbound({ kind: "attachment", ...handover, context: { ...context, carriesFile: true }, files: [picture] });
    }

    return;
  }

  if (file === null) {
    await deps.service.inbound({ kind: "message", ...handover });

    return;
  }

  // A file somebody handed over — a document that is neither a picture nor a sticker — goes as an
  // attachment, which with no caption wakes nobody but the senders the receiver names; a caption is
  // words in the room and goes as a message. Either way its bytes follow once they are had.
  const kind = file.sticker || text !== "" ? "message" : "attachment";

  await deps.service.inbound({ kind, ...handover, context: { ...context, carriesFile: true, gathering: true } });
  await gather(deps, client, message, chatId);
}

// One connection on the owner's account. A session must not be used from two places at once, so
// everything that needs the account — the live reading, the history walk, the profile pictures, the
// owner's sends — shares the client this returns.
export async function connectReader(account: AccountConfig): Promise<Client> {
  const client = new (mtproto().TelegramClient)(
    new (mtprotoSessions().StringSession)(account.session),
    account.apiId,
    account.apiHash,
    // A flood wait up to this long is slept through rather than raised.
    { connectionRetries: 5, floodSleepThreshold: 120 },
  );

  await client.connect();

  if (!(await client.isUserAuthorized())) {
    await client.disconnect();
    throw new Error("the stored session is not authorized; a new one is made with `bun run login`");
  }

  return client;
}

/** The account connected, every new message it reads handed on. */
export async function startReader(deps: ReaderDeps): Promise<Client> {
  const client = await connectReader(deps.account);

  client.addEventHandler(async (event: NewMessageEvent) => {
    try {
      await ingest(deps, client, event);
    } catch (error) {
      console.error("reader ingest failed:", error);
    }
  }, new (mtprotoEvents().NewMessage)({ incoming: true }));

  return client;
}
