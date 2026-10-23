//! A send in the owner's own voice (docs/telegram.md, 「The owner's voice」): the words in the parts and the
//! markup the bot's send would take, and files gathered into the albums Telegram takes, each sent through the
//! reader over the seam and noted, so the bot account's sight of it is known as its echo. What the owner's
//! account refuses comes back as the reader said it.

use std::path::Path;

use bmo_chat::{
  AlbumItem, ChatRefusal, ChatSend, ChatSendRequest, ChatSendResult, ChatSent, FileItem, MissedPicture,
  MissedPictureReason,
};
use telegram_dialect::ALBUM_LIMIT;
use telegram_dialect::js;

use crate::compose::{caption, text_parts};
use crate::echoes::Echoes;
use crate::files::audio_type;
use crate::pictures::Picture;
use crate::policy::{POLICY, fill};
use crate::provider::wire_name;
use crate::reader::{AsOwner, ReaderClient};
use crate::send::{Sent, Speaker, refused};

/// What Telegram makes of a file, by its own type, or by its name where none came with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
  Photo,
  Animation,
  Video,
  Audio,
  Document,
}

/// A file's kind: a JPEG or a PNG a photo, a GIF an animation, any other image a file kept whole, a video a
/// video, a sound an audio, everything else a document.
pub fn kind_of(mime: &str, name: &str) -> Kind {
  let given = mime.trim().to_lowercase();
  let guessed = if given.is_empty() {
    mime_guess::from_path(name).first_raw().unwrap_or_default().to_owned()
  } else {
    given
  };

  match guessed.as_str() {
    "image/gif" => Kind::Animation,
    "image/jpeg" | "image/jpg" | "image/png" => Kind::Photo,
    video if video.starts_with("video/") => Kind::Video,
    audio if audio.starts_with("audio/") => Kind::Audio,
    _ => Kind::Document,
  }
}

/// Which files Telegram lets stand together in one album: pictures and videos, documents, sounds. It mixes none
/// of the three and takes an animation into none of them.
pub fn album_kind(kind: Kind) -> Option<u8> {
  match kind {
    Kind::Photo | Kind::Video => Some(0),
    Kind::Document => Some(1),
    Kind::Audio => Some(2),
    Kind::Animation => None,
  }
}

/// The files of one send gathered into the albums Telegram takes, the way a phone sends several at once: each
/// kind of album holds every file of its kind up to `limit`, an animation goes alone, and the albums go in the
/// order their first file was attached.
pub fn albums<T: Clone>(files: &[T], kind: impl Fn(&T) -> Kind, limit: usize) -> Vec<Vec<T>> {
  let mut albums: Vec<Vec<T>> = vec![];
  let mut open: Vec<(u8, usize)> = vec![];

  for file in files {
    let joined = album_kind(kind(file)).and_then(|of| {
      open
        .iter()
        .find(|(open_kind, _)| *open_kind == of)
        .map(|(_, at)| *at)
        .filter(|at| albums[*at].len() < limit)
    });

    if let Some(at) = joined {
      albums[at].push(file.clone());
      continue;
    }

    albums.push(vec![file.clone()]);

    if let Some(of) = album_kind(kind(file)) {
      open.retain(|(open_kind, _)| *open_kind != of);
      open.push((of, albums.len() - 1));
    }
  }

  albums
}

/// A size as the owner reads it, in megabytes: to one place below ten, whole above.
pub fn megabytes(size: u64) -> String {
  let mib = 1024.0 * 1024.0;
  let places = if size < 10 * 1024 * 1024 { 1 } else { 0 };

  fill(&POLICY.owner.megabytes, &[("megabytes", &format!("{:.*}", places, size as f64 / mib))])
}

/// Why a file is refused before anything goes: it holds nothing, or it is past the ceiling.
pub fn refusal(name: &str, size: u64) -> Option<String> {
  let owner = &POLICY.owner;

  if size == 0 {
    return Some(fill(&owner.empty_file, &[("name", name)]));
  }

  (size > owner.largest_bytes).then(|| {
    fill(
      &owner.too_large,
      &[("name", name), ("size", &megabytes(size)), ("largest", &megabytes(owner.largest_bytes))],
    )
  })
}

/// The words typed beside the files, with every line break a single line feed.
pub fn caption_of(field: Option<&str>) -> String {
  js::trim(&field.unwrap_or_default().replace("\r\n", "\n").replace('\r', "\n")).to_owned()
}

// The name the owner's account refuses a send by while it is not connected.
const UNAVAILABLE: &str = "unavailable";

// What the owner's account refused, as the provider refuses it: a mode that is not running, or the account's
// own words.
fn owner_refused(name: &str, error: &str) -> ChatRefusal {
  if name == UNAVAILABLE {
    POLICY.mode_down("human-reader", Some(error))
  } else {
    POLICY.refused(error)
  }
}

// One item of the owner's album as a file: one handed over as it is, one named by an address read where a
// picture of the bot's may be read from.
async fn file_of(
  speaker: &Speaker,
  root: Option<&Path>,
  item: &AlbumItem,
) -> Result<FileItem, MissedPicture> {
  let picture = Picture::of(root, item)?;

  if let AlbumItem::File(file) = item {
    return Ok(file.clone());
  }

  let (bytes, mime) = speaker.picture_bytes(&picture).await.map_err(|_| MissedPicture {
    address: picture.address.clone(),
    reason: MissedPictureReason::Unfetched,
  })?;
  let name = picture.address.rsplit('/').next().unwrap_or_default().to_owned();

  Ok(FileItem { name, mime, bytes })
}

