import { describe, expect, test } from "bun:test";

import { albumSendOptions, attachmentKind, attachmentName, attachmentSendOptions, heldOwnerAccount, LineUnmatched, MessageGone, NotOwnLine, pinServiceMessageId, turnedDown, TurnedDown, wordsHash } from "../src/owner";
import { OwnSends } from "../src/sends";
import { BOT_ID, OWNER_ID, ROOM_ID } from "./support";

import type { TelegramClient } from "telegram";

describe("what Telegram makes of an attached file", () => {
  test("a JPEG and a PNG are photos", () => {
    expect(attachmentKind("image/jpeg", "IMG_0421.jpg")).toBe("photo");
    expect(attachmentKind("image/png", "shot.png")).toBe("photo");
    expect(attachmentKind("IMAGE/JPEG", "shot.JPG")).toBe("photo");
  });

  test("a GIF moves", () => {
    expect(attachmentKind("image/gif", "cat.gif")).toBe("animation");
  });

  test("any other image is kept whole as a file rather than recompressed into something else", () => {
    expect(attachmentKind("image/webp", "sticker.webp")).toBe("document");
    expect(attachmentKind("image/svg+xml", "chart.svg")).toBe("document");
    expect(attachmentKind("image/heic", "IMG_0001.heic")).toBe("document");
  });

  test("a video is a video and a sound is an audio", () => {
    expect(attachmentKind("video/mp4", "clip.mp4")).toBe("video");
    expect(attachmentKind("video/quicktime", "clip.mov")).toBe("video");
    expect(attachmentKind("audio/mpeg", "song.mp3")).toBe("audio");
    expect(attachmentKind("audio/ogg", "voice.ogg")).toBe("audio");
  });

  test("everything else is a file", () => {
    expect(attachmentKind("application/pdf", "report.pdf")).toBe("document");
    expect(attachmentKind("text/plain", "notes.txt")).toBe("document");
    expect(attachmentKind("application/octet-stream", "blob.bin")).toBe("document");
  });

  test("a file that arrived with no type of its own is read off its name", () => {
    expect(attachmentKind("", "IMG_0421.JPG")).toBe("photo");
    expect(attachmentKind("", "clip.mp4")).toBe("video");
    expect(attachmentKind("", "archive.tar.gz")).toBe("document");
    expect(attachmentKind("", "")).toBe("document");
  });
});

describe("the name a file travels under", () => {
  test("a photo pasted with no ending is given the one its type says", () => {
    expect(attachmentName("photo", "clipboard", "image/png")).toBe("clipboard.png");
    expect(attachmentName("photo", "clipboard", "image/jpeg")).toBe("clipboard.jpg");
    expect(attachmentName("photo", "", "image/jpeg")).toBe("attachment.jpg");
  });

  test("a photo that already says what it is keeps its own name", () => {
    expect(attachmentName("photo", "IMG_0421.jpg", "image/jpeg")).toBe("IMG_0421.jpg");
    expect(attachmentName("photo", "shot.PNG", "image/png")).toBe("shot.PNG");
  });

  test("every other kind keeps its name exactly", () => {
    expect(attachmentName("document", "final report.pdf", "application/pdf")).toBe("final report.pdf");
    expect(attachmentName("video", "clip.mkv", "video/x-matroska")).toBe("clip.mkv");
  });
});

describe("what the account is told", () => {
  test("only a document is forced to stay one, and only a video may stream", () => {
    expect(attachmentSendOptions("photo")).toEqual({ forceDocument: false, supportsStreaming: false, animated: false });
    expect(attachmentSendOptions("video")).toEqual({ forceDocument: false, supportsStreaming: true, animated: false });
    expect(attachmentSendOptions("animation")).toEqual({ forceDocument: false, supportsStreaming: false, animated: true });
    expect(attachmentSendOptions("audio")).toEqual({ forceDocument: false, supportsStreaming: false, animated: false });
    expect(attachmentSendOptions("document")).toEqual({ forceDocument: true, supportsStreaming: false, animated: false });
  });
});

describe("the albums a send of several goes as", () => {
  test("an album of documents stays documents, and one carrying a video streams", () => {
    expect(albumSendOptions(["document", "document"])).toEqual({ forceDocument: true, supportsStreaming: false, animated: false });
    expect(albumSendOptions(["photo", "video"])).toEqual({ forceDocument: false, supportsStreaming: true, animated: false });
  });
});

// The owner's account over a client that holds the lines given by number and answers an edit as `answer` says,
// keeping each edit it was asked for.
function accountHolding(lines: Record<number, { out: boolean; message: string }>, answer: (text: string, parseMode: unknown) => string | null) {
  const edits: { message: number; text: string; parseMode: unknown }[] = [];
  const client = {
    getEntity: async () => ({}),
    getMessages: async (_entity: unknown, { ids }: { ids: number[] }) => ids.map((id) => (lines[id] === undefined ? undefined : { id, ...lines[id] })),
    editMessage: async (_entity: unknown, asked: { message: number; text: string; parseMode: unknown }) => {
      edits.push(asked);

      const refused = answer(asked.text, asked.parseMode);

      if (refused !== null) throw { code: 400, errorMessage: refused };

      return { message: asked.text };
    },
  } as unknown as TelegramClient;

  return { account: heldOwnerAccount(() => client, { ownerId: OWNER_ID, botId: BOT_ID }, new OwnSends()), edits };
}

