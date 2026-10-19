import { describe, expect, test } from "bun:test";
import { Api } from "telegram";

import { fetchFile, readerFileOf } from "../src/media";
import type { VideoKind } from "../src/media";
import { BOT_ID, OWNER_ID } from "./support";

import type { TelegramClient } from "telegram";

const peers = { ownerId: OWNER_ID, botId: BOT_ID };

function videoMessage(id: number, attributes: Api.TypeDocumentAttribute[]): Api.Message {
  return new Api.Message({
    id,
    peerId: new Api.PeerChat({ chatId: 10n as never }),
    date: 1_788_487_000,
    message: "",
    media: new Api.MessageMediaDocument({
      document: new Api.Document({
        id: BigInt(700 + id) as never,
        accessHash: 2n as never,
        fileReference: Buffer.from([]),
        date: 1_788_487_000,
        mimeType: "video/mp4",
        size: 30_000_000n as never,
        dcId: 2,
        attributes,
      }),
    }),
  });
}

function account(message: Api.Message | null): { asked: number[]; client: TelegramClient } {
  const asked: number[] = [];

  return {
    asked,
    client: {
      getMessages: async (_entity: unknown, options: { ids?: number[] }) => {
        asked.push(...(options.ids ?? []));

        return message === null ? [] : [message];
      },
      downloadMedia: async () => new TextEncoder().encode("video bytes"),
    } as unknown as TelegramClient,
  };
}

describe("videos", () => {
  test("one too large for the Bot API is fetched through the reader, and with no reader is answered as none", async () => {
    const reader = account(videoMessage(60, [new Api.DocumentAttributeVideo({ duration: 3, w: 640, h: 360 })]));
    const fetched = await fetchFile(reader.client, peers, { chatId: -10, messageId: 60, sentAt: 1_788_487_000 });

    expect(fetched.outcome).toBe("fetched");
    expect(fetched.outcome === "fetched" ? Buffer.from(fetched.bytes, "base64").toString() : null).toBe("video bytes");
    expect(fetched.outcome === "fetched" ? [fetched.id, fetched.name, fetched.mime] : null).toEqual(["document-760", "video.mp4", "video/mp4"]);
    expect(reader.asked).toEqual([60]);
    expect(await fetchFile(null, peers, { chatId: -10, messageId: 61, sentAt: 1_788_487_000 })).toEqual({
      outcome: "no-reader",
      reason: "the owner's account is not connected",
    });
  });

  test("one another bot sent is named by the reader as a video, a round note and a GIF told apart", () => {
    const cases: [number, Api.TypeDocumentAttribute[], string, VideoKind][] = [
      [62, [new Api.DocumentAttributeVideo({ duration: 3, w: 640, h: 360 })], "video.mp4", "video"],
      [63, [new Api.DocumentAttributeVideo({ duration: 2, w: 384, h: 384, roundMessage: true })], "video-note.mp4", "video-note"],
      [64, [new Api.DocumentAttributeVideo({ duration: 1, w: 320, h: 240 }), new Api.DocumentAttributeAnimated()], "gif.mp4", "gif"],
    ];

    for (const [id, attributes, name, video] of cases) {
      const file = readerFileOf(videoMessage(id, attributes).document!);

      expect({ name: file.name, video: file.video }).toEqual({ name, video });
    }
  });
});
