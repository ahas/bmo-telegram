//! The bot account's pictures (docs/telegram.md, 「Sends」): a picture and an album, by address where Telegram
//! fetches it and uploaded where it will not — a file under the picture root and a picture handed over inline
//! go only uploaded — their caption in the dialect's markup, and a picture that did not go handed back with its
//! reason. The bytes an upload carries are read and redrawn by `telegram-picture`.

use std::path::Path;
use std::time::Duration;

use bmo_chat::{
  AlbumItem, ChatRefusal, ChatSendResult, ChatSent, FileBody, FileItem, MissedPicture, MissedPictureReason,
};
use reqwest::multipart::{Form, Part as Field};
use serde::Serialize;
use serde_json::{Value, json};
use telegram_dialect::ALBUM_LIMIT;
use telegram_picture::{for_upload, raster_type};

use crate::bot_api::Answered;
use crate::compose::{Part, caption};
use crate::policy::POLICY;
use crate::send::{Sent, Speaker, message_id, refused};

/// Whether an address is one a reader in the room can open: a page on the web, and nothing on this machine.
pub fn is_web_address(address: &str) -> bool {
  url::Url::parse(address).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Whether an address is a file this machine may send: only one whose real path sits under the picture root,
/// never whatever path a caller named and never a symbolic link reaching past the root. With no root
/// configured, no file may go.
pub fn is_kept(root: Option<&Path>, address: &str) -> bool {
  let path = Path::new(address);
  let Some(root) = root else {
    return false;
  };

  if !path.is_absolute() {
    return false;
  }

  // Canonicalizing both reads every symbolic link and every `.` and `..` on the way, so the check is made on
  // where the file really stands, not on the characters a caller wrote.
  let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize()) else {
    return false;
  };

  path.starts_with(root) && path.is_file()
}

/// Why a picture named by an address cannot be sent at all, asked before anything reaches the platform; none
/// for one that may go.
pub fn unsendable(root: Option<&Path>, address: &str) -> Option<MissedPictureReason> {
  if is_web_address(address) || is_kept(root, address) {
    return None;
  }

  if !Path::new(address).is_absolute() {
    return Some(MissedPictureReason::NotAnAddress);
  }

  Some(if Path::new(address).exists() {
    MissedPictureReason::OutsideDataRoot
  } else {
    MissedPictureReason::MissingFile
  })
}

/// Pictures split into albums of at most Telegram's limit, as evenly as that allows, so no album is left with
/// a single picture — Telegram takes an album of two or more.
pub fn groups<T>(pictures: &[T]) -> Vec<&[T]> {
  let count = pictures.len().div_ceil(ALBUM_LIMIT);
  let size = pictures.len().div_ceil(count.max(1)).max(1);

  pictures.chunks(size).collect()
}

/// The wait a refusal asks for, where it is the flood limit's: a run of albums meets that limit where one
/// message never does, and the album is asked again after the wait Telegram names, however long.
pub fn flood_wait(answered: &Answered) -> Option<Duration> {
  let wait = answered.parameters.as_ref().and_then(|parameters| parameters.retry_after)?;

  (!answered.ok && answered.error_code == Some(429)).then(|| Duration::from_secs(wait))
}

/// One picture a send may carry: by the address it was given as, read from the web, from a file under the
/// picture root, or from the bytes handed over with it.
#[derive(Debug, Clone)]
pub struct Picture {
  pub address: String,
  source: Source,
}

#[derive(Debug, Clone)]
enum Source {
  Web,
  Kept,
  Inline { bytes: FileBody, mime: String },
}

impl Picture {
  /// The picture an address names, or why it cannot go.
  pub fn named(root: Option<&Path>, address: &str) -> Result<Self, MissedPicture> {
    if let Some(reason) = unsendable(root, address) {
      return Err(MissedPicture {
        address: address.to_owned(),
        reason,
      });
    }

    Ok(Self {
      address: address.to_owned(),
      source: if is_web_address(address) { Source::Web } else { Source::Kept },
    })
  }

  /// The picture an album item names: an address, or the file itself, known by its name.
  pub fn of(root: Option<&Path>, item: &AlbumItem) -> Result<Self, MissedPicture> {
    match item {
      AlbumItem::Address { address } => Self::named(root, address),
      AlbumItem::File(FileItem { name, mime, bytes }) => Ok(Self {
        address: name.clone(),
        source: Source::Inline {
          bytes: bytes.clone(),
          mime: mime.clone(),
        },
      }),
    }
  }

