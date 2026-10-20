// The reader's own store, a SQLite file in the provider's data folder: how far the history walk of
// each room has come, and which profile picture is held for each room and person. Nothing else reads
// it; the files the pictures are kept in stand beside it, and the service serves them.

import { Database } from "bun:sqlite";

export type AvatarKind = "room" | "person";

// One subject's profile picture: which picture Telegram is showing for it, where the file is, and
// when it was taken, in epoch milliseconds. A subject Telegram shows no picture for keeps a row all
// the same, so nothing is asked for twice.
export interface AvatarRecord {
  kind: AvatarKind;
  subjectId: number;
  photoId: string | null;
  path: string | null;
  updatedAt: number;
}

// One kept picture as the service lists it.
export interface KeptAvatar {
  kind: AvatarKind;
  id: number;
  takenAt: number;
}

// How far one room's history walk came: the lowest number reached, and when the top was.
export interface HistoryProgress {
  reachedId: number;
  doneAt: number | null;
}

interface AvatarRow {
  kind: string;
  subject_id: number;
  photo_id: string | null;
  path: string | null;
  updated_at: number;
}

export class ReaderStore {
  private readonly db: Database;

  /** The store at `path`, or in memory for ":memory:", made where it does not stand yet. */
  constructor(path: string) {
    this.db = new Database(path, { create: true });
    this.db.run("PRAGMA journal_mode = WAL");
    this.db.run("CREATE TABLE IF NOT EXISTS history (chat_id INTEGER PRIMARY KEY, reached_id INTEGER NOT NULL, done_at INTEGER)");
    this.db.run(
      "CREATE TABLE IF NOT EXISTS avatars (kind TEXT NOT NULL, subject_id INTEGER NOT NULL, photo_id TEXT, path TEXT, updated_at INTEGER NOT NULL, PRIMARY KEY (kind, subject_id))",
    );
  }

  historyProgress(chatId: number): HistoryProgress | null {
    const row = this.db.query("SELECT reached_id, done_at FROM history WHERE chat_id = ?").get(chatId) as
      | { reached_id: number; done_at: number | null }
      | null;

    return row === null ? null : { reachedId: row.reached_id, doneAt: row.done_at };
  }

  /** The lowest number the walk of a room has reached: never higher than it already stood. */
  setHistoryReached(chatId: number, reachedId: number): void {
    this.db
      .query("INSERT INTO history (chat_id, reached_id) VALUES (?, ?) ON CONFLICT (chat_id) DO UPDATE SET reached_id = MIN(reached_id, excluded.reached_id)")
      .run(chatId, reachedId);
  }

  markHistoryDone(chatId: number, at: number = Math.floor(Date.now() / 1000)): void {
    this.db
      .query("INSERT INTO history (chat_id, reached_id, done_at) VALUES (?, 0, ?) ON CONFLICT (chat_id) DO UPDATE SET done_at = excluded.done_at")
      .run(chatId, at);
  }

  getAvatar(kind: AvatarKind, subjectId: number): AvatarRecord | null {
    const row = this.db.query("SELECT * FROM avatars WHERE kind = ? AND subject_id = ?").get(kind, subjectId) as AvatarRow | null;

    return row === null
      ? null
      : { kind: row.kind as AvatarKind, subjectId: row.subject_id, photoId: row.photo_id, path: row.path, updatedAt: row.updated_at };
  }

  /** Every picture with a file standing behind it, as the service lists them. */
  keptAvatars(): KeptAvatar[] {
    const rows = this.db
      .query("SELECT kind, subject_id, updated_at FROM avatars WHERE photo_id IS NOT NULL AND path IS NOT NULL")
      .all() as { kind: string; subject_id: number; updated_at: number }[];

    // A record written before times crossed in milliseconds holds seconds; it is read as the millisecond it
    // names.
    return rows.map((row) => ({
      kind: row.kind as AvatarKind,
      id: row.subject_id,
      takenAt: row.updated_at < 1_000_000_000_000 ? row.updated_at * 1000 : row.updated_at,
    }));
  }

  saveAvatar(record: AvatarRecord): void {
    this.db
      .query(
        "INSERT INTO avatars (kind, subject_id, photo_id, path, updated_at) VALUES (?, ?, ?, ?, ?) ON CONFLICT (kind, subject_id) DO UPDATE SET photo_id = excluded.photo_id, path = excluded.path, updated_at = excluded.updated_at",
      )
      .run(record.kind, record.subjectId, record.photoId, record.path, record.updatedAt);
  }

  close(): void {
    this.db.close();
  }
}
