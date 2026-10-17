//! The provider's fixed values and English words, read from `policy/telegram.toml` (docs/telegram.md): what it
//! declares, how it reads what Telegram answers, the reasons it refuses with, as bmo-chat's refusal codes, how
//! it handles a picture, the names it gives a file with no name of its own, and the placeholders a message is
//! handed over as.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use bmo_chat::{ChatCapability, ChatRefusal, ChatVoice, MissedPictureReason};
use serde::Deserialize;

/// The policy, read once from the file the crate is built with.
pub static POLICY: LazyLock<Policy> = LazyLock::new(|| {
  Policy::read(include_str!("../policy/telegram.toml")).expect("policy/telegram.toml is the policy's shape")
});

/// The whole of `policy/telegram.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
  pub telegram: Platform,
  pub refusals: Refusals,
  pub pictures: Pictures,
  pub owner: Owner,
  pub gathering: Gathering,
  pub kinds: Kinds,
}

/// What the provider declares, and how it reads what Telegram answers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
  pub label: String,
  pub can: Vec<ChatCapability>,
  pub unpinned: Vec<ChatVoice>,
  pub gone_descriptions: Vec<String>,
  pub unchanged_description: String,
  pub caption_refused_descriptions: Vec<String>,
}

/// The provider's own reasons, each the detail of a `platform` refusal.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Refusals {
  pub message_gone: String,
  pub sticker_missing: String,
  pub sticker_unset: String,
  pub mode_down: String,
  pub mode_missing: String,
  pub owner_verb: String,
}

/// A picture's placeholder, why one did not go, and how one is redrawn.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pictures {
  pub mark: String,
  pub refused: String,
  pub failed: String,
  pub left_out: String,
  pub upload_quality: u8,
  pub kept_threshold_bytes: usize,
  pub kept_max_dimension: u32,
  pub kept_quality: u8,
}

/// A send in the owner's own account: the largest file it carries, and why a file is left out before anything
/// goes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
  pub largest_bytes: u64,
  pub empty_file: String,
  pub too_large: String,
  pub megabytes: String,
}

/// What the provider gathers of a message: why a file was not kept, and the names a file with none is given.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gathering {
  pub file_not_kept: String,
  pub voice_name: String,
  pub sound_name: String,
  pub video_name: String,
  pub video_note_name: String,
  pub gif_name: String,
  pub video_fallback_extension: String,
  pub audio_extensions: BTreeMap<String, String>,
  pub video_extensions: BTreeMap<String, String>,
}

/// The placeholder each kind of message is handed over as.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kinds {
  pub forwarded: String,
  pub forwarded_bare: String,
  pub forwarded_chat: String,
  pub photo: String,
  pub location: String,
  pub location_live: String,
  pub venue: String,
  pub contact: String,
  pub poll: String,
  pub dice: String,
  pub sticker: String,
  pub sticker_set: String,
  pub video: String,
  pub video_note: String,
  pub gif: String,
  pub voice: String,
  pub audio: String,
  pub document: String,
  pub unnamed_file: String,
  pub story: String,
  pub paid_media: String,
  pub giveaway: String,
  pub joined: String,
  pub left: String,
  pub pinned: String,
  pub pinned_message: String,
  pub renamed: String,
  pub photo_changed: String,
  pub photo_deleted: String,
  pub call_started: String,
  pub call_ended: String,
}

impl Policy {
  /// The policy a TOML text states, or why it is not the policy's shape.
  pub fn read(text: &str) -> Result<Self, toml::de::Error> {
    toml::from_str(text)
  }

  fn platform(&self, detail: String) -> ChatRefusal {
    ChatRefusal::Platform {
      label: self.telegram.label.clone(),
      detail,
    }
  }

  /// An act refused because Telegram no longer has the message, `why` in Telegram's own words. The send's
  /// result marks it gone.
  pub fn message_gone(&self, why: &str) -> ChatRefusal {
    self.platform(fill(&self.refusals.message_gone, &[("why", why)]))
  }

  /// A sticker the set does not carry.
  pub fn sticker_missing(&self, set: &str, emoji: &str) -> ChatRefusal {
    self.platform(fill(&self.refusals.sticker_missing, &[("set", set), ("emoji", emoji)]))
  }

  /// A sticker asked for where no sticker set is configured.
  pub fn sticker_unset(&self) -> ChatRefusal {
    self.platform(self.refusals.sticker_unset.clone())
  }

  /// A send asked to be spoken in a mode that is not running, `why` the mode's own reason, or in one the
  /// provider does not read by, where there is none.
  pub fn mode_down(&self, mode: &str, why: Option<&str>) -> ChatRefusal {
    let why = why.unwrap_or(&self.refusals.mode_missing);

    self.platform(fill(&self.refusals.mode_down, &[("mode", mode), ("why", why)]))
  }

  /// A send in the owner's own account of a verb it does not carry.
  pub fn owner_verb(&self, verb: &str) -> ChatRefusal {
    self.platform(fill(&self.refusals.owner_verb, &[("verb", verb)]))
  }

  /// A send Telegram refused, in Telegram's own description of it.
  pub fn refused(&self, description: &str) -> ChatRefusal {
    self.platform(description.to_owned())
  }

  /// Why a picture did not go: Telegram refused it without saying why, or the last try failed with `fault`.
  pub fn picture_failed(&self, fault: Option<&str>) -> MissedPictureReason {
    let detail = match fault {
      Some(fault) => fill(&self.pictures.failed, &[("fault", fault)]),
      None => self.pictures.refused.clone(),
    };

    MissedPictureReason::Platform { detail }
  }

  /// Why a picture was left out of an album that went.
  pub fn picture_left_out(&self) -> MissedPictureReason {
    MissedPictureReason::Platform {
      detail: self.pictures.left_out.clone(),
    }
  }
}

/// A line with each `{name}` mark filled by its value; a mark with no value stands as it is written.
pub fn fill(line: &str, values: &[(&str, &str)]) -> String {
  let mut out = String::new();
  let mut rest = line;

  while let Some(open) = rest.find('{') {
    out.push_str(&rest[..open]);

    let after = &rest[open + 1..];
    let name_end = after
      .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
      .unwrap_or(after.len());
    let value = (name_end > 0 && after[name_end..].starts_with('}'))
      .then(|| values.iter().find(|(name, _)| *name == &after[..name_end]))
      .flatten();

    match value {
      Some((_, value)) => {
        out.push_str(value);
        rest = &after[name_end + 1..];
      }
      None => {
        out.push('{');
        rest = after;
      }
    }
  }

  out + rest
}
