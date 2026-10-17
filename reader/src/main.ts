// The reader's process (docs/reader.md): it listens on the loopback for the service, waits until the
// service answers and the session lock is its own, then connects the owner's account and starts the live
// reading, the history walk and the profile pictures on the one connection. Without the account's three
// values it opens nothing and its health says why.

import { startAvatarSweeper } from "./avatars";
import { accountMissing, places, readerSettings } from "./config";
import { startHistory } from "./history";
import { lockOnceFree, takeLock } from "./lock";
import { heldOwnerAccount } from "./owner";
import { startReader } from "./reader";
import { readerHandler } from "./routes";
import { serviceAt } from "./service";
import { ReaderStore } from "./store";

import type { TelegramClient } from "telegram";
import type { HeldLock } from "./lock";

// How often the reader asks again whether it may open the account.
const askEveryMs = 2_000;

const settings = readerSettings();
const where = places(settings.dataDir);
const peers = { ownerId: settings.ownerId, botId: settings.botId };
const service = serviceAt(settings.servicePort, settings.token);
let client: TelegramClient | null = null;
let missing: string | null = settings.account === null ? accountMissing : null;
let lock: HeldLock | null = null;
let stopping = false;

const server = Bun.serve({
  hostname: "127.0.0.1",
  port: settings.port,
  idleTimeout: 0,
  fetch: readerHandler({ token: settings.token, peers, owner: heldOwnerAccount(() => client, peers), client: () => client, missing: () => missing }),
});

console.log(`reader: listening on 127.0.0.1:${server.port}`);

async function open(): Promise<void> {
  const account = settings.account;

  if (account === null) {
    console.log(`reader: the owner's account stands inactive: ${accountMissing}`);

    return;
  }

  lock = await lockOnceFree({
    mayOpen: service.reachable,
    takeLock: async () => takeLock(where.lock),
    waits: () => !stopping,
    pause: async () => await Bun.sleep(askEveryMs),
    waiting: () => console.log("reader: waiting until the service answers and the session lock is free"),
  });

  if (lock === null) return;

  const store = new ReaderStore(where.store);

  try {
    client = await startReader({ account, peers, service });
    missing = null;
    console.log("reader: the owner's account is connected and reading other bots' messages");
    startHistory({ client, peers, service, store });
    startAvatarSweeper({ client, store, service, avatarDir: where.avatars, ownerId: settings.ownerId, botId: settings.botId });
  } catch (error) {
    missing = `the owner's account did not connect: ${error instanceof Error ? error.message : String(error)}`;
    console.error(`reader: ${missing}`);
  }
}

void open();

// The account let go and the lock released, within the stop systemd allows.
async function stop(): Promise<void> {
  if (stopping) return;

  stopping = true;
  await client?.disconnect().catch(() => undefined);
  await lock?.release();
  await server.stop(true);
  process.exit(0);
}

process.once("SIGTERM", () => void stop());
process.once("SIGINT", () => void stop());
