//! The rooms and the senders the provider has handed a message of over or sent into (docs/boundary-api.md,
//! 「The service's routes, called by the reader」): whose profile pictures the reader keeps, and which rooms
//! are the provider's own when the receiver asks; each room's own senders, whom the bot account checks as the
//! room's members (docs/telegram.md, 「The members of a room」). With them, the people who wrote to the bot in
//! their private chat with it, whose one-to-one is the bot's (docs/telegram.md, 「The rooms」). Kept in the
//! provider's data folder, so a restart forgets none of them.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// What has been seen: every room and every sender, each once, each room's senders, and everyone who wrote to the
/// bot privately.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sighted {
  pub rooms: BTreeSet<i64>,
  pub people: BTreeSet<i64>,
  #[serde(default)]
  pub wrote_privately: BTreeSet<i64>,
  #[serde(default)]
  pub senders: BTreeMap<i64, BTreeSet<i64>>,
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

  /// A message handed over: its room and its sender seen, the sender as one of the room's.
  pub fn message(&self, room_id: i64, sender_id: Option<i64>) {
    self.changed(|sighted| {
      let room = sighted.rooms.insert(room_id);
      let person = sender_id.is_some_and(|sender| sighted.people.insert(sender));
      let speaker =
        sender_id.is_some_and(|sender| sighted.senders.entry(room_id).or_default().insert(sender));

      room || person || speaker
    });
  }

  /// People seen speaking in a room, kept as the room's senders alone.
  pub fn spoke(&self, room_id: i64, senders: impl IntoIterator<Item = i64>) {
    self.changed(|sighted| {
      let speakers = sighted.senders.entry(room_id).or_default();

      // Every sender is kept, so none is passed over once one is found new.
      senders.into_iter().filter(|&sender| speakers.insert(sender)).count() > 0
    });
  }

  /// A room sent into.
  pub fn room(&self, chat_id: i64) {
    self.changed(|sighted| sighted.rooms.insert(chat_id));
  }

  /// A person who wrote to the bot in their private chat with it, as the bot account read the message.
  pub fn wrote_privately(&self, person_id: i64) {
    self.changed(|sighted| sighted.wrote_privately.insert(person_id));
  }

  /// Whether a person has written to the bot in their private chat with it.
  pub fn has_written_privately(&self, person_id: i64) -> bool {
    self
      .sighted
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .wrote_privately
      .contains(&person_id)
  }

  /// Whether the room has been seen.
  pub fn has_room(&self, chat_id: i64) -> bool {
    self.sighted.lock().unwrap_or_else(PoisonError::into_inner).rooms.contains(&chat_id)
  }

  /// Everyone seen speaking in a room.
  pub fn senders_of(&self, room_id: i64) -> BTreeSet<i64> {
    let sighted = self.sighted.lock().unwrap_or_else(PoisonError::into_inner);

    sighted.senders.get(&room_id).cloned().unwrap_or_default()
  }

  /// Everything seen so far.
  pub fn sighted(&self) -> Sighted {
    self.sighted.lock().unwrap_or_else(PoisonError::into_inner).clone()
  }
}
