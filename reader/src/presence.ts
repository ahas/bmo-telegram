// Who is writing in a room now and who is reachable now, as the owner's account hears them (docs/reader.md,
// 「Typing and presence」): a person's typing action, and the status of the people who stand in the rooms whose
// people the service says the reader tells of. The Bot API hands a bot neither.

import { chatIdOfPeer } from "./aftermath";
import { roomMembers } from "./avatars";
import { resolveEntity } from "./history";
import { mtproto } from "./mtproto";

import type { TelegramClient } from "telegram";
import type { Peers } from "./media";
import type { Rooms, Service } from "./service";

/** A person writing in a room now, or no longer. */
export interface TypingHeard {
  roomId: number;
  personId: number;
  on: boolean;
}

/** One person's presence as the chat protocol carries it. */
export interface PersonPresence {
  personId: number;
  state: "online" | "away" | "offline";
  lastSeenAt: number | null;
}

/** People's presence: one as it changes, or everyone told of as a whole list. */
export interface PresenceHeard {
  people: PersonPresence[];
  whole: boolean;
}

/** The time as the presence reads it, and a run after a while that can be called off. */
export interface Clock {
  now(): number;
  after(ms: number, run: () => void): () => void;
}

export const systemClock: Clock = {
  now: () => Date.now(),
  after: (ms, run) => {
    const timer = setTimeout(run, ms);

    return () => clearTimeout(timer);
  },
};

/** How long the service's word on whether a room tells is kept, so a person typing on does not ask it each time. */
export const tellsKeptMs = 30_000;

/** The whole list waits until the service has counted the account connected, then comes again this often. */
export const wholeDelayMs = 15_000;
export const wholeIntervalMs = 10 * 60 * 1000;

// The actions of writing a message; a cancel takes the mark off, and any other action is no mark.
const writing = new Set([
  "SendMessageTypingAction",
  "SendMessageRecordVideoAction",
  "SendMessageUploadVideoAction",
  "SendMessageRecordAudioAction",
  "SendMessageUploadAudioAction",
  "SendMessageUploadPhotoAction",
  "SendMessageUploadDocumentAction",
  "SendMessageGeoLocationAction",
  "SendMessageChooseContactAction",
  "SendMessageRecordRoundAction",
  "SendMessageUploadRoundAction",
  "SendMessageChooseStickerAction",
]);

/** A typing update as the account hears it, by the fields the three kinds carry. */
export interface TypingUpdate {
  className: string;
  userId?: unknown;
  chatId?: unknown;
  channelId?: unknown;
  fromId?: unknown;
  action: { className: string };
}

/** The mark a typing update tells, numbered as the service numbers rooms; null for no mark, for an update that is
 * none of the three kinds, and for the account's own typing and the provider's bot's. */
export function typingOf(update: TypingUpdate, peers: Peers): TypingHeard | null {
  const on = update.action.className === "SendMessageCancelAction" ? false : writing.has(update.action.className) ? true : null;

  if (on === null) {
    return null;
  }

  const room = typingRoom(update);

  if (room === null || room.personId === peers.ownerId || room.personId === peers.botId) {
    return null;
  }

  return { ...room, on };
}

function typingRoom(update: TypingUpdate): { roomId: number; personId: number } | null {
  const from = (): number | null => {
    const peer = update.fromId as { className: string; userId?: unknown } | undefined;

    return peer?.className === "PeerUser" ? chatIdOfPeer(peer) : null;
  };

  switch (update.className) {
    case "UpdateUserTyping": {
      const personId = Number(String(update.userId));

      return { roomId: personId, personId };
    }
    case "UpdateChatUserTyping": {
      const personId = from();

      return personId === null ? null : { roomId: chatIdOfPeer({ className: "PeerChat", chatId: update.chatId })!, personId };
    }
    case "UpdateChannelUserTyping": {
      const personId = from();

      return personId === null ? null : { roomId: chatIdOfPeer({ className: "PeerChannel", channelId: update.channelId })!, personId };
    }
    default:
      return null;
  }
}

/** A person's presence as a status reads at `now`, and the moment it lapses where it is online until one; null for
 * no status, as a bot has none. */
export function presenceOf(status: unknown, now: number): { state: "online" | "offline"; lastSeenAt: number | null; lapsesAt: number | null } | null {
  const read = status as { className?: string; expires?: number; wasOnline?: number } | undefined;

  switch (read?.className) {
    case "UserStatusOnline": {
      const lapsesAt = (read.expires ?? 0) * 1000;

      return lapsesAt > now ? { state: "online", lastSeenAt: null, lapsesAt } : { state: "offline", lastSeenAt: lapsesAt, lapsesAt: null };
    }
    case "UserStatusOffline":
      return { state: "offline", lastSeenAt: (read.wasOnline ?? 0) * 1000, lapsesAt: null };
    // A coarse word in place of a moment, under the person's own privacy setting, is no moment.
    case "UserStatusRecently":
    case "UserStatusLastWeek":
    case "UserStatusLastMonth":
    case "UserStatusEmpty":
      return { state: "offline", lastSeenAt: null, lapsesAt: null };
    default:
      return null;
  }
}

/** Whether a room's people are the reader's to tell of, as the service last said within `tellsKeptMs`. */
export class RoomTells {
  private readonly kept = new Map<number, { tells: boolean; at: number }>();

  constructor(
    private readonly ask: (chatId: number) => Promise<boolean>,
    private readonly clock: Clock,
  ) {}

  async of(chatId: number): Promise<boolean> {
    const kept = this.kept.get(chatId);

    if (kept !== undefined && this.clock.now() - kept.at < tellsKeptMs) {
      return kept.tells;
    }

    const tells = await this.ask(chatId);

    this.kept.set(chatId, { tells, at: this.clock.now() });

    return tells;
  }
}

