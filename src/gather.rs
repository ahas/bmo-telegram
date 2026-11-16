//! What a message carries, gathered for the receiver (docs/telegram.md, 「Reading a room」): its picture, its
//! file, and the sticker and custom emoji assets among its expressions, each fetched through the Bot API, or
//! past the Bot API's ceiling for a bot through the reader. What is gathered crosses as bytes under the file's
//! own name and its id; where the receiver keeps it is the receiver's. A picture is kept as `telegram-picture`
//! keeps one.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use bmo_chat::{ChatFileKind, MessageFile};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use telegram_picture::{Keeping, kept, raster_type};

use crate::bot_api::{BotApi, DOWNLOAD_LIMIT};
use crate::file_ids::FileIds;
use crate::policy::POLICY;
use crate::reader::{ReaderClient, ReaderFetch};
use crate::reading::Picture;

/// A file as the gathering takes one: what Telegram calls it, the name it does not change, its own name, size
/// and type, whether it was a spoken note, and what kind of file it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotFile {
  pub file_id: String,
  pub file_unique_id: Option<String>,
  pub file_name: Option<String>,
  pub file_size: Option<i64>,
  pub mime_type: Option<String>,
  pub voice: bool,
  pub kind: ChatFileKind,
}

// The custom emoji Telegram is asked for at once.
const EMOJI_BATCH: usize = 200;

// How many files gathered lately are remembered, so a file asked again — an edit that kept its file — is not
// fetched twice.
const REMEMBERED: usize = 1024;

fn text(value: &Value, key: &str) -> Option<String> {
  value.get(key).and_then(Value::as_str).map(str::to_owned)
}

// A type as its first part reads, without its parameters or its case.
fn bare_type(mime_type: Option<&str>) -> String {
  mime_type
    .unwrap_or_default()
    .split(';')
    .next()
    .unwrap_or_default()
    .trim()
    .to_lowercase()
}

/// What a sound with no name of its own is called: a note somebody spoke, or a sound file they attached.
pub fn media_file_name(mime_type: Option<&str>, voice: bool) -> String {
  let lines = &POLICY.gathering;
  let name = if voice { &lines.voice_name } else { &lines.sound_name };

  format!(
    "{name}{}",
    lines
      .audio_extensions
      .get(&bare_type(mime_type))
      .map(String::as_str)
      .unwrap_or_default()
  )
}

// What a video with no name of its own is called: a video, a round note or a GIF.
fn video_file_name(mime_type: Option<&str>, what: &str) -> String {
  let lines = &POLICY.gathering;

  format!(
    "{what}{}",
    lines
      .video_extensions
      .get(&bare_type(mime_type))
      .unwrap_or(&lines.video_fallback_extension)
  )
}

/// The file a message carries. A GIF arrives with a document beside its animation, so the video kinds are
/// asked first; each is a video, and so is a video somebody attached as a file.
pub fn bot_file(message: &Value) -> Option<BotFile> {
  let lines = &POLICY.gathering;
  let present = |key: &str| message.get(key).filter(|value| !value.is_null());
  let video = present("animation").or_else(|| present("video")).or_else(|| present("video_note"));
  let file = video
    .or_else(|| present("document"))
    .or_else(|| present("audio"))
    .or_else(|| present("voice"))?;
  let mime_type = text(file, "mime_type");
  let what = if present("animation").is_some() {
    &lines.gif_name
  } else if present("video_note").is_some() {
    &lines.video_note_name
  } else {
    &lines.video_name
  };
  let audio = mime_type.as_deref().is_some_and(|mime| mime.starts_with("audio/"));

  Some(BotFile {
    file_id: text(file, "file_id").unwrap_or_default(),
    file_unique_id: text(file, "file_unique_id"),
    file_name: text(file, "file_name")
      .or_else(|| video.map(|_| video_file_name(mime_type.as_deref(), what))),
    file_size: file.get("file_size").and_then(Value::as_i64),
    kind: if present("animation").is_some() {
      ChatFileKind::Animation
    } else if present("video_note").is_some() {
      ChatFileKind::RoundVideo
    } else if video.is_some() || mime_type.as_deref().is_some_and(|mime| mime.starts_with("video/")) {
      ChatFileKind::Video
    } else if present("voice").is_some() {
      ChatFileKind::VoiceNote
    } else if present("document").is_some() && !audio {
      ChatFileKind::Document
    } else {
      ChatFileKind::Audio
    },
    voice: present("voice").is_some(),
    mime_type,
  })
}

// Whether a file is past the Bot API's ceiling for a bot's download.
fn oversized(size: Option<i64>) -> bool {
  size.is_some_and(|size| u64::try_from(size).unwrap_or_default() > DOWNLOAD_LIMIT)
}