/// The owner's voice as one send uses it: the bot account's sending, for where a picture is read from and the
/// record of rooms sent into; the reader the owner's account is reached through; and the notes of what went.
pub struct OwnerVoice<'a> {
  pub speaker: &'a Speaker,
  pub reader: &'a ReaderClient,
  pub echoes: &'a Echoes,
}

impl OwnerVoice<'_> {
  /// One send in the owner's voice: words, a picture, a file, a sound or an album, each as the bot's send would
  /// carry it; any other verb refused.
  pub async fn send(&self, request: &ChatSendRequest) -> Sent {
    let room = request.room_id;
    let reply_to = request.reply_to_message_id;

    match &request.send {
      ChatSend::Text { text, silent, fold } => self.say(room, text, *silent, *fold, reply_to).await,
      ChatSend::Album { items, caption } => self.album(room, items, caption.as_deref(), reply_to).await,
      ChatSend::Photo { image_url, caption } => {
        let items = [AlbumItem::Address {
          address: image_url.clone(),
        }];

        self.album(room, &items, caption.as_deref(), reply_to).await
      }
      // A document is kept whole as a file, whatever its name says it is.
      ChatSend::Document { bytes, file_name } => {
        let items = [AlbumItem::File(FileItem {
          name: file_name.clone(),
          mime: "application/octet-stream".to_owned(),
          bytes: bytes.clone(),
        })];

        self.album(room, &items, None, reply_to).await
      }
      ChatSend::Audio { audio, file_name } => {
        let items = [AlbumItem::File(FileItem {
          name: file_name.clone(),
          mime: audio_type(audio).1.to_owned(),
          bytes: audio.clone(),
        })];

        self.album(room, &items, None, reply_to).await
      }
      other => Ok(refused(POLICY.owner_verb(&wire_name(&other.kind())))),
    }
  }

  /// The owner's markdown said in the room through the reader in the parts the dialect divides it into, each in
  /// its markup and as its plain text where the account refuses the markup, the first answering the message
  /// named: the message the first part went out as. A part that does not go ends the send there.
  pub async fn say(
    &self,
    chat_id: i64,
    markdown: &str,
    silent: bool,
    fold: bool,
    reply_to: Option<i64>,
  ) -> Sent {
    let mut first = None;

    for part in text_parts(markdown, fold) {
      let pinned = if first.is_none() { reply_to } else { None };

      match self.reader.owner_say(chat_id, &part, silent, pinned).await? {
        AsOwner::Sent(went) => {
          self.echoes.sent(chat_id, went.message_id, &went.words);
          self.speaker.spoke_in(chat_id);
          first.get_or_insert(went.message_id);
        }
        AsOwner::Refused { name, error } => return Ok(refused(owner_refused(&name, &error))),
      }
    }

    Ok(ChatSendResult::Sent(ChatSent {
      message_id: first.flatten(),
      missed: Vec::new(),
    }))
  }

  /// The owner's files sent through the reader as the albums Telegram takes, the caption in its markup and the
  /// reply with the first. A file that holds nothing or is past the ceiling is missed before anything goes; an
  /// album the account would not take is missed by every file in it and stops none after it. The message the
  /// first file went out as.
  pub async fn album(
    &self,
    chat_id: i64,
    items: &[AlbumItem],
    caption_field: Option<&str>,
    reply_to: Option<i64>,
  ) -> Sent {
    let speaker = self.speaker;
    let root = speaker.sending.picture_root.as_deref();
    let first_caption = caption(&caption_of(caption_field));
    let bare = caption("");
    let mut missed = Vec::new();
    let mut files = Vec::new();

    for item in items {
      match file_of(speaker, root, item).await {
        Ok(file) => match refusal(&file.name, file.bytes.len() as u64) {
          Some(detail) => missed.push(MissedPicture {
            address: file.name.clone(),
            reason: MissedPictureReason::Platform { detail },
          }),
          None => files.push(file),
        },
        Err(one) => missed.push(one),
      }
    }

    let mut first: Option<i64> = None;
    let mut went_any = false;

    for (at, album) in
      albums(&files, |file| kind_of(&file.mime, &file.name), ALBUM_LIMIT).iter().enumerate()
    {
      let (said, pinned) = if at == 0 { (&first_caption, reply_to) } else { (&bare, None) };

      match self.reader.owner_album(chat_id, album, said, pinned).await? {
        AsOwner::Sent(went) => {
          went_any = true;
          self.speaker.spoke_in(chat_id);

          for one in &went {
            self.echoes.sent(chat_id, one.message_id, &one.words);
          }

          first = first.or(went.into_iter().find_map(|one| one.message_id));
        }
        AsOwner::Refused { name, error } => {
          let detail = match owner_refused(&name, &error) {
            ChatRefusal::Platform { detail, .. } => detail,
            other => other.to_string(),
          };

          missed.extend(album.iter().map(|file| MissedPicture {
            address: file.name.clone(),
            reason: MissedPictureReason::Platform {
              detail: detail.clone(),
            },
          }));
        }
      }
    }

    if !went_any && !missed.is_empty() {
      return Ok(refused(ChatRefusal::NoPictureSent {
        label: POLICY.telegram.label.clone(),
        missed,
      }));
    }

    Ok(ChatSendResult::Sent(ChatSent {
      message_id: first,
      missed,
    }))
  }
}
