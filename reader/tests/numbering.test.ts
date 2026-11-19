// The numbering that crosses to the service (docs/reader.md, 「Numbers」): every room that does not number for
// everyone keeps its owner-account numbers below every bot number, and every such number comes home by turning
// the offset back.

import { describe, expect, test } from "bun:test";

import { accountMessageId, numbersForEveryone, providerMessageId } from "../src/numbering";

const GROUP = -5_000_000_002;
const CHANNEL = -1_000_000_000_003;
const PERSON = 5_000_000_123;

describe("which rooms number for everyone", () => {
  test("only a supergroup or a channel, by the number the service gives them", () => {
    expect(numbersForEveryone(CHANNEL)).toBe(true);
    expect(numbersForEveryone(GROUP)).toBe(false);
    expect(numbersForEveryone(PERSON)).toBe(false);
  });
});

describe("the provider's number for a line", () => {
  test("a room that numbers for everyone keeps its own", () => {
    expect(providerMessageId(CHANNEL, 6871)).toBe(6871);
  });

  test("any other room's line stands below every number the bot account can see, rising with the account's", () => {
    const older = providerMessageId(GROUP, 100);
    const newer = providerMessageId(GROUP, 500);

    expect(older).toBeLessThan(0);
    expect(newer).toBeGreaterThan(older);
    expect(newer).toBeLessThan(0);
    expect(providerMessageId(PERSON, 1)).toBeLessThan(providerMessageId(PERSON, 2));
  });

  test("no provider number equals a number either account could give a line of the room", () => {
    // The account's own numbers are positive, and the bot account's are positive too.
    expect(providerMessageId(GROUP, 900)).toBeLessThan(0);
    expect(providerMessageId(PERSON, 900)).toBeLessThan(0);
  });
});

describe("turning a number back", () => {
  test("every provider number of a per-account room comes home to the account's own", () => {
    for (const own of [1, 42, 900, 12, 999_999_999]) {
      for (const room of [GROUP, PERSON]) {
        expect(accountMessageId(room, providerMessageId(room, own))).toBe(own);
      }
    }
  });

  test("a number that stands crosses as it stands", () => {
    expect(accountMessageId(CHANNEL, 6871)).toBe(6871);
    expect(accountMessageId(GROUP, 900)).toBe(900);
    expect(accountMessageId(PERSON, 42)).toBe(42);
  });

  test("a negative number of a room that numbers for everyone is no provider number to turn", () => {
    expect(accountMessageId(CHANNEL, -999_999_100)).toBe(-999_999_100);
  });
});