  /// Whether Telegram can fetch the picture for itself, which only a web address lets it.
  fn by_address(&self) -> bool {
    matches!(self.source, Source::Web)
  }
}

/// A picture's bytes as Telegram takes a photo: their type, and the type the picture came as.
#[derive(Debug)]
pub struct Redrawn {
  pub bytes: Vec<u8>,
  pub kind: String,
  pub served: String,
}

// A caption as one try sends it: its words and whether they are HTML.
#[derive(Clone, Copy)]
struct Captioned<'a> {
  text: Option<&'a str>,
  html: bool,
}

impl Captioned<'_> {
  fn fields(&self) -> Vec<(&'static str, String)> {
    let Some(text) = self.text else {
      return Vec::new();
    };
    let mut fields = vec![("caption", text.to_owned())];

    if self.html {
      fields.push(("parse_mode", "HTML".to_owned()));
    }

    fields
  }
}

// The tries a caption is sent in: its marks where it carries any, then its plain text.
fn tries(caption: Option<&Part>) -> Vec<Captioned<'_>> {
  let marked = caption.and_then(|caption| caption.markup.as_deref()).map(|markup| Captioned {
    text: Some(markup),
    html: true,
  });

  marked
    .into_iter()
    .chain([Captioned {
      text: caption.map(|caption| caption.plain.as_str()),
      html: false,
    }])
    .collect()
}

// The ways a picture goes up, in the order they are tried: by address, which Telegram fetches for itself, then
// uploaded; a picture Telegram cannot reach goes only uploaded.
fn ways(by_address: bool) -> &'static [bool] {
  if by_address { &[false, true] } else { &[true] }
}

// The message a picture answers, where it answers one: a message deleted in the meantime leaves nothing to
// reply to, and the picture is still wanted.
fn reply_parameters(reply_to: i64) -> Value {
  json!({ "message_id": reply_to, "allow_sending_without_reply": true })
}

fn reply_field(form: Form, reply_to: Option<i64>) -> Form {
  match reply_to {
    Some(reply_to) => form.text("reply_parameters", reply_parameters(reply_to).to_string()),
    None => form,
  }
}

// One picture of an album in the media list it goes up in.
#[derive(Serialize)]
struct Media<'a> {
  #[serde(rename = "type")]
  kind: &'static str,
  media: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  caption: Option<&'a str>,
  #[serde(skip_serializing_if = "Option::is_none")]
  parse_mode: Option<&'static str>,
}

impl<'a> Media<'a> {
  fn of(media: String, captioned: Option<Captioned<'a>>) -> Self {
    let caption = captioned.and_then(|captioned| captioned.text);

    Self {
      kind: "photo",
      media,
      caption,
      parse_mode: captioned.filter(|captioned| captioned.html && caption.is_some()).map(|_| "HTML"),
    }
  }
}

// A picture as a form's file, under the name Telegram is told.
fn file_field(redrawn: Redrawn, name: String) -> Result<Field, String> {
  Field::bytes(redrawn.bytes)
    .file_name(name)
    .mime_str(&redrawn.kind)
    .map_err(|why| why.to_string())
}

// Why one try of an album did not go, and the wait Telegram asked for where its flood limit refused it.
struct Tried {
  why: String,
  wait: Option<Duration>,
}

impl From<String> for Tried {
  fn from(why: String) -> Self {
    Self { why, wait: None }
  }
}

// The messages an album went out as, or the refusal.
fn album_messages(answered: &Answered) -> Result<Vec<i64>, Tried> {
  let Some(result) = answered.result.as_ref().filter(|_| answered.ok) else {
    return Err(Tried {
      why: answered
        .description
        .clone()
        .unwrap_or_else(|| "sendMediaGroup refused the album".to_owned()),
      wait: flood_wait(answered),
    });
  };

  Ok(
    result
      .as_array()
      .map(|messages| {
        messages
          .iter()
          .filter_map(|message| message.get("message_id").and_then(Value::as_i64))
          .collect()
      })
      .unwrap_or_default(),
  )
}

// Every picture of a send missed, refused as no picture sent.
fn none_went(missed: Vec<MissedPicture>) -> ChatSendResult {
  refused(ChatRefusal::NoPictureSent {
    label: POLICY.telegram.label.clone(),
    missed,
  })
}