/** The people told of and each one's presence handed over: a whole list, then each change as it comes, an online
 * status going offline by itself at the moment it lapses. */
export class PresenceTeller {
  private people = new Set<number>();
  private readonly lapses = new Map<number, () => void>();

  constructor(
    private readonly tell: (heard: PresenceHeard) => Promise<void>,
    private readonly clock: Clock,
  ) {}

  /** Everyone told of, each with the status they came with; one with none is left out of the list. */
  async whole(statuses: ReadonlyMap<number, unknown>): Promise<void> {
    for (const callOff of this.lapses.values()) {
      callOff();
    }

    this.lapses.clear();
    this.people = new Set(statuses.keys());

    const listed = [...statuses].flatMap(([personId, status]) => {
      const presence = this.read(personId, status);

      return presence === null ? [] : [presence];
    });

    await this.tell({ people: listed, whole: true });
  }

  /** One person's status changed; anyone not told of is passed over. */
  async changed(personId: number, status: unknown): Promise<void> {
    if (!this.people.has(personId)) {
      return;
    }

    this.lapses.get(personId)?.();
    this.lapses.delete(personId);

    const presence = this.read(personId, status);

    if (presence !== null) {
      await this.tell({ people: [presence], whole: false });
    }
  }

  private read(personId: number, status: unknown): PersonPresence | null {
    const presence = presenceOf(status, this.clock.now());

    if (presence === null) {
      return null;
    }

    const { lapsesAt } = presence;

    if (lapsesAt !== null) {
      this.lapses.set(personId, this.clock.after(lapsesAt - this.clock.now(), () => {
        this.lapses.delete(personId);
        this.tell({ people: [{ personId, state: "offline", lastSeenAt: lapsesAt }], whole: false })
          .catch((error) => console.error(`reader presence of ${personId} lapsing failed:`, error));
      }));
    }

    return { personId, state: presence.state, lastSeenAt: presence.lastSeenAt };
  }
}

/** A person as a room yields them: their number and the status they came with. */
export interface Standing {
  id: number;
  status: unknown;
}

/** Where the people told of are read from: the rooms the receiver names, whether each tells, the person of a
 * one-to-one and a group's or a channel's members. */
export interface PeopleSources {
  rooms(): Promise<Rooms>;
  tells(chatId: number): Promise<boolean>;
  personOf(chatId: number): Promise<Standing | null>;
  membersOf(chatId: number): Promise<Standing[]>;
}

/** Everyone who stands in a room whose people the reader tells of, each with their status, but the account itself
 * and the provider's bot; null where the receiver does not name its rooms. */
export async function gatherPeople(sources: PeopleSources, peers: Peers): Promise<Map<number, unknown> | null> {
  const named = await sources.rooms();

  if ("unanswered" in named) {
    console.error(`reader presence: no whole list this time: ${named.unanswered}`);

    return null;
  }

  const people = new Map<number, unknown>();

  for (const room of named.rooms) {
    // The owner's one-to-one with the bot holds nobody but the two.
    if (room.chatId === peers.ownerId || !(await sources.tells(room.chatId))) {
      continue;
    }

    const standing = room.chatId > 0 ? [await sources.personOf(room.chatId)] : await sources.membersOf(room.chatId);

    for (const person of standing) {
      if (person !== null && person.id !== peers.ownerId && person.id !== peers.botId) {
        people.set(person.id, person.status);
      }
    }
  }

  return people;
}

/** The sources of the people told of, read on the account. */
export function accountSources(client: TelegramClient, service: Service, tells: RoomTells): PeopleSources {
  return {
    rooms: () => service.rooms(),
    tells: (chatId) => tells.of(chatId),
    personOf: async (chatId) => {
      try {
        const entity = await resolveEntity(client, chatId);

        return entity instanceof mtproto().Api.User ? { id: chatId, status: entity.status } : null;
      } catch (error) {
        console.error(`reader presence: person ${chatId} could not be read:`, error);

        return null;
      }
    },
    membersOf: async (chatId) => {
      const members = await roomMembers({ client }, [{ kind: "room", subjectId: chatId, peer: chatId }]);

      return [...members].map(([id, user]) => ({ id, status: user.status }));
    },
  };
}

/** What the account hears of typing and statuses, handed to the service. */
export interface Hearing {
  update(update: { className: string }): Promise<void>;
}

/** Typing and presence heard on the connected account, the whole list told after `wholeDelayMs` and every
 * `wholeIntervalMs`. */
export function startHearing(deps: { client: TelegramClient; service: Service; peers: Peers }): Hearing {
  const tells = new RoomTells(async (chatId) => (await deps.service.listened(chatId)).tells, systemClock);
  const teller = new PresenceTeller((heard) => deps.service.presence(heard), systemClock);
  const sources = accountSources(deps.client, deps.service, tells);
  const whole = async (): Promise<void> => {
    try {
      const people = await gatherPeople(sources, deps.peers);

      if (people !== null) {
        await teller.whole(people);
      }
    } catch (error) {
      console.error("reader presence: the whole list failed:", error);
    }
  };

  setTimeout(() => void whole(), wholeDelayMs);
  setInterval(() => void whole(), wholeIntervalMs);

  return {
    update: async (update) => {
      if (update.className === "UpdateUserStatus") {
        const status = update as { userId?: unknown; status?: unknown };

        await teller.changed(Number(String(status.userId)), status.status);

        return;
      }

      if ("action" in update) {
        const heard = typingOf(update as TypingUpdate, deps.peers);

        if (heard !== null && (await tells.of(heard.roomId))) {
          await deps.service.typing(heard);
        }
      }
    },
  };
}
