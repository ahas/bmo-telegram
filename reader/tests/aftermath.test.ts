import { describe, expect, test } from "bun:test";

import { deletionsOf, reactionsOf, SeenMessages } from "../src/aftermath";

// What the owner's account sees happen to a message afterwards (docs/reader.md, 「What it hands over」): a message of
// a supergroup or a channel by its number, one of any other room by the stamp the reader saw it under, and one the
// reader never saw passed over.

const seen = (): SeenMessages => {
  const messages = new SeenMessages();

  messages.remember(701, -5_000_000_001, { senderId: 55, sentAt: 1_700_000_000_000 });
  messages.remember(702, -5_000_000_001, { senderId: 56, sentAt: 1_700_000_001_000 });

  return messages;
};

describe("a deletion the account sees", () => {
  test("names a channel's messages by their numbers", () => {
    expect(deletionsOf({ className: "UpdateDeleteChannelMessages", channelId: 1234567890n, messages: [7, 8] }, seen())).toEqual([
      { roomId: -1001234567890, messageIds: [7, 8], stamps: [] },
    ]);
  });

  test("names another room's messages by their stamps and passes over one it never saw", () => {
    expect(deletionsOf({ className: "UpdateDeleteMessages", messages: [701, 702, 999] }, seen())).toEqual([
      { roomId: -5_000_000_001, messageIds: [], stamps: [{ senderId: 55, sentAt: 1_700_000_000_000 }, { senderId: 56, sentAt: 1_700_000_001_000 }] },
    ]);
  });
});

describe("a message's reactions the account sees", () => {
  const update = (peer: object, msgId: number, recent: { userId: number; emoticon: string }[], counted: number) =>
    ({
      peer,
      msgId,
      reactions: {
        results: [{ count: counted }],
        recentReactions: recent.map(({ userId, emoticon }) => ({
          peerId: { className: "PeerUser", userId },
          reaction: { className: "ReactionEmoji", emoticon },
        })),
      },
    }) as never;

  test("names a channel's message by its number, whole where every reaction is listed", () => {
    expect(reactionsOf(update({ className: "PeerChannel", channelId: 1234567890n }, 7, [{ userId: 56, emoticon: "\u{1F44D}" }], 1), seen())).toEqual({
      roomId: -1001234567890,
      messageId: 7,
      reactions: [{ personId: 56, emojis: ["\u{1F44D}"] }],
      whole: true,
    });
  });

  test("names another room's message by its stamp, not whole where reactions go unlisted", () => {
    expect(reactionsOf(update({ className: "PeerChat", chatId: 5_000_000_001n }, 702, [{ userId: 55, emoticon: "\u{2764}" }], 3), seen())).toEqual({
      roomId: -5_000_000_001,
      stamp: { senderId: 56, sentAt: 1_700_000_001_000 },
      reactions: [{ personId: 55, emojis: ["\u{2764}"] }],
      whole: false,
    });
    expect(reactionsOf(update({ className: "PeerChat", chatId: 5_000_000_001n }, 999, [], 0), seen())).toBeNull();
  });
});