// The extension a picture's type implies.
fn picture_extension(mime: &str) -> &'static str {
  match mime {
    "image/png" => "png",
    "image/gif" => "gif",
    "image/webp" => "webp",
    "image/avif" => "avif",
    _ => "jpg",
  }
}

/// The files gathered lately, each by its message, its name and the file Telegram says it is.
#[derive(Debug, Default)]
struct Remembered {
  kept: VecDeque<(String, Option<String>)>,
}

impl Remembered {
  fn holds(&self, key: &str, unique: Option<&str>) -> bool {
    unique.is_some() && self.kept.iter().any(|(held, id)| held == key && id.as_deref() == unique)
  }

  fn keep(&mut self, key: String, unique: Option<String>) {
    self.kept.retain(|(held, _)| *held != key);
    self.kept.push_back((key, unique));

    if self.kept.len() > REMEMBERED {
      self.kept.pop_front();
    }
  }
}

// The id a file crosses with: Telegram's name for it that does not change, or its place in the message where
// Telegram gives none.
fn file_id_of(unique: Option<&str>, place: &str) -> String {
  unique.filter(|unique| !unique.is_empty()).unwrap_or(place).to_owned()
}

/// The gathering: the Bot API, the reader for what is past its ceiling, the files gathered lately, and the
/// record of the id each message's picture or file crossed with.
pub struct Gatherer {
  api: BotApi,
  reader: Option<ReaderClient>,
  remembered: Mutex<Remembered>,
  file_ids: Arc<FileIds>,
}

impl Gatherer {
  pub fn new(api: BotApi, reader: Option<ReaderClient>, file_ids: Arc<FileIds>) -> Self {
    Self {
      api,
      reader,
      remembered: Mutex::default(),
      file_ids,
    }
  }

  fn holds(&self, key: &str, unique: Option<&str>) -> bool {
    self.remembered.lock().unwrap_or_else(PoisonError::into_inner).holds(key, unique)
  }

  fn keep(&self, key: String, unique: Option<String>) {
    self.remembered.lock().unwrap_or_else(PoisonError::into_inner).keep(key, unique);
  }

  // A file's bytes past the Bot API's ceiling, through the reader.
  async fn through_reader(&self, chat_id: i64, message_id: i64, sent_at: i64) -> Result<Vec<u8>, String> {
    let Some(reader) = &self.reader else {
      return Err("the file exceeds the Bot API's ceiling and no reader stands".to_owned());
    };

    match reader.file(chat_id, message_id, sent_at).await {
      ReaderFetch::Fetched(file) => Ok(file.bytes),
      ReaderFetch::Unavailable(why) | ReaderFetch::NoReader(why) => Err(why),
    }
  }

  /// The file a message carries, gathered: none where it carries none or it was gathered lately as the same
  /// file, or why it could not be had.
  pub async fn file(
    &self,
    chat_id: i64,
    message_id: i64,
    raw: &Value,
  ) -> Result<Option<MessageFile>, String> {
    let Some(file) = bot_file(raw) else {
      return Ok(None);
    };
    let name = file
      .file_name
      .clone()
      .unwrap_or_else(|| media_file_name(file.mime_type.as_deref(), file.voice));
    let key = format!("{chat_id}:{message_id}:{name}");

    if self.holds(&key, file.file_unique_id.as_deref()) {
      return Ok(None);
    }

    let mime = file.mime_type.clone().unwrap_or_else(|| "application/octet-stream".to_owned());
    let sent_at = raw.get("date").and_then(Value::as_i64).unwrap_or_default();
    let bytes = if oversized(file.file_size) {
      self.through_reader(chat_id, message_id, sent_at).await?
    } else {
      self.api.file_bytes(&file.file_id).await?
    };
    let id = file_id_of(file.file_unique_id.as_deref(), "file");

    self.keep(key, file.file_unique_id);
    self.file_ids.keep(chat_id, message_id, &id);

    Ok(Some(MessageFile {
      id,
      name,
      mime,
      bytes,
      kind: Some(file.kind),
    }))
  }

  /// The picture a message carries, kept as a picture is kept: fetched through the Bot API, or past its ceiling
  /// through the reader. None where nothing could be had.
  pub async fn picture(
    &self,
    chat_id: i64,
    message_id: i64,
    sent_at: i64,
    picture: &Picture,
  ) -> Option<MessageFile> {
    let bytes = if oversized(picture.file_size) {
      match &self.reader {
        Some(reader) => match reader.picture(chat_id, message_id, Some(sent_at)).await {
          ReaderFetch::Fetched(file) => Ok(file.bytes),
          ReaderFetch::Unavailable(why) | ReaderFetch::NoReader(why) => Err(why),
        },
        None => Err("the picture exceeds the Bot API's ceiling and no reader stands".to_owned()),
      }
    } else {
      self.api.file_bytes(&picture.file_id).await
    };
    let bytes = bytes
      .inspect_err(|why| {
        eprintln!("telegram: the picture of chat {chat_id} message {message_id} was not had: {why}")
      })
      .ok()?;
    let pictures = &POLICY.pictures;
    let kept = kept(
      bytes,
      Keeping {
        threshold_bytes: pictures.kept_threshold_bytes,
        max_dimension: pictures.kept_max_dimension,
        quality: pictures.kept_quality,
      },
    );
    let mime = raster_type(&kept).unwrap_or("image/jpeg").to_owned();
    let id = file_id_of(picture.file_unique_id.as_deref(), "picture");

    self.file_ids.keep(chat_id, message_id, &id);

    Some(MessageFile {
      id,
      name: format!("{message_id}.{}", picture_extension(&mime)),
      mime,
      bytes: kept,
      kind: Some(ChatFileKind::Photo),
    })
  }

