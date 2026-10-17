//! A send in the owner's own voice (docs/telegram.md, 「The owner's voice」): words cut into the pieces Telegram
//! takes and files gathered into the albums it takes, each sent through the reader over the seam. What the
//! owner's account refuses comes back as the reader said it.

use std::path::Path;

use bmo_chat::{
  AlbumItem, ChatFailure, ChatRefusal, ChatSendResult, FileItem, MissedPicture, MissedPictureReason,
};
use telegram_dialect::ALBUM_LIMIT;
use telegram_dialect::js;
use telegram_dialect::telegram::{TEXT_LIMIT, plain_parts};

use crate::pictures::Picture;
use crate::policy::{POLICY, fill};
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

/// The owner's words said in the room through the reader, piece by piece as a client sends a long paste: the
/// message the first piece went out as.
pub async fn say(reader: &ReaderClient, chat_id: i64, text: &str) -> Sent {
  let mut first = None;

  for part in plain_parts(text, TEXT_LIMIT) {
    match reader.owner_say(chat_id, &part, None).await? {
      AsOwner::Sent(went) => {
        first.get_or_insert(went);
      }
      AsOwner::Refused { name, error } => return Ok(refused(owner_refused(&name, &error))),
    }
  }

  Ok(ChatSendResult::Sent {
    message_id: first.flatten(),
    missed: Vec::new(),
  })
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

/// The owner's files sent through the reader as the albums Telegram takes, the caption with the first. A file
/// that holds nothing or is past the ceiling is missed before anything goes; an album the account would not
/// take is missed by every file in it and stops none after it. The message the first file went out as.
pub async fn album(
  speaker: &Speaker,
  reader: &ReaderClient,
  chat_id: i64,
  items: &[AlbumItem],
  caption: Option<&str>,
) -> Result<ChatSendResult, ChatFailure> {
  let root = speaker.sending.picture_root.as_deref();
  let caption = caption_of(caption);
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

  for (at, album) in albums(&files, |file| kind_of(&file.mime, &file.name), ALBUM_LIMIT).iter().enumerate()
  {
    let caption = if at == 0 { caption.as_str() } else { "" };

    match reader.owner_album(chat_id, album, caption, None).await? {
      AsOwner::Sent(ids) => {
        went_any = true;
        first = first.or(ids.into_iter().flatten().next());
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

  Ok(ChatSendResult::Sent {
    message_id: first,
    missed,
  })
}
