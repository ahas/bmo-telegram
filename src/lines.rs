//! The ledger of whose numbering each line of a room numbered per account stands under (docs/telegram.md, 「The
//! owner's voice」): in a room where each account numbers its own messages the receiving side holds each line
//! under the number of the account that read or sent it, and an act in the owner's account needs that account's
//! own number. A number the owner's account gave stands as it is; a number the bot account gave is kept with the
//! line's stamp, by which the owner's account finds the line again. Kept in the provider's data folder, one record
//! to a line added as each is learned, so a restart forgets none; a later record of a line stands over an earlier
//! one.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::echoes::numbered_for_everyone;
use crate::kinds::Message;

/// A line of the bot account's as the owner's account can find it again: who sent it, when to the second, in epoch
/// milliseconds, and the SHA-256 of its words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stamp {
  pub sender_id: i64,
  pub sent_at: i64,
  pub words: String,
}

/// The SHA-256 of a line's words, in lowercase hex, as the reader reckons it too.
pub fn words_hash(words: &str) -> String {
  format!("{:x}", Sha256::digest(words.as_bytes()))
}

/// What the ledger says a number of a room names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
  /// The owner's account's own number, or one it turns into its own: it stands as it is.
  Owners,
  /// The bot account's number for a line, found on the owner's account by its stamp.
  Bots(Stamp),
  /// No line the provider can tell: a number under both numberings, or one it never learned in a room the bot
  /// account reads.
  Unknown,
}

// Whose numbering one record holds a line under.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "by", rename_all = "kebab-case")]
enum Numbering {
  Owner,
  Bot { stamp: Stamp },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
  chat_id: i64,
  message_id: i64,
  #[serde(flatten)]
  numbering: Numbering,
}

// What the ledger holds of one number of a room: whether the owner's account gave it, and the stamp of the bot
// account's line under it.
#[derive(Debug, Default)]
struct Held {
  owners: bool,
  bots: Option<Stamp>,
}

#[derive(Debug, Default)]
struct Kept {
  lines: HashMap<(i64, i64), Held>,
  // The rooms the ledger holds a line of the bot account's for: the rooms the bot account reads.
  bot_rooms: HashSet<i64>,
}

impl Kept {
  // One record taken in; whether it changed what the ledger holds.
  fn take(&mut self, record: &Record) -> bool {
    let held = self.lines.entry((record.chat_id, record.message_id)).or_default();

    match &record.numbering {
      Numbering::Owner if held.owners => false,
      Numbering::Owner => {
        held.owners = true;
        true
      }
      Numbering::Bot { stamp } if held.bots.as_ref() == Some(stamp) => false,
      Numbering::Bot { stamp } => {
        held.bots = Some(stamp.clone());
        self.bot_rooms.insert(record.chat_id);
        true
      }
    }
  }
}

/// The ledger, kept in a file where one is named.
#[derive(Debug, Default)]
pub struct Lines {
  file: Option<PathBuf>,
  kept: Mutex<Kept>,
}

// Whether a number of a room is one the ledger keeps: a room numbered for everyone needs none, and a number at
// or below zero is the history walk's, which the reader turns back into the account's own.
fn ledgered(chat_id: i64, message_id: i64) -> bool {
  !numbered_for_everyone(chat_id) && message_id > 0
}

impl Lines {
  /// A ledger kept only in memory.
  pub fn in_memory() -> Self {
    Self::default()
  }

  /// The ledger kept in `file`, read back from it where it stands. A record cut short by a stop is left out.
  pub fn kept_in(file: PathBuf) -> Self {
    let bytes = std::fs::read(&file).unwrap_or_default();
    let mut kept = Kept::default();

    for record in serde_json::Deserializer::from_slice(&bytes).into_iter::<Record>().map_while(Result::ok) {
      kept.take(&record);
    }

    Self {
      file: Some(file),
      kept: Mutex::new(kept),
    }
  }

  fn keep(&self, record: Record) {
    let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);

    if !kept.take(&record) {
      return;
    }

    let Some(file) = &self.file else {
      return;
    };
    let written = serde_json::to_vec(&record).map_err(std::io::Error::other).and_then(|mut line| {
      line.push(b'\n');
      std::fs::OpenOptions::new().create(true).append(true).open(file)?.write_all(&line)
    });

    if let Err(why) = written {
      eprintln!(
        "telegram: the numbering of chat {} message {} could not be kept: {why}",
        record.chat_id, record.message_id
      );
    }
  }

  /// A number the owner's account gave a line: one a send in it answered, or one the reader handed a line over
  /// under.
  pub fn owners(&self, chat_id: i64, message_id: i64) {
    if ledgered(chat_id, message_id) {
      self.keep(Record {
        chat_id,
        message_id,
        numbering: Numbering::Owner,
      });
    }
  }

  /// A line the bot account read or sent, under its number, with its stamp; a line it holds already takes the
  /// words an edit left it.
  pub fn bots(&self, message: &Message) {
    let sender = message
      .from
      .as_ref()
      .map(|from| from.id)
      .or(message.sender_chat.as_ref().map(|chat| chat.id));
    let (chat_id, message_id) = (message.chat.id, message.message_id);
    let (Some(sender_id), true) = (sender, ledgered(chat_id, message_id)) else {
      return;
    };
    let words = message.text.as_deref().or(message.caption.as_deref()).unwrap_or_default();

    self.keep(Record {
      chat_id,
      message_id,
      numbering: Numbering::Bot {
        stamp: Stamp {
          sender_id,
          sent_at: message.date * 1000,
          words: words_hash(words),
        },
      },
    });
  }

  /// An edit the bot account read: the new words of a line the ledger holds under the bot account's number. A line
  /// it does not hold so is left alone, since the edit may be of a line the owner's account sent, which the
  /// receiving side holds under that account's number.
  pub fn edited(&self, message: &Message) {
    let held = {
      let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);

      kept
        .lines
        .get(&(message.chat.id, message.message_id))
        .is_some_and(|held| held.bots.is_some())
    };

    if held {
      self.bots(message);
    }
  }

  /// What a Bot API call answered with, where it is a message or several the bot account sent or edited.
  pub fn answered(&self, result: &Value) {
    let messages = match result {
      Value::Array(messages) => messages.iter().collect(),
      one => vec![one],
    };

    for message in messages {
      if let Ok(message) = Message::deserialize(message) {
        self.bots(&message);
      }
    }
  }

  /// What the number `message_id` of a room names, as the owner's account is to be asked for it.
  pub fn line(&self, chat_id: i64, message_id: i64) -> Line {
    if !ledgered(chat_id, message_id) {
      return Line::Owners;
    }

    let kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);

    match kept.lines.get(&(chat_id, message_id)) {
      Some(Held {
        owners: true,
        bots: Some(_),
      }) => Line::Unknown,
      Some(Held {
        bots: Some(stamp), ..
      }) => Line::Bots(stamp.clone()),
      Some(Held { owners: true, .. }) => Line::Owners,
      _ if kept.bot_rooms.contains(&chat_id) => Line::Unknown,
      _ => Line::Owners,
    }
  }
}
