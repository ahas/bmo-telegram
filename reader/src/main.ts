// The reader's process (docs/reader.md): it listens on the loopback for the service, waits until the
// service answers and the session lock is its own, then connects the owner's account and starts the live
// reading, the history walk and the profile pictures on the one connection. Without the account's three
// values it opens nothing and its health says why.

import { startAvatarSweeper } from "./avatars";
import { accountMissing, places, readerSettings } from "./config";
import { contactLister, makeGroup, startChat } from "./contacts";
import { roomLister } from "./dialogs";
import { startHistory } from "./history";
import { lockOnceFree, takeLock } from "./lock";
import { chatMembers } from "./members";
import { recentMessages } from "./messages";
import { accountStatus, renewalOf, standOnline } from "./online";
import { heldOwnerAccount } from "./owner";
import { systemClock } from "./presence";
import { startReader } from "./reader";
import { readerHandler } from "./routes";
import { OwnSends } from "./sends";
import { serviceAt } from "./service";
import { ReaderStore } from "./store";

import type { TelegramClient } from "telegram";
import type { HeldLock } from "./lock";
import type { Online } from "./online";

// How often the reader asks again whether it may open the account.
const askEveryMs = 2_000;

const settings = readerSettings();
const where = places(settings.dataDir);
const peers = { ownerId: settings.ownerId, botId: settings.botId };
const service = serviceAt(settings.servicePort, settings.token);
const rooms = roomLister(peers);
const contacts = contactLister(peers, rooms);
const sends = new OwnSends();
let client: TelegramClient | null = null;
let store: ReaderStore | null = null;
let missing: string | null = settings.account === null ? accountMissing : null;
let lock: HeldLock | null = null;
let online: Online | null = null;
let stopping = false;

const server = Bun.serve({
  hostname: "127.0.0.1",
  port: settings.port,
  idleTimeout: 0,
  fetch: readerHandler({
    token: settings.token,
    peers,
    owner: heldOwnerAccount(() => client, peers, sends),
    client: () => client,
    rooms,
    contacts,
    start: (account, asked) =>
      startChat(asked, { contacts: () => contacts(account), makeGroup: (title, ids) => makeGroup(account, title, ids), forgetRooms: rooms.forget }, peers),
    avatars: () => store?.keptAvatars() ?? [],
    messages: (account, asked) => recentMessages(account, peers, asked),
    members: (account, chatId) => chatMembers(account, peers, chatId),
    missing: () => missing,
  }),
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

  store = new ReaderStore(where.store);

  try {
    client = await startReader({ account, peers, service, sends });
    missing = null;
    console.log("reader: the owner's account is connected and reading other bots' messages");
    startHistory({ client, peers, service, store });
    startAvatarSweeper({ client, store, service, avatarDir: where.avatars, ownerId: settings.ownerId, botId: settings.botId });

    // Telegram pushes typing and a change of status only to a session that stands online (docs/reader.md,
    // 「Typing and presence」).
    if (settings.online) {
      const renewalMs = await renewalOf(client);

      online = standOnline(accountStatus(client), renewalMs, systemClock);
      console.log(`reader: the owner's account stands online, renewed every ${renewalMs / 1000} s`);
    }
  } catch (error) {
    missing = `the owner's account did not connect: ${error instanceof Error ? error.message : String(error)}`;
    console.error(`reader: ${missing}`);
  }
}

void open();

// The account let go and the lock released, then the process ends.
async function stop(): Promise<void> {
  if (stopping) return;

  stopping = true;
  await online?.stop();
  await client?.disconnect().catch(() => undefined);
  await lock?.release();
  await server.stop(true);
  process.exit(0);
}

process.once("SIGTERM", () => void stop());
process.once("SIGINT", () => void stop());
