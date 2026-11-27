//! The rooms of a supervisor's bot's one-to-ones (docs/telegram.md, 「A bot for each supervisor」): Telegram
//! numbers a person's private chat with every bot by that person, so a person's one-to-one with the service's bot
//! and with a supervisor's bot share one number. Each one-to-one with a supervisor's bot is given a room of its own
//! here, numbered in a range Telegram numbers no chat in, and kept in the provider's data folder, one record a
//! room, so a restart gives every one-to-one the room it had.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// Where the rooms of supervisors' one-to-ones are numbered from, counting down: below every number Telegram gives
/// a chat, a supergroup's and a channel's included, and above the console's own rooms.
pub const FIRST_ROOM: i64 = -8_100_000_000_000;

/// One one-to-one with a supervisor's bot: its room, the supervisor whose bot it is, and the person, as Telegram
/// numbers the chat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DmRoom {
  pub room: i64,
  pub supervisor: String,
  pub person: i64,
}

#[derive(Default)]
struct Kept {
  by_room: HashMap<i64, DmRoom>,
  by_chat: HashMap<(String, i64), i64>,
}

/// The rooms of supervisors' one-to-ones, in memory or kept in a file.
#[derive(Default)]
pub struct DmRooms {
  kept: Mutex<Kept>,
  file: Option<PathBuf>,
}

impl DmRooms {
  /// The rooms kept in `file`, read back from it where it stands; a record a stop cut short is left out.
  pub fn kept_in(file: PathBuf) -> Self {
    let mut kept = Kept::default();

    if let Ok(text) = std::fs::read_to_string(&file) {
      for line in text.lines() {
        if let Ok(room) = serde_json::from_str::<DmRoom>(line) {
          kept.by_chat.insert((room.supervisor.clone(), room.person), room.room);
          kept.by_room.insert(room.room, room);
        }
      }
    }

    Self {
      kept: Mutex::new(kept),
      file: Some(file),
    }
  }

  /// The room of a person's one-to-one with a supervisor's bot, given one the first time it is asked.
  pub fn room_of(&self, supervisor: &str, person: i64) -> i64 {
    let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);

    if let Some(room) = kept.by_chat.get(&(supervisor.to_owned(), person)) {
      return *room;
    }

    let room = kept.by_room.keys().min().map_or(FIRST_ROOM, |lowest| (*lowest).min(FIRST_ROOM + 1) - 1);
    let made = DmRoom {
      room,
      supervisor: supervisor.to_owned(),
      person,
    };

    if let Some(file) = &self.file {
      let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)
        .and_then(|mut opened| writeln!(opened, "{}", serde_json::to_string(&made).unwrap_or_default()));

      if let Err(why) = written {
        eprintln!("telegram: the room of a one-to-one with {supervisor}'s bot was not kept: {why}");
      }
    }

    kept.by_chat.insert((supervisor.to_owned(), person), room);
    kept.by_room.insert(room, made);
    room
  }

  /// The one-to-one a room stands for, none where it is no supervisor's one-to-one.
  pub fn of_room(&self, room: i64) -> Option<DmRoom> {
    self.kept.lock().unwrap_or_else(PoisonError::into_inner).by_room.get(&room).cloned()
  }
}
