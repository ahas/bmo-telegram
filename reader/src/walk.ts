// Walks one room's history by hand: `bun run history --room <chat> [--max <messages>]`. The session must
// not be in use elsewhere, so this runs only while the reader's own process is stopped; the session lock
// refuses it otherwise.

import { accountMissing, places, readerSettings } from "./config";
import { walkNamedRoom } from "./history";
import { takeLock } from "./lock";
import { connectReader } from "./reader";
import { serviceAt } from "./service";
import { ReaderStore } from "./store";

function argValue(name: string): string | null {
  const index = process.argv.indexOf(name);
  const value = process.argv[index + 1];

  return index === -1 || value === undefined ? null : value;
}

const chatId = Number.parseInt(argValue("--room") ?? "", 10);
const maxArg = argValue("--max");
const maxMessages = maxArg === null ? undefined : Number.parseInt(maxArg, 10);

if (Number.isNaN(chatId)) {
  console.error("usage: bun run history --room <chat> [--max <messages>]  (the reader stopped)");
  process.exit(1);
}

const settings = readerSettings();

if (settings.account === null) {
  console.error(`the owner's account is not configured (${accountMissing})`);
  process.exit(1);
}

const where = places(settings.dataDir);
const lock = takeLock(where.lock);

if (lock === null) {
  console.error("the reader holds the session; stop it before walking a room by hand");
  process.exit(1);
}

const client = await connectReader(settings.account);
const store = new ReaderStore(where.store);

try {
  const peers = { ownerId: settings.ownerId, botId: settings.botId };
  const outcome = await walkNamedRoom({ client, peers, service: serviceAt(settings.servicePort, settings.token), store }, chatId, { maxMessages });

  console.log(`handed over ${outcome.handed} messages, ${outcome.kept} kept; ${outcome.reachedTop ? "reached the top" : "stopped early, resume later"}`);
} finally {
  await client.disconnect();
  store.close();
  await lock.release();
}