describe("the owner's own line edited", () => {
  const edit = (messageId: number, markup: string | null) => ({ chatId: ROOM_ID, messageId, text: "new words", markup });

  test("goes in its markup, and as its plain words where Telegram refuses the markup", async () => {
    const { account, edits } = accountHolding({ 900: { out: true, message: "old" } }, (_, parseMode) => (parseMode === "html" ? "ENTITY_BOUNDS_INVALID" : null));

    expect(await account.edit(edit(900, "<b>new</b> words"))).toEqual({ id: 900, text: "new words", edited: true });
    expect(edits.map((one) => [one.message, one.text, one.parseMode])).toEqual([
      [900, "<b>new</b> words", "html"],
      [900, "new words", false],
    ]);
  });

  test("words the line already says change nothing, and Telegram's refusal crosses in its own words", async () => {
    const same = accountHolding({ 900: { out: true, message: "new words" } }, () => "MESSAGE_NOT_MODIFIED");

    expect(await same.account.edit(edit(900, null))).toEqual({ id: 900, text: "new words", edited: false });

    const long = accountHolding({ 900: { out: true, message: "old" } }, () => "MESSAGE_TOO_LONG");

    await expect(long.account.edit(edit(900, "<b>new</b> words"))).rejects.toEqual(new TurnedDown("MESSAGE_TOO_LONG"));
  });

  test("is refused for a line that is not the account's own, and for a number it holds no line under", async () => {
    const { account, edits } = accountHolding({ 901: { out: false, message: "someone's" } }, () => null);

    await expect(account.edit(edit(901, null))).rejects.toBeInstanceOf(NotOwnLine);
    await expect(account.edit(edit(902, null))).rejects.toBeInstanceOf(MessageGone);
    expect(edits).toEqual([]);
  });
});

describe("what Telegram answers an act", () => {
  test("a refusal for what it asked is turned down in Telegram's words, and its flood limit is not", () => {
    expect(turnedDown({ code: 400, errorMessage: "CHAT_ADMIN_REQUIRED" })).toEqual(new TurnedDown("CHAT_ADMIN_REQUIRED"));

    const flood = { code: 420, errorMessage: "FLOOD_WAIT_30" };

    expect(turnedDown(flood)).toBe(flood);
  });

  test("a pin names the service message Telegram wrote for it, and an answer with none names nothing", () => {
    const pinned = {
      updates: [
        { className: "UpdateMessageID", id: 30 },
        { className: "UpdateNewMessage", message: { className: "MessageService", id: 31, action: { className: "MessageActionPinMessage" } } },
      ],
    };

    expect(pinServiceMessageId(pinned)).toBe(31);
    expect(pinServiceMessageId({ updates: [{ className: "UpdatePinnedMessages", messages: [9] }] })).toBeNull();
  });
});

describe("a line named by its stamp", () => {
  // The room's lines of one second as the account reads them, newest first, each with who sent it.
  const second = 1_700_000_000;
  const held = (lines: { id: number; date: number; out: boolean; senderId: number; message: string }[]) => {
    const asked: unknown[] = [];
    const client = {
      getEntity: async () => ({}),
      getMessages: async (_entity: unknown, options: unknown) => {
        asked.push(options);

        return lines.map((line) => ({ ...line, senderId: BigInt(line.senderId) }));
      },
    } as unknown as TelegramClient;

    return { account: heldOwnerAccount(() => client, { ownerId: OWNER_ID, botId: BOT_ID }, new OwnSends()), asked };
  };
  const stamp = (senderId: number, words: string) => ({ chatId: OWNER_ID, senderId, sentAt: second * 1000, words: wordsHash(words) });

  test("the hash of a line's words is the one the service reckons", () => {
    expect(wordsHash("héllo there")).toBe("1702af18d505d036fbde1cdf11961be738378371e39204f212dcbbc6f306ab19");
  });

  test("names the one line from that sender at that second with those words, the account's own being the owner's", async () => {
    const { account, asked } = held([
      { id: 812, date: second, out: false, senderId: BOT_ID, message: "an answer" },
      { id: 811, date: second, out: true, senderId: OWNER_ID, message: "a question" },
      { id: 810, date: second - 1, out: false, senderId: BOT_ID, message: "an answer" },
    ]);

    expect(await account.line(stamp(BOT_ID, "an answer"))).toBe(812);
    expect(await account.line(stamp(OWNER_ID, "a question"))).toBe(811);
    expect(asked[0]).toEqual({ offsetDate: second + 1, limit: 100 });
  });

  test("names none where no line answers the stamp or more than one does", async () => {
    const { account } = held([
      { id: 812, date: second, out: false, senderId: BOT_ID, message: "twice" },
      { id: 811, date: second, out: false, senderId: BOT_ID, message: "twice" },
    ]);

    await expect(account.line(stamp(BOT_ID, "twice"))).rejects.toEqual(new LineUnmatched(2));
    await expect(account.line(stamp(BOT_ID, "never said"))).rejects.toEqual(new LineUnmatched(0));
  });
});
