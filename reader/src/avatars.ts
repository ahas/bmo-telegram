// Profile pictures (docs/reader.md, 「Profile pictures」): the picture of every room and every person the
// provider has seen, kept current from the owner's account in the provider's data folder, its record in
// the reader's own store. The service serves the files; word that some changed goes to it once a pass.

import { randomUUID } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, rename, rm } from "node:fs/promises";
import { dirname, join } from "node:path";

import { resolveEntity } from "./history";
import { mtproto } from "./mtproto";

import type { Api, TelegramClient } from "telegram";
import type { Entity } from "telegram/define";
import type { Service } from "./service";
import type { AvatarKind, AvatarRecord, ReaderStore } from "./store";

export interface AvatarDeps {
  client: TelegramClient;
  store: ReaderStore;
  service: Service;
  // Where the picture files are kept.
  avatarDir: string;
  ownerId: number;
  botId: number | null;
}

// One picture to take: whose it is on screen, and which Telegram peer actually holds it. The two part
// company in the owner's one-to-one with the bot, which wears the bot's picture, not the owner's.
export interface AvatarSubject {
  kind: AvatarKind;
  subjectId: number;
  peer: number;
}

// What became of one subject in a pass, and the tally of a whole pass.
export type AvatarOutcome = "taken" | "unchanged" | "none" | "failed";

export type AvatarSweep = Record<AvatarOutcome, number>;

// The account has just connected and the history walk is starting; the pictures can wait until the
// opening rush is over. After that a picture changes rarely enough that a few hours between looks is
// often enough.
export const avatarSweepDelayMs = 60_000;
export const avatarSweepIntervalMs = 6 * 60 * 60 * 1000;

export function avatarFilePath(avatarDir: string, kind: AvatarKind, subjectId: number): string {
  return join(avatarDir, kind, `${subjectId}.jpg`);
}

// Every room and every person the provider has seen, each named once. The bot stands among the people
// whether it has spoken lately or not, since its own face is on every answer.
export function avatarSubjects(input: {
  chatIds: readonly number[];
  senderIds: readonly number[];
  ownerId: number;
  // The bot account's number. Null where it cannot be read — then its own face is not among the people,
  // and the owner's one-to-one is asked about by its own number.
  botId: number | null;
}): AvatarSubject[] {
  const rooms = [...new Set(input.chatIds)]
    .sort((left, right) => left - right)
    .map((chatId) => ({
      kind: "room" as const,
      subjectId: chatId,
      peer: chatId === input.ownerId ? (input.botId ?? chatId) : chatId,
    }));

  const people = [...new Set([...input.senderIds, ...(input.botId === null ? [] : [input.botId])])]
    .sort((left, right) => left - right)
    .map((userId) => ({ kind: "person" as const, subjectId: userId, peer: userId }));

  return [...rooms, ...people];
}

// Telegram numbers a profile picture, and the number changes when the picture does. That is what tells a
// picture already held from one worth downloading; an account showing none has no number at all.
export function photoIdOf(entity: unknown): string | null {
  if (entity instanceof mtproto().Api.User) {
    return entity.photo instanceof mtproto().Api.UserProfilePhoto ? entity.photo.photoId.toString() : null;
  }

  if (entity instanceof mtproto().Api.Channel || entity instanceof mtproto().Api.Chat) {
    return entity.photo instanceof mtproto().Api.ChatPhoto ? entity.photo.photoId.toString() : null;
  }

  return null;
}

// Whether the picture Telegram is showing has to be fetched: when it is new, when it is not the one held,
// or when the file the record points at is gone.
export function needsFetch(held: AvatarRecord | null, photoId: string, hasFile: boolean): boolean {
  return held === null || held.photoId !== photoId || held.path === null || !hasFile;
}

async function writeAvatar(path: string, bytes: Uint8Array): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  const temporary = `${path}.${randomUUID()}.tmp`;
  try {
    await Bun.write(temporary, bytes);
    await rename(temporary, path);
  } finally {
    await rm(temporary, { force: true });
  }
}

// Someone who has only ever spoken inside a room is in none of the account's own conversations, and
// Telegram will not hand them over by their number alone. The room knows them, so a pass reads each room's
// members once and takes them from there.
const memberLimit = 500;

