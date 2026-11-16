// The live reading on the owner's account (docs/reader.md, 「What it is for」). Telegram never delivers
// one bot's messages to another bot, so the bot account alone cannot see what other bots say in a shared
// room. This reads every new message another bot writes in a room the receiver listens to, and a person's in
// a room the bot is not in where the service says it is the reader's, the owner's own there among them, and
// hands it to the service, which hands it to the receiver as read in the `human-reader` mode.

import { fileIdOf, messageFileKind, messageHasPhoto, pictureOf, readerFileOf, seconds } from "./media";
import { deletionsOf, reactionsOf, SeenMessages } from "./aftermath";
import { mtproto, mtprotoEvents, mtprotoSessions } from "./mtproto";
import { fill, POLICY } from "./policy";
import { startHearing } from "./presence";
import { base64Of, handedMessage, roomFacts } from "./service";

import type { Api, TelegramClient as Client } from "telegram";
import type { NewMessageEvent } from "telegram/events";
import type { AccountConfig } from "./config";
import type { Peers } from "./media";
import type { OwnSends } from "./sends";
import type { HandedContext, RoomReading, Service } from "./service";

export interface ReaderDeps {
  account: AccountConfig;
  peers: Peers;
  service: Service;
  sends: OwnSends;
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

// The sticker a message is, in the bot account's reading's shape as far as the account can tell it: its emoji,
// its set's name where Telegram names one, and whether it is an animated scene or a video; null for none.
function stickerOf(message: Api.Message): Record<string, unknown> | null {
  const { Api } = mtproto();
  const document = message.document;
  const sticker = document?.attributes.find((one): one is Api.DocumentAttributeSticker => one instanceof Api.DocumentAttributeSticker);

  if (document === undefined || document === null || sticker === undefined) {
    return null;
  }

  const mime = document.mimeType.toLowerCase();

  return {
    emoji: sticker.alt,
    ...(sticker.stickerset instanceof Api.InputStickerSetShortName ? { set_name: sticker.stickerset.shortName } : {}),
    is_animated: mime === "application/x-tgsticker",
    is_video: mime === "video/webm",
  };
}

// Telegram's own words, the custom emoji among their marks and the sticker the message is, as the bot account's
// reading keeps them, so a room draws the sticker where it can.
function metadataOf(message: Api.Message, text: string): unknown {
  const { Api } = mtproto();

  return {
    text,
    entities: (message.entities ?? []).flatMap((entity) =>
      entity instanceof Api.MessageEntityCustomEmoji
        ? [{ type: "custom_emoji", offset: entity.offset, length: entity.length, custom_emoji_id: String(entity.documentId) }]
        : [],
    ),
    sticker: stickerOf(message),
  };
}

// The file a message from another bot carries, gathered and handed over once it is had, or why not.
async function gather(deps: ReaderDeps, client: Client, message: Api.Message, chatId: number): Promise<void> {
  const document = message.document!;
  const file = readerFileOf(document);

  try {
    const data = await client.downloadMedia(message);

    if (!(data instanceof Uint8Array)) {
      throw new Error("downloadMedia returned no data");
    }

    await deps.service.inbound({
      kind: "gathered",
      roomId: chatId,
      messageId: message.id,
      patch: {
        files: [{
          id: fileIdOf(message),
          name: file.name,
          mime: document.mimeType,
          bytes: base64Of(data),
          kind: messageFileKind(document),
        }],
      },
    });
  } catch (error) {
    console.error(`reader keeping ${file.name} from chat ${chatId} failed:`, error);
    await deps.service.inbound({
      kind: "gathered",
      roomId: chatId,
      messageId: message.id,
      patch: { files: [], gatherFailure: POLICY.gathering.file_not_kept },
    });
  }
}

/** Whether a message from `sender` is the reader's to hand over in a room the service says `room` of: another
 * bot's but the provider's own wherever the room is listened to, a person's only where the service says the
 * room's people are the reader's, and the owner's own (`out`) there too, but for one the reader's own send
 * went out as (`ownSend`). */
export function handsOver(
  sender: { id: number; bot: boolean; out?: boolean; ownSend?: boolean },
  room: RoomReading,
  botId: number | null,
): boolean {
  if (!room.listened) {
    return false;
  }

  if (sender.out === true) {
    return room.people && sender.ownSend !== true;
  }

  return sender.bot ? sender.id !== botId : room.people;
}

/** One new message the account read, handed to the service where it is the reader's to hand over. */
export async function ingest(deps: ReaderDeps, client: Client, event: NewMessageEvent): Promise<void> {
  const { Api } = mtproto();
  const message = event.message;
  const text = message.message;
  const eventChat = event.chatId;

  if (eventChat === undefined) {
    return;
  }

  const chatId = Number(eventChat.toString());
  const room = await deps.service.listened(chatId);

  if (!room.listened) {
    return;
  }

  const sender = await message.getSender();

  if (!(sender instanceof Api.User)) {
    return;
  }

  // The account's own message is the owner's, under the number the bot account knows the owner by; one the
  // reader's own send went out as is the sender's already.
  const out = message.out === true;
  const ownSend = out && room.people && (await deps.sends.isOwn(chatId, message.id));
  const senderId = out ? deps.peers.ownerId : Number(sender.id.toString());

  // The provider's own bot is read by the bot account, so its words are not read in again here; a person's
  // words, the owner's among them, are the bot account's to read wherever the bot is.
  if (!handsOver({ id: senderId, bot: !out && sender.bot === true, out, ownSend }, room, deps.peers.botId)) {
    return;
  }

  const chat = await message.getChat();
  const roomTitle = chat instanceof Api.Chat || chat instanceof Api.Channel ? chat.title : undefined;

  const document = message.document;
  const file = document === undefined || document === null ? null : readerFileOf(document);
  const carriesPhoto = file?.svg !== true && messageHasPhoto(message);

  if (text === "" && !carriesPhoto && file === null) {
    return;
  }

  const replyHeader = message.replyTo;
  const context: HandedContext = {
    ...(roomTitle === undefined ? {} : { roomTitle }),
    ...(text === "" ? {} : { spokenText: text }),
    metadata: metadataOf(message, text),
  };
  const handed = handedMessage({
    chatId,
    messageId: message.id,
    origin: "live",
    senderId,
    senderName: sender.firstName ?? sender.username ?? String(senderId),
    sentAt: message.date,
    text: text === "" ? readerMark(message, carriesPhoto) : text,
    replyToMessageId: replyHeader instanceof Api.MessageReplyHeader ? (replyHeader.replyToMsgId ?? null) : null,
    context,
    room: roomFacts(chatId, deps.peers.ownerId, deps.peers.botId),
  });

  // A photo, or an image sent as an uncompressed file: handed over as words are, with its bytes where they
  // could be had.
  if (carriesPhoto) {
    const picture = await pictureOf(client, message);

    if (picture !== null) {
      handed.context = { ...context, carriesFile: true };
      handed.files = [picture];
    }

    await deps.service.inbound({ kind: "message", roomId: chatId, messageId: message.id, message: handed });

    return;
  }

  if (file === null) {
    await deps.service.inbound({ kind: "message", roomId: chatId, messageId: message.id, message: handed });

    return;
  }

  // A file somebody handed over whose bytes are still on their way: the message says so and the files follow
  // once they are had. A caption is words in the room, and the bytes follow all the same.
  handed.context = { ...context, carriesFile: true };
  handed.gathering = true;

  await deps.service.inbound({ kind: "message", roomId: chatId, messageId: message.id, message: handed });
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
    // A connection is tried again as often as it takes, and a flood wait Telegram names is slept through,
    // however long, the request then asked again as often as it is answered with one.
    { connectionRetries: Infinity, requestRetries: Infinity, floodSleepThreshold: Infinity },
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
  const { Api } = mtproto();
  // What the reader saw of each message, so a deletion or a reaction the account names by its own number finds
  // the message in a room that numbers it apart (docs/reader.md, 「What it hands over」).
  const seen = new SeenMessages();
  // Who is writing and who is reachable (docs/reader.md, 「Typing and presence」).
  const hearing = startHearing({ client, service: deps.service, peers: deps.peers });

  client.addEventHandler(async (event: NewMessageEvent) => {
    const message = event.message;

    if (event.chatId !== undefined) {
      seen.remember(message.id, Number(event.chatId.toString()), {
        senderId: message.out === true ? deps.peers.ownerId : Number(message.senderId?.toString() ?? 0),
        sentAt: message.date * 1000,
      });
    }

    try {
      await ingest(deps, client, event);
    } catch (error) {
      console.error("reader ingest failed:", error);
    }
  }, new (mtprotoEvents().NewMessage)({}));

  client.addEventHandler(async (update: Api.TypeUpdate) => {
    try {
      if (update instanceof Api.UpdateDeleteMessages || update instanceof Api.UpdateDeleteChannelMessages) {
        for (const deletion of deletionsOf(update, seen)) {
          await deps.service.deleted(deletion);
        }
      } else if (update instanceof Api.UpdateMessageReactions) {
        const reactions = reactionsOf(update, seen);

        if (reactions !== null) {
          await deps.service.reactions(reactions);
        }
      } else {
        await hearing.update(update);
      }
    } catch (error) {
      console.error("reader aftermath failed:", error);
    }
  }, new (mtprotoEvents().Raw)({}));

  return client;
}
