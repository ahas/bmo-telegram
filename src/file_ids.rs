//! The id each picture and file the bot account handed over crossed with (docs/telegram.md, 「Reading a
//! room」), by its message, so the same file fetched again through the reader crosses under the same id. Kept
//! in the provider's data folder, one record to a line added as each is handed over, so a restart forgets
//! none of them; a later record of a message stands over an earlier one.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// One message's picture or file, and the id it crossed with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Crossed {
  chat_id: i64,
  message_id: i64,
  id: String,
}

/// The record, kept in a file where one is named.
#[derive(Debug, Default)]
pub struct FileIds {
  file: Option<PathBuf>,
  ids: Mutex<HashMap<(i64, i64), String>>,
}

impl FileIds {
  /// A record kept only in memory.
  pub fn in_memory() -> Self {
    Self::default()
  }

  /// The record kept in `file`, read back from it where it stands. A record cut short by a stop is left out.
  pub fn kept_in(file: PathBuf) -> Self {
    let bytes = std::fs::read(&file).unwrap_or_default();
    let ids = serde_json::Deserializer::from_slice(&bytes)
      .into_iter::<Crossed>()
      .map_while(Result::ok)
      .map(|crossed| ((crossed.chat_id, crossed.message_id), crossed.id))
      .collect();

    Self {
      file: Some(file),
      ids: Mutex::new(ids),
    }
  }

  /// A message's picture or file handed over under `id`.
  pub fn keep(&self, chat_id: i64, message_id: i64, id: &str) {
    let mut ids = self.ids.lock().unwrap_or_else(PoisonError::into_inner);

    if ids.get(&(chat_id, message_id)).is_some_and(|held| held == id) {
      return;
    }

    ids.insert((chat_id, message_id), id.to_owned());

    if let Some(file) = &self.file {
      let crossed = Crossed {
        chat_id,
        message_id,
        id: id.to_owned(),
      };
      let written = serde_json::to_vec(&crossed).map_err(std::io::Error::other).and_then(|mut line| {
        line.push(b'\n');
        std::fs::OpenOptions::new().create(true).append(true).open(file)?.write_all(&line)
      });

      if let Err(why) = written {
        eprintln!(
          "telegram: the id of chat {chat_id} message {message_id}'s file could not be kept: {why}"
        );
      }
    }
  }

  /// The id a message's picture or file crossed with, where the bot account handed one over.
  pub fn of(&self, chat_id: i64, message_id: i64) -> Option<String> {
    self
      .ids
      .lock()
      .unwrap_or_else(PoisonError::into_inner)
      .get(&(chat_id, message_id))
      .cloned()
  }
}