impl Speaker {
  /// The bytes behind a picture with what they are: a kept file's own first bytes say what it is, since
  /// nothing labelled it on the way in; a web address says what it served; a picture handed over says what it
  /// is beside its bytes.
  pub async fn picture_bytes(&self, picture: &Picture) -> Result<(Vec<u8>, String), String> {
    let address = &picture.address;
    let unreached = || format!("image download failed: {address}");

    match &picture.source {
      // A picture is drawn whole before it goes up, wherever its bytes were kept.
      Source::Inline { bytes, mime } => {
        let bytes = bytes.read().await.map_err(|_| unreached())?;

        Ok((bytes, mime.clone()))
      }
      Source::Kept => {
        let bytes = tokio::fs::read(address).await.map_err(|_| unreached())?;
        let kind = raster_type(&bytes).unwrap_or("application/octet-stream").to_owned();

        Ok((bytes, kind))
      }
      Source::Web => {
        let answer = self.web.get(address).send().await.map_err(|_| unreached())?;

        if !answer.status().is_success() {
          return Err(unreached());
        }

        let kind = answer
          .headers()
          .get(reqwest::header::CONTENT_TYPE)
          .and_then(|kind| kind.to_str().ok())
          .unwrap_or("application/octet-stream")
          .split(';')
          .next()
          .unwrap_or_default()
          .trim()
          .to_owned();
        let bytes = answer.bytes().await.map_err(|_| unreached())?;

        Ok((bytes.to_vec(), kind))
      }
    }
  }

  /// A picture's bytes as Telegram takes a photo, fetched or read and redrawn where they must be, or the fault
  /// that kept them from being had.
  pub async fn upload(&self, picture: &Picture) -> Result<Redrawn, String> {
    let (bytes, served) = self.picture_bytes(picture).await?;
    let asked = served.clone();
    let quality = POLICY.pictures.upload_quality;
    let redrawn = tokio::task::spawn_blocking(move || for_upload(bytes, &asked, quality))
      .await
      .map_err(|why| why.to_string())?;
    let Some((bytes, kind)) = redrawn else {
      return Err(format!("not a picture: {served}"));
    };

    Ok(Redrawn {
      bytes,
      kind,
      served,
    })
  }

  /// A picture sent to its room with its caption: the message it went out as, or the picture refused with why
  /// it did not go, its caption and web address said in its place.
  pub async fn photo(
    &self,
    chat_id: i64,
    address: &str,
    markdown: Option<&str>,
    reply_to: Option<i64>,
  ) -> Sent {
    let picture = match Picture::named(self.sending.picture_root.as_deref(), address) {
      Ok(picture) => picture,
      Err(missed) => return Ok(none_went(vec![missed])),
    };
    let part = markdown.map(caption);

    match self.deliver_photo(chat_id, &picture, part.as_ref(), reply_to).await {
      Ok(went) => {
        self.spoke_in(chat_id);

        Ok(ChatSendResult::Sent(ChatSent {
          message_id: Some(went),
          missed: Vec::new(),
        }))
      }
      Err(fault) => {
        self.fall_back(chat_id, markdown, &[&picture]).await?;

        Ok(none_went(vec![MissedPicture {
          address: address.to_owned(),
          reason: POLICY.picture_failed(fault.as_deref()),
        }]))
      }
    }
  }

  // A picture tried every way it may go, with its caption's markup and then its plain text: the message it
  // went out as, or why the last try failed, none where Telegram did not say.
  async fn deliver_photo(
    &self,
    chat_id: i64,
    picture: &Picture,
    caption: Option<&Part>,
    reply_to: Option<i64>,
  ) -> Result<i64, Option<String>> {
    let mut fault: Option<String> = None;

    for captioned in tries(caption) {
      for &uploaded in ways(picture.by_address()) {
        let tried = if uploaded {
          self.photo_uploaded(chat_id, picture, captioned, reply_to).await
        } else {
          self.photo_by_address(chat_id, &picture.address, captioned, reply_to).await
        };

        match tried {
          Ok(answered) => match message_id(&answered) {
            Some(went) => return Ok(went),
            None => fault = answered.description,
          },
          Err(why) => fault = Some(why),
        }
      }
    }

    Err(fault)
  }

