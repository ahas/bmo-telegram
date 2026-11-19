//! What a send in the owner's voice made (docs/telegram.md, 「The owner's voice」): the messages it went out as,
//! the edit it made and the service message of the pin it made, noted so the bot account's sight of each — the
//! owner's own doing, seen from the bot's side of the room — is known as that send's echo and not handed over
//! again. Kept in memory: an echo comes within moments of its send.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::Notify;
use tokio::time::Instant;

/// How long a note waits for its echo.
pub const KEPT_FOR: Duration = Duration::from_secs(300);

// The most notes kept at once; the oldest goes first.
const MOST: usize = 512;

/// Whether a room numbers its messages for everyone, a supergroup or a channel, so the number the owner's
/// account gave a message is the one the bot account reads it by.
pub fn numbered_for_everyone(chat_id: i64) -> bool {
  chat_id.to_string().starts_with("-100")
}

/// What the owner's account did that the bot account sees: a message sent, a line edited, or a message pinned,
/// which the bot account sees as the service message Telegram writes for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnerAct {
  Said,
  Edited,
  Pinned,
}

// One thing a send in the owner's voice made.
struct Note {
  act: OwnerAct,
  room: i64,
  // The number the send answered it under: the account's own where the room numbers for everyone, the
  // provider's own elsewhere — where it is kept for telling, never for matching.
  id: Option<i64>,
  words: String,
  at: Instant,
}

impl Note {
  // Whether what the bot account read is this one: the same act in the same room, by its number where the room
  // numbers messages for everyone, by its words where each account numbers its own and the bot account knows
  // the line by a number the send did not answer.
  fn answers(&self, act: OwnerAct, room: i64, id: i64, words: &str) -> bool {
    if self.act != act || self.room != room {
      return false;
    }

    match self.id {
      Some(noted) if numbered_for_everyone(room) => noted == id,
      _ => self.words == words,
    }
  }
}

#[derive(Default)]
struct Notes {
  // The sends in the owner's voice still going, counted by room.
  going: HashMap<i64, usize>,
  // What the sends made, oldest first.
  made: VecDeque<Note>,
}

impl Notes {
  fn forget_stale(&mut self) {
    let now = Instant::now();

    while self.made.front().is_some_and(|note| now.duration_since(note.at) > KEPT_FOR) {
      self.made.pop_front();
    }
  }
}

/// The notes of what the owner's sends made, and of the ones still going.
#[derive(Default)]
pub struct Echoes {
  notes: Mutex<Notes>,
  settled: Notify,
}

/// A send in the owner's voice going into a room, for as long as it is held.
pub struct Going {
  echoes: Arc<Echoes>,
  room: i64,
}

impl Drop for Going {
  fn drop(&mut self) {
    {
      let mut notes = self.echoes.notes.lock().unwrap_or_else(PoisonError::into_inner);

      if let Some(count) = notes.going.get_mut(&self.room) {
        *count -= 1;

        if *count == 0 {
          notes.going.remove(&self.room);
        }
      }
    }

    self.echoes.settled.notify_waiters();
  }
}

impl Echoes {
  /// A send in the owner's voice into `room` begins; it is going until what this answers is dropped.
  pub fn going(self: &Arc<Self>, room: i64) -> Going {
    *self.notes.lock().unwrap_or_else(PoisonError::into_inner).going.entry(room).or_default() += 1;

    Going {
      echoes: self.clone(),
      room,
    }
  }

  /// One thing a send in the owner's voice made: what it is, its room, the number the owner's account knows it
  /// by where it named one, and the words Telegram holds for it.
  pub fn noted(&self, act: OwnerAct, room: i64, id: Option<i64>, words: &str) {
    {
      let mut notes = self.notes.lock().unwrap_or_else(PoisonError::into_inner);

      notes.forget_stale();
      notes.made.push_back(Note {
        act,
        room,
        id,
        words: words.to_owned(),
        at: Instant::now(),
      });

      while notes.made.len() > MOST {
        notes.made.pop_front();
      }
    }

    self.settled.notify_waiters();
  }

  /// Whether what the bot account read from the owner, the `act` it shows, is the echo of a send in the owner's
  /// voice: the note it answers is let go. While such a send into its room is still going, it waits for that
  /// send to end, gone out or failed.
  pub async fn echo(&self, act: OwnerAct, room: i64, id: i64, words: &str) -> bool {
    loop {
      let mut settled = std::pin::pin!(self.settled.notified());

      settled.as_mut().enable();

      {
        let mut notes = self.notes.lock().unwrap_or_else(PoisonError::into_inner);

        notes.forget_stale();

        if let Some(at) = notes.made.iter().position(|note| note.answers(act, room, id, words)) {
          notes.made.remove(at);
          return true;
        }

        if !notes.going.contains_key(&room) {
          return false;
        }
      }

      settled.await;
    }
  }
}
