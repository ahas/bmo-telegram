// The rooms of the owner's account (docs/reader.md, 「The rooms of the account」): every chat in its dialog
// list, under the number the service gives the room and with the title the account shows for it, read again
// at most every thirty seconds.

import type { TelegramClient } from "telegram";
import type { Peers } from "./media";

/** One chat of the account as the service lists it: the room's number and the account's title for it. */
export interface AccountRoom {
  chatId: number;
  title: string | null;
}

/** One entry of the account's dialog list: its number, its title, and whether it is the account's own. */
export interface DialogEntry {
  id: number;
  title: string | null;
  self: boolean;
}

// How long a list read off the account answers before it is read again.
const freshForMs = 30_000;

// The account sees the owner's one-to-one with the bot as the chat with the bot, so that chat is listed under
// the owner's number; the account's own saved messages are no room of the provider's.
export function accountRooms(dialogs: readonly DialogEntry[], peers: Peers): AccountRoom[] {
  return dialogs
    .filter((dialog) => !dialog.self)
    .map((dialog) => ({
      chatId: dialog.id === peers.botId ? peers.ownerId : dialog.id,
      title: dialog.title === null || dialog.title === "" ? null : dialog.title,
    }));
}

// Every dialog of the account, the archived ones included and a basic group that became a supergroup left out.
async function dialogsOf(client: TelegramClient): Promise<DialogEntry[]> {
  const dialogs = await client.getDialogs({ ignoreMigrated: true });

  return dialogs.flatMap((dialog) => {
    if (dialog.id === undefined) return [];

    const entity = dialog.entity as { self?: boolean } | undefined;

    return [{ id: Number(dialog.id.toString()), title: dialog.title ?? null, self: entity?.self === true }];
  });
}

/** The account's rooms, read off the account, and `forget` to read them again on the next asking. */
export type RoomLister = ((client: TelegramClient) => Promise<AccountRoom[]>) & { forget: () => void };

/** The account's rooms, the list last read answering while it is fresh. `now` is the clock, for tests. */
export function roomLister(peers: Peers, now: () => number = Date.now): RoomLister {
  let last: { at: number; rooms: AccountRoom[] } | null = null;

  const list = async (client: TelegramClient): Promise<AccountRoom[]> => {
    if (last !== null && now() - last.at < freshForMs) return last.rooms;

    const rooms = accountRooms(await dialogsOf(client), peers);

    last = { at: now(), rooms };

    return rooms;
  };

  return Object.assign(list, {
    forget: () => {
      last = null;
    },
  });
}
