// What the reader's tests share: neutral numbers, a scratch folder taken down after each test, a service
// that keeps what it is handed, and a clock that stands still until moved.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { Clock } from "../src/presence";
import type { HistoryRoom, Inbound, Rooms, Service } from "../src/service";

export const OWNER_ID = 42;
export const ROOM_ID = -100;
export const BOT_ID = 7_000_000_001;

const made: string[] = [];

/** A fresh folder under the system's temporary one, taken down by `sweepScratch`. */
export function scratch(prefix: string): string {
  const directory = mkdtempSync(join(tmpdir(), `telegram-reader-${prefix}-`));

  made.push(directory);

  return directory;
}

export function sweepScratch(): void {
  for (const directory of made.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
}

export interface KeptService extends Service {
  handed: Inbound[];
  changed: { rooms: number[]; people: number[] }[];
}

/** A service that keeps every hand-over, names the rooms it is given, and has seen what it is told. */
export function keptService(options: { rooms?: HistoryRoom[] | string; seen?: { rooms: number[]; people: number[] }; kept?: boolean } = {}): KeptService {
  const handed: Inbound[] = [];
  const changed: { rooms: number[]; people: number[] }[] = [];

  return {
    handed,
    changed,
    reachable: async () => true,
    inbound: async (inbound) => {
      handed.push(inbound);

      return inbound.kind === "message" && inbound.message.origin === "history"
        ? { kept: options.kept ?? true }
        : {};
    },
    listened: async () => ({ listened: true, people: false, tells: false }),
    rooms: async (): Promise<Rooms> =>
      typeof options.rooms === "string" ? { unanswered: options.rooms } : { rooms: options.rooms ?? [] },
    seen: async () => options.seen ?? { rooms: [], people: [] },
    avatarsChanged: async (rooms, people) => {
      changed.push({ rooms, people });
    },
    reactions: async () => {},
    deleted: async () => {},
    typing: async () => {},
    presence: async () => {},
  };
}

/** A clock that stands still until moved, running what was set to run once its moment has come. */
export function standingClock(start: number): Clock & { move(ms: number): void } {
  let now = start;
  let waiting: { at: number; run: () => void }[] = [];

  return {
    now: () => now,
    after: (ms, run) => {
      const one = { at: now + ms, run };

      waiting.push(one);

      return () => {
        waiting = waiting.filter((other) => other !== one);
      };
    },
    move: (ms) => {
      now += ms;
      const due = waiting.filter((one) => one.at <= now);

      waiting = waiting.filter((one) => one.at > now);
      due.forEach((one) => one.run());
    },
  };
}
