import type * as Telegram from "telegram";
import type * as TelegramEvents from "telegram/events";
import type * as TelegramSessions from "telegram/sessions";
import type * as TelegramUploads from "telegram/client/uploads";

// The MTProto library the owner's own account is read through, loaded the first time something
// asks for it rather than when a module naming it is loaded. It builds its whole schema as it
// loads, which is most of what starting the reader costs, so a test that never reads the account
// never pays for it, and a process that does pays once.
//
// Its types are imported as types everywhere, which the compiler erases; a value is reached only
// through these four.

let library: typeof Telegram | null = null;
let events: typeof TelegramEvents | null = null;
let sessions: typeof TelegramSessions | null = null;
let uploads: typeof TelegramUploads | null = null;

export function mtproto(): typeof Telegram {
  return library ??= require("telegram") as typeof Telegram;
}

export function mtprotoEvents(): typeof TelegramEvents {
  return events ??= require("telegram/events") as typeof TelegramEvents;
}

export function mtprotoSessions(): typeof TelegramSessions {
  return sessions ??= require("telegram/sessions") as typeof TelegramSessions;
}

export function mtprotoUploads(): typeof TelegramUploads {
  return uploads ??= require("telegram/client/uploads") as typeof TelegramUploads;
}
