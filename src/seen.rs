//! The rooms and the senders the provider has handed a message of over or sent into (docs/boundary-api.md,
//! 「The service's routes, called by the reader」): whose profile pictures the reader keeps, and which rooms
//! are the provider's own when the receiver asks. Kept in the provider's data folder, so a restart forgets
//! none of them.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use bmo_chat::ChatMessage;
use serde::{Deserialize, Serialize};

/// What has been seen: every room and every sender, each once.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sighted {
  pub rooms: BTreeSet<i64>,
  pub people: BTreeSet<i64>,
}

/// The record, kept in a file where one is named.
#[derive(Debug, Default)]
pub struct Seen {
  file: Option<PathBuf>,
  sighted: Mutex<Sighted>,
}

impl Seen {
  /// A record kept only in memory.
  pub fn in_memory() -> Self {
    Self::default()
  }

  /// The record kept in `file`, read back from it where it stands.
  pub fn kept_in(file: PathBuf) -> Self {
    let sighted = std::fs::read(&file)
      .ok()
      .and_then(|bytes| serde_json::from_slice(&bytes).ok())
      .unwrap_or_default();

    Self {
      file: Some(file),
      sighted: Mutex::new(sighted),
    }
  }

  fn changed(&self, change: impl FnOnce(&mut Sighted) -> bool) {
    let mut sighted = self.sighted.lock().unwrap_or_else(PoisonError::into_inner);

    if !change(&mut sighted) {
      return;
    }

    if let Some(file) = &self.file {
      let written = serde_json::to_vec(&*sighted).map_err(std::io::Error::other).and_then(|bytes| {
        let temporary = file.with_extension("tmp");

        std::fs::write(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, file))
      });

      if let Err(why) = written {
        eprintln!("telegram: the record of rooms seen could not be written: {why}");
      }
    }
  }

  /// A message handed over: its room and its sender seen.
  pub fn message(&self, message: &ChatMessage) {
    self.changed(|sighted| {
      let room = sighted.rooms.insert(message.chat_id);
      let person = message.sender_id.is_some_and(|sender| sighted.people.insert(sender));

      room || person
    });
  }

  /// A room sent into.
  pub fn room(&self, chat_id: i64) {
    self.changed(|sighted| sighted.rooms.insert(chat_id));
  }

  /// Whether the room has been seen.
  pub fn has_room(&self, chat_id: i64) -> bool {
    self.sighted.lock().unwrap_or_else(PoisonError::into_inner).rooms.contains(&chat_id)
  }

  /// Everything seen so far.
  pub fn sighted(&self) -> Sighted {
    self.sighted.lock().unwrap_or_else(PoisonError::into_inner).clone()
  }
}
