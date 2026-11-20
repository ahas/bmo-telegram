// The owner's account kept online while the reader holds it (docs/reader.md, 「Typing and presence」): Telegram
// pushes a person's typing and a change of status only to a session that stands online, and an online status
// lapses unless it is renewed.

import { mtproto } from "./mtproto";

import type { TelegramClient } from "telegram";
import type { Clock } from "./presence";

/** How often the status is renewed where Telegram's own configuration cannot be read. */
export const renewalFallbackMs = 60_000;

/** How long stopping waits for Telegram to take the account offline before it lets go regardless. */
export const offlineWaitMs = 5_000;

/** Says the account online (`offline` false) or offline. */
export type SayStatus = (offline: boolean) => Promise<void>;

/** The account standing online, until stopped. */
export interface Online {
  stop(): Promise<void>;
}

/** The account said online at once and again every `everyMs`, and offline once stopped. A say that fails is
 * logged, and the next one comes on time all the same. */
export function standOnline(say: SayStatus, everyMs: number, clock: Clock): Online {
  let callOff = (): void => undefined;
  const renew = (): void => {
    say(false).catch((error) => console.error("reader online: the account's status was not renewed:", error));
    callOff = clock.after(everyMs, renew);
  };

  renew();

  return {
    stop: async () => {
      callOff();

      const offline = say(true).catch((error) => console.error("reader online: the account was not said offline:", error));
      let giveUp = (): void => undefined;
      const waited = new Promise<void>((resolve) => {
        giveUp = clock.after(offlineWaitMs, resolve);
      });

      await Promise.race([offline, waited]);
      giveUp();
    },
  };
}

/** The account's status said on the connection the reader holds. */
export function accountStatus(client: TelegramClient): SayStatus {
  return async (offline) => {
    await client.invoke(new (mtproto().Api.account.UpdateStatus)({ offline }));
  };
}

/** How often Telegram asks a client to renew its online status, as its configuration says; the fallback where
 * the configuration cannot be read. */
export async function renewalOf(client: TelegramClient): Promise<number> {
  try {
    const config = await client.invoke(new (mtproto().Api.help.GetConfig)());

    return config.onlineUpdatePeriodMs > 0 ? config.onlineUpdatePeriodMs : renewalFallbackMs;
  } catch (error) {
    console.error("reader online: Telegram's configuration could not be read:", error);

    return renewalFallbackMs;
  }
}
