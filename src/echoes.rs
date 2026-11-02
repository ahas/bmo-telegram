//! The messages a send in the owner's voice went out as (docs/telegram.md, 「The owner's voice」), noted so the
//! bot account's sight of them — the owner's own message, seen from the bot's side of the room — is known as
//! that send's echo and not handed over again. Kept in memory: an echo comes within moments of its send.

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

// One message a send in the owner's voice went out as.
struct Note {
  room: i64,
  id: Option<i64>,
  words: String,
  at: Instant,
}

impl Note {
  // Whether a message the bot account read is this one: by its number where the room numbers messages for
  // everyone, by its words where each account numbers its own.
  fn answers(&self, room: i64, id: i64, words: &str) -> bool {
    if self.room != room {
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
  sent: VecDeque<Note>,
}

impl Notes {
  fn forget_stale(&mut self) {
    let now = Instant::now();

    while self.sent.front().is_some_and(|note| now.duration_since(note.at) > KEPT_FOR) {
      self.sent.pop_front();
    }
  }
}

/// The notes of what the owner's sends went out as, and of the ones still going.
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

  /// One message a send in the owner's voice went out as: its room, the number the owner's account gave it
  /// where it gave one, and the words Telegram holds for it.
  pub fn sent(&self, room: i64, id: Option<i64>, words: &str) {
    {
      let mut notes = self.notes.lock().unwrap_or_else(PoisonError::into_inner);

      notes.forget_stale();
      notes.sent.push_back(Note {
        room,
        id,
        words: words.to_owned(),
        at: Instant::now(),
      });

      while notes.sent.len() > MOST {
        notes.sent.pop_front();
      }
    }

    self.settled.notify_waiters();
  }

  /// Whether a message the bot account read from the owner is the echo of a send in the owner's voice: the
  /// note it answers is let go. While such a send into its room is still going, it waits for that send to end,
  /// gone out or failed.
  pub async fn echo(&self, room: i64, id: i64, words: &str) -> bool {
    loop {
      let mut settled = std::pin::pin!(self.settled.notified());

      settled.as_mut().enable();

      {
        let mut notes = self.notes.lock().unwrap_or_else(PoisonError::into_inner);

        notes.forget_stale();

        if let Some(at) = notes.sent.iter().position(|note| note.answers(room, id, words)) {
          notes.sent.remove(at);
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
