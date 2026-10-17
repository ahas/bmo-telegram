// The reader's configuration, read from the environment the deployment's env file supplies
// (docs/boundary-api.md, 「Configuration」). The owner's account is this process's alone; the rest
// it shares with the service: the token both sides of the seam take, the two loopback ports, the
// provider's data folder, and the numbers of the owner and of the bot account.

import { join } from "node:path";

// What the owner's account is reached with.
export interface AccountConfig {
  apiId: number;
  apiHash: string;
  session: string;
}

export interface ReaderSettings {
  // The owner's account, or null where any of its three values is missing.
  account: AccountConfig | null;
  // The owner's Telegram number, which is also the number of the owner's one-to-one with the bot.
  ownerId: number;
  // The bot account's number, read off its token; null where no token is set.
  botId: number | null;
  // The bearer every route of the service and of the reader takes.
  token: string;
  // The loopback ports the service and the reader listen on.
  servicePort: number;
  port: number;
  // The provider's own data folder.
  dataDir: string;
}

export const accountMissing = "BMO_TG_API_ID, BMO_TG_API_HASH, BMO_TG_SESSION not all set";

const defaultServicePort = 3220;
const defaultReaderPort = 3221;

type Environment = Record<string, string | undefined>;

function given(env: Environment, name: string): string | null {
  const value = env[name]?.trim();

  return value === undefined || value === "" ? null : value;
}

function required(env: Environment, name: string): string {
  const value = given(env, name);

  if (value === null) {
    throw new Error(`${name} is not set`);
  }

  return value;
}

function integer(name: string, value: string): number {
  const parsed = Number.parseInt(value, 10);

  if (Number.isNaN(parsed)) {
    throw new Error(`${name} must be an integer`);
  }

  return parsed;
}

function port(env: Environment, name: string, otherwise: number): number {
  const value = given(env, name);

  return value === null ? otherwise : integer(name, value);
}

/** The owner's account, or null where any of its three values is missing. */
export function accountConfig(env: Environment = process.env): AccountConfig | null {
  const apiId = given(env, "BMO_TG_API_ID");
  const apiHash = given(env, "BMO_TG_API_HASH");
  const session = given(env, "BMO_TG_SESSION");

  if (apiId === null || apiHash === null || session === null) {
    return null;
  }

  return { apiId: integer("BMO_TG_API_ID", apiId), apiHash, session };
}

/** The bot account's number: the part of its token before the colon. */
export function botIdOf(token: string | null): number | null {
  const id = Number.parseInt(token?.split(":")[0] ?? "", 10);

  return Number.isNaN(id) ? null : id;
}

/** The whole configuration, refused where a required value is missing. */
export function readerSettings(env: Environment = process.env): ReaderSettings {
  return {
    account: accountConfig(env),
    ownerId: integer("BMO_OWNER_ID", required(env, "BMO_OWNER_ID")),
    botId: botIdOf(given(env, "BMO_BOT_TOKEN")),
    token: required(env, "BMO_TELEGRAM_TOKEN"),
    servicePort: port(env, "BMO_TELEGRAM_PORT", defaultServicePort),
    port: port(env, "BMO_TELEGRAM_READER_PORT", defaultReaderPort),
    dataDir: required(env, "BMO_TELEGRAM_DATA"),
  };
}

/** Where the reader's own store, its profile pictures and its session lock stand in the data folder. */
export function places(dataDir: string): { store: string; avatars: string; lock: string } {
  return {
    store: join(dataDir, "reader.db"),
    avatars: join(dataDir, "avatars"),
    lock: join(dataDir, "session.lock"),
  };
}