  async fn photo_by_address(
    &self,
    chat_id: i64,
    address: &str,
    captioned: Captioned<'_>,
    reply_to: Option<i64>,
  ) -> Result<Answered, String> {
    let mut body = json!({ "chat_id": chat_id, "photo": address });

    for (name, value) in captioned.fields() {
      body[name] = json!(value);
    }

    if let Some(reply_to) = reply_to {
      body["reply_parameters"] = reply_parameters(reply_to);
    }

    self.call("sendPhoto", &body).await.map_err(|why| why.to_string())
  }

  async fn photo_uploaded(
    &self,
    chat_id: i64,
    picture: &Picture,
    captioned: Captioned<'_>,
    reply_to: Option<i64>,
  ) -> Result<Answered, String> {
    let redrawn = self.upload(picture).await?;
    let mut form = Form::new().text("chat_id", chat_id.to_string());

    for (name, value) in captioned.fields() {
      form = form.text(name, value);
    }

    form = reply_field(form, reply_to).part("photo", file_field(redrawn, "photo.jpg".to_owned())?);

    self.call_form("sendPhoto", form).await.map_err(|why| why.to_string())
  }

  /// Several pictures as one album — or as consecutive albums past Telegram's limit — with the caption on the
  /// first picture; one picture goes as a photo. A picture is never sent alone out of an album: what cannot go
  /// in one is missed with why, and an album none of which goes is said as its caption and web addresses in
  /// text, the way a single picture falls back. The message the first album went out as.
  pub async fn album(
    &self,
    chat_id: i64,
    items: &[AlbumItem],
    markdown: Option<&str>,
    reply_to: Option<i64>,
  ) -> Sent {
    let root = self.sending.picture_root.as_deref();
    let mut missed: Vec<MissedPicture> = Vec::new();
    let mut sendable: Vec<Picture> = Vec::new();
    let mut first: Option<i64> = None;
    let mut went_any = false;
    let part = markdown.map(caption);

    for item in items {
      match Picture::of(root, item) {
        Ok(picture) => sendable.push(picture),
        Err(one) => missed.push(one),
      }
    }

    if let [one] = sendable.as_slice() {
      match self.deliver_photo(chat_id, one, part.as_ref(), reply_to).await {
        Ok(went) => {
          first = Some(went);
          went_any = true;
        }
        Err(fault) => {
          self.fall_back(chat_id, markdown, &[one]).await?;
          missed.push(MissedPicture {
            address: one.address.clone(),
            reason: POLICY.picture_failed(fault.as_deref()),
          });
        }
      }
    } else if !sendable.is_empty() {
      for (index, group) in groups(&sendable).into_iter().enumerate() {
        // The caption and the reply belong to the first album only; the ones after it continue the same
        // pictures.
        let caption = part.as_ref().filter(|_| index == 0);
        let reply_to = reply_to.filter(|_| index == 0);

        match self.deliver_album(chat_id, group, caption, reply_to).await {
          Ok((messages, went)) => {
            missed.extend(group.iter().enumerate().filter(|(at, _)| !went.contains(at)).map(
              |(_, picture)| MissedPicture {
                address: picture.address.clone(),
                reason: POLICY.picture_left_out(),
              },
            ));
            first = first.or(messages.first().copied());
            went_any = true;
          }
          Err(fault) => {
            let shown: Vec<&Picture> = group.iter().collect();

            self.fall_back(chat_id, markdown.filter(|_| index == 0), &shown).await?;
            missed.extend(group.iter().map(|picture| MissedPicture {
              address: picture.address.clone(),
              reason: POLICY.picture_failed(fault.as_deref()),
            }));
          }
        }
      }
    }

    if went_any {
      self.spoke_in(chat_id);
    }

    if !went_any && !missed.is_empty() {
      return Ok(none_went(missed));
    }

    Ok(ChatSendResult::Sent(ChatSent {
      message_id: first,
      missed,
    }))
  }

  // One album tried every way it may go, a flood limit's wait taken as asked: the messages it went out as and
  // which of its pictures went, or why the last try failed.
  async fn deliver_album(
    &self,
    chat_id: i64,
    group: &[Picture],
    caption: Option<&Part>,
    reply_to: Option<i64>,
  ) -> Result<(Vec<i64>, Vec<usize>), Option<String>> {
    let by_address = group.iter().all(Picture::by_address);
    let mut fault: Option<String> = None;

    for captioned in tries(caption) {
      for &uploaded in ways(by_address) {
        loop {
          let tried = if uploaded {
            self.album_uploaded(chat_id, group, captioned, reply_to).await
          } else {
            self.album_by_address(chat_id, group, captioned, reply_to).await
          };

          match tried {
            Ok(went) => return Ok(went),
            Err(Tried {
              wait: Some(wait), ..
            }) => tokio::time::sleep(wait).await,
            Err(tried) => {
              fault = Some(tried.why);
              break;
            }
          }
        }
      }
    }

    Err(fault)
  }