export async function roomMembers(deps: Pick<AvatarDeps, "client">, rooms: readonly AvatarSubject[]): Promise<Map<number, Api.User>> {
  const members = new Map<number, Api.User>();

  for (const room of rooms) {
    try {
      for (const member of await deps.client.getParticipants(room.peer, { limit: memberLimit })) {
        if (member instanceof mtproto().Api.User) {
          members.set(Number(member.id.toString()), member);
        }
      }
    } catch (error) {
      console.error(`members of room ${room.subjectId} could not be read:`, error);
    }
  }

  return members;
}

/**
 * Takes one subject's picture if it is not the one already held. A subject showing none loses the file it
 * had, so a picture taken down goes off the screen with it. `changed` hears of every subject whose picture
 * or record changed.
 */
export async function refreshAvatar(
  deps: AvatarDeps,
  subject: AvatarSubject,
  known: Entity | null = null,
  changed: (subject: AvatarSubject) => void = () => undefined,
): Promise<AvatarOutcome> {
  const { client, store } = deps;
  const path = avatarFilePath(deps.avatarDir, subject.kind, subject.subjectId);

  try {
    const entity = known ?? (await resolveEntity(client, subject.peer));
    const photoId = photoIdOf(entity);
    const held = store.getAvatar(subject.kind, subject.subjectId);
    const updatedAt = Math.max(Date.now(), (held?.updatedAt ?? 0) + 1);

    if (photoId === null) {
      if (held?.photoId === null && held.path === null) return "none";
      await rm(path, { force: true });
      store.saveAvatar({ kind: subject.kind, subjectId: subject.subjectId, photoId: null, path: null, updatedAt });
      changed(subject);

      return "none";
    }

    if (!needsFetch(held, photoId, existsSync(path))) {
      // The data folder moved but the photo did not: a file standing where it now belongs repairs its record.
      if (held !== null && held.path !== path) {
        store.saveAvatar({ ...held, path, updatedAt });
        changed(subject);
      }

      return "unchanged";
    }

    const data = await client.downloadProfilePhoto(entity, { isBig: true });

    if (!(data instanceof Uint8Array) || data.byteLength === 0) {
      throw new Error("downloadProfilePhoto returned no data");
    }

    await writeAvatar(path, new Uint8Array(data));
    store.saveAvatar({ kind: subject.kind, subjectId: subject.subjectId, photoId, path, updatedAt });
    changed(subject);

    return "taken";
  } catch (error) {
    console.error(`avatar for ${subject.kind} ${subject.subjectId} failed:`, error);

    return "failed";
  }
}

/** One pass over everyone the provider has seen; a subject the account cannot resolve fails on its own and the pass carries on. */
export async function sweepAvatars(deps: AvatarDeps): Promise<AvatarSweep> {
  const seen = await deps.service.seen();
  const subjects = avatarSubjects({ chatIds: seen.rooms, senderIds: seen.people, ownerId: deps.ownerId, botId: deps.botId });
  const sweep: AvatarSweep = { taken: 0, unchanged: 0, none: 0, failed: 0 };
  const members = await roomMembers(deps, subjects.filter((subject) => subject.kind === "room"));
  const rooms: number[] = [];
  const people: number[] = [];
  const changed = (subject: AvatarSubject): void => {
    (subject.kind === "room" ? rooms : people).push(subject.subjectId);
  };

  for (const subject of subjects) {
    sweep[await refreshAvatar(deps, subject, members.get(subject.peer) ?? null, changed)] += 1;
  }

  if (rooms.length > 0 || people.length > 0) {
    await deps.service.avatarsChanged(rooms, people);
  }

  return sweep;
}

// The pictures are taken on this timer and nowhere else, and a sweep that is still going is never started
// a second time on top of itself.
export function startAvatarSweeper(deps: AvatarDeps): void {
  let running = false;

  const run = async (): Promise<void> => {
    if (running) {
      return;
    }

    running = true;

    try {
      const sweep = await sweepAvatars(deps);

      console.log(`avatars swept: ${sweep.taken} taken, ${sweep.unchanged} unchanged, ${sweep.none} without one, ${sweep.failed} failed`);
    } catch (error) {
      console.error("avatar sweep failed:", error);
    } finally {
      running = false;
    }
  };

  setTimeout(() => void run(), avatarSweepDelayMs);
  setInterval(() => void run(), avatarSweepIntervalMs);
}
