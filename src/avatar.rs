//! The avatar prefix (docs/telegram.md, 「The avatar」): what the agent's words begin with in the owner's
//! account, the one thing telling them from the owner's own, put before every line the avatar sends and read
//! off every line of the owner's account that comes back with it.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use bmo_chat::{ChatVoice, HandedMessage};

use crate::policy::POLICY;

/// The prefix as words are read against it: a platform may trim the space after it, so the mark ends at its last
/// character that is not a space.
fn mark(prefix: &str) -> &str {
  prefix.trim_end()
}

/// Whether the words begin with the prefix.
pub fn is_avatar_text(prefix: &str, words: &str) -> bool {
  let mark = mark(prefix);

  !mark.is_empty() && words.trim_start().starts_with(mark)
}

/// The words without the prefix they begin with, and the one space after it; words that do not begin with it
/// stand as they are.
pub fn without_prefix<'a>(prefix: &str, words: &'a str) -> &'a str {
  if !is_avatar_text(prefix, words) {
    return words;
  }

  let rest = &words.trim_start()[mark(prefix).len()..];

  rest.strip_prefix(' ').unwrap_or(rest)
}

/// The avatar as the provider's reading knows it: the owner's number its lines come from, and the prefix they
/// carry, the deployment's where it names one and the policy's otherwise.
#[derive(Debug, Clone)]
pub struct AvatarLines {
  owner_id: i64,
  prefix: String,
  // Every prefix a send has handed it, each an agent's name tag, as their marks, and the file they are kept in.
  known: Arc<Mutex<BTreeSet<String>>>,
  kept_in: Option<PathBuf>,
}

impl AvatarLines {
  /// The avatar of the owner `owner_id`, behind `configured` where it is not blank.
  pub fn new(owner_id: i64, configured: Option<String>) -> Self {
    let prefix = configured
      .filter(|prefix| !prefix.trim().is_empty())
      .unwrap_or_else(|| POLICY.avatar.prefix.clone());

    Self {
      owner_id,
      prefix,
      known: Arc::default(),
      kept_in: None,
    }
  }

  /// The avatar keeping the prefixes it is handed in `file`, one a line, and reading those kept there before.
  pub fn kept_in(mut self, file: PathBuf) -> Self {
    let kept: BTreeSet<String> = std::fs::read_to_string(&file)
      .unwrap_or_default()
      .lines()
      .map(str::to_owned)
      .filter(|line| !line.trim().is_empty())
      .collect();

    *self.known.lock().unwrap_or_else(PoisonError::into_inner) = kept;
    self.kept_in = Some(file);
    self
  }

  /// A prefix a send handed it learnt, so a line it went before is read back as the agent's whatever the tag says.
  pub fn learn(&self, prefix: &str) {
    let mark = mark(prefix);

    if mark.is_empty() {
      return;
    }

    let fresh = self.known.lock().unwrap_or_else(PoisonError::into_inner).insert(mark.to_owned());

    if fresh && let Some(file) = &self.kept_in {
      use std::io::Write;

      if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(out, "{mark}");
      }
    }
  }

  /// The prefix a line's words begin with: the longest of those learnt, else the default's; none where none does.
  pub fn prefix_of(&self, words: &str) -> Option<String> {
    let known = self.known.lock().unwrap_or_else(PoisonError::into_inner);

    known
      .iter()
      .chain(std::iter::once(&self.prefix))
      .filter(|prefix| is_avatar_text(prefix, words))
      .max_by_key(|prefix| mark(prefix).len())
      .cloned()
  }

  /// The prefix the avatar's words carry where a send names none.
  pub fn prefix(&self) -> &str {
    &self.prefix
  }

  /// A message read back from the owner's account made the agent's own where it is a line the avatar sent: from
  /// the owner's number, in a room with an identity of the provider's own, its words behind the prefix. Such a
  /// message goes under that identity in the avatar's mode, the prefix taken off its words and off the words
  /// typed under a file. Whether it was.
  pub fn take_as_agent_s(&self, message: &mut HandedMessage) -> bool {
    let Some(identity) = message.room.identity else {
      return false;
    };
    let words = message.context.spoken_text.as_deref().unwrap_or(&message.text);

    if message.sender_id != Some(self.owner_id) {
      return false;
    }

    let Some(prefix) = self.prefix_of(words) else {
      return false;
    };

    message.sender_id = Some(identity);
    message.mode = ChatVoice::HumanAvatar;
    message.text = without_prefix(&prefix, &message.text).to_owned();
    message.context.spoken_text = message
      .context
      .spoken_text
      .as_deref()
      .map(|spoken| without_prefix(&prefix, spoken).to_owned());

    true
  }
}
