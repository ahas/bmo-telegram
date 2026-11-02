// The messages the reader's own sends went out as (docs/reader.md, 「What it is for」): the owner's words in a
// room whose people are the reader's are handed over, but for these, which the sender holds already. Each is
// noted by its room and the number the account gave it, for a while and up to a bound per room; a message read
// while a send into its room is still going waits for that send to end before it is matched.

const keptForMs = 5 * 60_000;
const keptPerRoom = 200;

export interface OwnSendsOptions {
  now?: () => number;
}

export class OwnSends {
  // By room: each number the account gave a send, with when it was noted.
  private readonly noted = new Map<number, Map<number, number>>();
  // By room: the sends still going, each settled once its numbers are noted or it failed.
  private readonly going = new Map<number, Set<Promise<void>>>();
  private readonly now: () => number;

  constructor(options: OwnSendsOptions = {}) {
    this.now = options.now ?? Date.now;
  }

  /** A send into `chatId` carried out by `send`, the numbers it answers noted as the reader's own. */
  async sending<T>(chatId: number, send: () => Promise<T>, ids: (sent: T) => (number | null)[]): Promise<T> {
    let settle = () => {};
    const settled = new Promise<void>((resolve) => {
      settle = resolve;
    });
    const room = this.going.get(chatId) ?? new Set<Promise<void>>();

    room.add(settled);
    this.going.set(chatId, room);

    try {
      const sent = await send();

      this.note(chatId, ids(sent).filter((id): id is number => id !== null));

      return sent;
    } finally {
      room.delete(settled);

      if (room.size === 0) {
        this.going.delete(chatId);
      }

      settle();
    }
  }

  /** The numbers a send into `chatId` went out as. */
  note(chatId: number, ids: readonly number[]): void {
    const room = this.noted.get(chatId) ?? new Map<number, number>();
    const at = this.now();

    for (const id of ids) {
      room.set(id, at);
    }

    for (const [id, notedAt] of room) {
      if (room.size <= keptPerRoom && at - notedAt <= keptForMs) break;

      room.delete(id);
    }

    this.noted.set(chatId, room);
  }

  /** Whether message `id` of `chatId` is one the reader's own sends went out as, once the sends still going
   * into the room have settled, gone out or failed; a match is let go. */
  async isOwn(chatId: number, id: number): Promise<boolean> {
    const going = this.going.get(chatId);

    if (going !== undefined && going.size > 0) {
      await Promise.all(going);
    }

    const room = this.noted.get(chatId);
    const notedAt = room?.get(id);

    if (room === undefined || notedAt === undefined) {
      return false;
    }

    room.delete(id);

    return this.now() - notedAt <= keptForMs;
  }
}