  // The album sent by address: Telegram fetches every picture itself, and one it cannot fetch fails the whole
  // album without saying which.
  async fn album_by_address(
    &self,
    chat_id: i64,
    group: &[Picture],
    captioned: Captioned<'_>,
    reply_to: Option<i64>,
  ) -> Result<(Vec<i64>, Vec<usize>), Tried> {
    let media: Vec<Media> = group
      .iter()
      .enumerate()
      .map(|(index, picture)| Media::of(picture.address.clone(), (index == 0).then_some(captioned)))
      .collect();
    let mut body = json!({ "chat_id": chat_id, "media": media });

    if let Some(reply_to) = reply_to {
      body["reply_parameters"] = reply_parameters(reply_to);
    }

    let answered = self.call("sendMediaGroup", &body).await.map_err(|why| why.to_string())?;

    Ok((album_messages(&answered)?, (0..group.len()).collect()))
  }

  // The album sent as uploaded bytes: every picture is redrawn first, so one that cannot be fetched, or is not
  // a picture, is left out and the rest still go as one album.
  async fn album_uploaded(
    &self,
    chat_id: i64,
    group: &[Picture],
    captioned: Captioned<'_>,
    reply_to: Option<i64>,
  ) -> Result<(Vec<i64>, Vec<usize>), Tried> {
    let mut fetched: Vec<(usize, Redrawn)> = Vec::new();

    for (at, picture) in group.iter().enumerate() {
      // An album takes only what came as a picture; the rest is missed by the caller.
      if let Ok(redrawn) = self.upload(picture).await
        && redrawn.served.starts_with("image/")
      {
        fetched.push((at, redrawn));
      }
    }

    let mut form = reply_field(Form::new().text("chat_id", chat_id.to_string()), reply_to);

    if fetched.len() == 1 {
      let (at, redrawn) = fetched.remove(0);

      for (name, value) in captioned.fields() {
        form = form.text(name, value);
      }

      form = form.part("photo", file_field(redrawn, "photo.jpg".to_owned())?);

      let answered = self.call_form("sendPhoto", form).await.map_err(|why| why.to_string())?;

      return match message_id(&answered) {
        Some(went) => Ok((vec![went], vec![at])),
        None => Err(Tried {
          wait: flood_wait(&answered),
          why: answered.description.unwrap_or_else(|| "sendPhoto refused the picture".to_owned()),
        }),
      };
    }

    if fetched.is_empty() {
      return Err("no picture of the album could be fetched".to_owned().into());
    }

    let media: Vec<Media> = (0..fetched.len())
      .map(|index| Media::of(format!("attach://p{index}"), (index == 0).then_some(captioned)))
      .collect();
    let media = serde_json::to_string(&media).map_err(|why| why.to_string())?;
    let mut went = Vec::new();

    form = form.text("media", media);

    for (index, (at, redrawn)) in fetched.into_iter().enumerate() {
      form = form.part(format!("p{index}"), file_field(redrawn, format!("p{index}.jpg"))?);
      went.push(at);
    }

    let answered = self.call_form("sendMediaGroup", form).await.map_err(|why| why.to_string())?;

    Ok((album_messages(&answered)?, went))
  }

  // What a failed send falls back to saying: the caption with the web addresses, which are still the pictures
  // to whoever reads them, as one text. A path on this machine and a picture handed over are nothing to a
  // room, so where no web address is left nothing is said.
  async fn fall_back(
    &self,
    chat_id: i64,
    markdown: Option<&str>,
    pictures: &[&Picture],
  ) -> Result<(), bmo_chat::ChatFailure> {
    let shown: Vec<&str> = pictures
      .iter()
      .filter(|picture| picture.by_address())
      .map(|picture| picture.address.as_str())
      .collect();

    if shown.is_empty() {
      return Ok(());
    }

    let text = markdown.into_iter().chain(shown).collect::<Vec<_>>().join("\n");

    self.text(chat_id, &text, None, false, false).await.map(drop)
  }
}