  // A sticker's asset as a file of the message, at its place there: the file Telegram keeps for it, an animated
  // one's TGS scene among them, so a room draws it as the platform does.
  async fn sticker_asset(
    &self,
    chat_id: i64,
    message_id: i64,
    sticker: &Value,
    name: &str,
    place: &str,
  ) -> Option<MessageFile> {
    let (extension, mime) = sticker_format(sticker);
    let name = format!("{name}.{extension}");
    let unique = text(sticker, "file_unique_id");
    let key = format!("{chat_id}:{message_id}:{name}");

    if self.holds(&key, unique.as_deref()) {
      return None;
    }

    let bytes = self
      .api
      .file_bytes(&text(sticker, "file_id").unwrap_or_default())
      .await
      .inspect_err(|why| eprintln!("telegram: the asset {name} of chat {chat_id} was not had: {why}"))
      .ok()?;

    let id = file_id_of(unique.as_deref(), place);

    self.keep(key, unique);

    Some(MessageFile {
      id,
      name,
      mime: mime.to_owned(),
      bytes,
      kind: Some(ChatFileKind::Sticker),
    })
  }

  /// Whether a message's expressions carry an asset to gather: a sticker, or a custom emoji among its words.
  pub fn has_assets(raw: &Value) -> bool {
    raw.get("sticker").is_some_and(|sticker| !sticker.is_null()) || !custom_emoji(raw).is_empty()
  }

  /// The sticker a message is and the custom emoji among its words, each asset as a file of the message. An
  /// asset Telegram cannot supply is left out; its characters stand as they are.
  pub async fn assets(&self, chat_id: i64, message_id: i64, raw: &Value) -> Vec<MessageFile> {
    let mut files = Vec::new();

    if let Some(sticker) = raw.get("sticker").filter(|sticker| !sticker.is_null()) {
      let unique = text(sticker, "file_unique_id").unwrap_or_default();
      let digest = format!("{:x}", Sha256::digest(unique.as_bytes()));

      files.extend(
        self
          .sticker_asset(chat_id, message_id, sticker, &format!("sticker-{}", &digest[..12]), "sticker")
          .await,
      );
    }

    for batch in custom_emoji(raw).chunks(EMOJI_BATCH) {
      let stickers: Vec<Value> =
        match self.api.call("getCustomEmojiStickers", &json!({ "custom_emoji_ids": batch })).await {
          Ok(stickers) => stickers,
          Err(why) => {
            eprintln!("telegram: expressions unavailable in chat {chat_id}, message {message_id}: {why}");
            break;
          }
        };

      for sticker in &stickers {
        if let Some(id) = text(sticker, "custom_emoji_id") {
          let name = format!("emoji-{id}");

          files.extend(self.sticker_asset(chat_id, message_id, sticker, &name, &name).await);
        }
      }
    }

    files
  }
}

/// The extension and the type of the file Telegram keeps for a sticker or a custom emoji: an animated one's TGS
/// scene, a video one's WebM, or a still one's WebP.
pub fn sticker_format(sticker: &Value) -> (&'static str, &'static str) {
  let flag = |key: &str| sticker.get(key).and_then(Value::as_bool) == Some(true);

  if flag("is_animated") {
    ("tgs", "application/x-tgsticker")
  } else if flag("is_video") {
    ("webm", "video/webm")
  } else {
    ("webp", "image/webp")
  }
}

// The custom emoji among a message's words, each once.
fn custom_emoji(raw: &Value) -> Vec<String> {
  let entities = ["entities", "caption_entities"]
    .iter()
    .find_map(|key| raw.get(*key).and_then(Value::as_array))
    .cloned()
    .unwrap_or_default();
  let mut ids: Vec<String> = Vec::new();

  for entity in &entities {
    if entity.get("type").and_then(Value::as_str) == Some("custom_emoji")
      && let Some(id) = text(entity, "custom_emoji_id")
      && !ids.contains(&id)
    {
      ids.push(id);
    }
  }

  ids
}
