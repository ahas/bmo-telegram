// Which messages the live reading hands over (docs/reader.md, 「What it is for」): another bot's wherever the
// room is listened to, never the provider's own bot's, and a person's only where the service says the room's
// people are the reader's.

import { describe, expect, test } from "bun:test";

import { handsOver } from "../src/reader";

const botId = 7_000_000_001;
const otherBot = { id: 9, bot: true };
const person = { id: 55, bot: false };

describe("handsOver", () => {
  test("another bot's words go wherever the room is listened to", () => {
    expect(handsOver(otherBot, { listened: true, people: false }, botId)).toBe(true);
    expect(handsOver(otherBot, { listened: false, people: true }, botId)).toBe(false);
  });

  test("the provider's own bot is the bot account's to read", () => {
    expect(handsOver({ id: botId, bot: true }, { listened: true, people: true }, botId)).toBe(false);
  });

  test("a person's words go only where the room's people are the reader's", () => {
    expect(handsOver(person, { listened: true, people: true }, botId)).toBe(true);
    expect(handsOver(person, { listened: true, people: false }, botId)).toBe(false);
    expect(handsOver(person, { listened: false, people: true }, botId)).toBe(false);
  });
});
