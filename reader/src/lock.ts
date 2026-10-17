// The session lock. Telegram ends a session whose string is used from two places at once, so the
// owner's account is opened only by the process holding this lock, a file in the provider's data
// folder naming the process that holds it. A lock whose process is gone is free: the process ended
// without letting it go.

import { closeSync, mkdirSync, openSync, readFileSync, rmSync, writeSync } from "node:fs";
import { dirname } from "node:path";

export interface HeldLock {
  release: () => Promise<void>;
}

function alive(pid: number): boolean {
  if (!Number.isInteger(pid) || pid <= 0) {
    return false;
  }

  try {
    process.kill(pid, 0);

    return true;
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === "EPERM";
  }
}

/** The lock at `path`, taken where no living process holds it, or null where one does. */
export function takeLock(path: string, pid: number = process.pid): HeldLock | null {
  mkdirSync(dirname(path), { recursive: true });

  for (let tries = 0; tries < 2; tries += 1) {
    try {
      const file = openSync(path, "wx");

      writeSync(file, String(pid));
      closeSync(file);

      return {
        release: async () => {
          if (readFileSync(path, "utf8").trim() === String(pid)) rmSync(path, { force: true });
        },
      };
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "EEXIST") throw error;

      const holder = Number.parseInt(readFileSync(path, "utf8").trim(), 10);

      if (alive(holder)) return null;

      rmSync(path, { force: true });
    }
  }

  return null;
}

/** What the reader asks before it opens the account, and how it waits between askings. */
export interface OpenAsking {
  // Whether the account may be opened at all: false while something else still holds it.
  mayOpen: () => Promise<boolean>;
  takeLock: () => Promise<HeldLock | null>;
  // Whether it still waits: false once it is stopping, and then it opens nothing.
  waits: () => boolean;
  pause: () => Promise<void>;
  // Said once, the first time it has to wait.
  waiting: () => void;
}

/** The session lock, taken once the account may be opened: asked again after each pause, or null where the waiting ended first. */
export async function lockOnceFree(asking: OpenAsking): Promise<HeldLock | null> {
  let said = false;

  while (asking.waits()) {
    const lock = await asking.mayOpen() ? await asking.takeLock() : null;

    if (lock !== null) return lock;

    if (!said) {
      said = true;
      asking.waiting();
    }

    await asking.pause();
  }

  return null;
}
