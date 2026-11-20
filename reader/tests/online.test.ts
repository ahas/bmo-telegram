import { expect, test } from "bun:test";

import { offlineWaitMs, standOnline } from "../src/online";
import { standingClock } from "./support";

// The owner's account kept online while the reader holds it (docs/reader.md, 「Typing and presence」).

const EVERY_MS = 60_000;

test("the account is said online at once and each renewal, a failed say not stopping the next, and offline once stopped", async () => {
  const clock = standingClock(0);
  const said: boolean[] = [];
  const online = standOnline(async (offline) => {
    said.push(offline);

    if (said.length === 2) {
      throw new Error("refused");
    }
  }, EVERY_MS, clock);

  expect(said).toEqual([false]);

  clock.move(EVERY_MS - 1);
  expect(said).toEqual([false]);

  clock.move(1);
  clock.move(EVERY_MS);
  expect(said).toEqual([false, false, false]);

  await online.stop();
  clock.move(EVERY_MS * 3);
  expect(said).toEqual([false, false, false, true]);
});

test("stopping lets go once Telegram has not answered the offline within the wait", async () => {
  const clock = standingClock(0);
  const online = standOnline((offline) => (offline ? new Promise<void>(() => {}) : Promise.resolve()), EVERY_MS, clock);
  let stopped = false;
  const stopping = online.stop().then(() => {
    stopped = true;
  });

  clock.move(offlineWaitMs - 1);
  await Promise.resolve();
  expect(stopped).toBe(false);

  clock.move(1);
  await stopping;
  expect(stopped).toBe(true);
});
