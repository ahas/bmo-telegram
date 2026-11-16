//! The bot account's files (docs/telegram.md, 「Sends」): what a sound's bytes are, read off their first bytes —
//! the extension it is named with and the type it goes up under; Telegram wants OGG/Opus for a voice, which is
//! what a synthesiser writes — and a document, an audio file and a voice sent, the voice with the reading
//! actually spoken folded under it.

use std::time::Duration;

use bmo_chat::FileBody;
use regex::RegexBuilder;
use reqwest::Body;
use reqwest::multipart::{Form, Part as Field};
use serde_json::json;
use telegram_dialect::telegram::transcript_parts;

use crate::bot_api::Answered;
use crate::policy::POLICY;
use crate::send::{Sent, Speaker, message_id, sent, turned_down};

/// The extension and the type of a sound, by its first bytes; OGG where none other matches.
pub fn audio_type(bytes: &[u8]) -> (&'static str, &'static str) {
  if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE".as_slice()) {
    return ("wav", "audio/wav");
  }

  if bytes.starts_with(b"ID3")
    || (bytes.first() == Some(&0xff) && bytes.get(1).is_some_and(|byte| byte & 0xe0 == 0xe0))
  {
    return ("mp3", "audio/mpeg");
  }

  if bytes.get(4..8) == Some(b"ftyp".as_slice()) {
    return ("m4a", "audio/mp4");
  }

  if bytes.starts_with(b"fLaC") {
    return ("flac", "audio/flac");
  }

  ("ogg", "audio/ogg")
}

/// Whether Telegram's description of a refused voice says it will not take the caption.
pub fn caption_refused(description: &str) -> bool {
  RegexBuilder::new(&POLICY.telegram.caption_refused_descriptions.join("|"))
    .case_insensitive(true)
    .build()
    .is_ok_and(|refused| refused.is_match(description))
}

// The message a file answers, where it answers one: a message deleted in the meantime leaves nothing to reply
// to, and the file is still wanted.
fn reply_field(form: Form, reply_to: Option<i64>) -> Form {
  match reply_to {
    Some(message_id) => form.text(
      "reply_parameters",
      json!({ "message_id": message_id, "allow_sending_without_reply": true }).to_string(),
    ),
    None => form,
  }
}

// One voice send, in the shape its two tries share: the file, the caption it may carry, the message it answers.
struct Voiced<'a> {
  chat_id: i64,
  audio: &'a FileBody,
  file_name: &'a str,
  mime: &'a str,
  first: Option<&'a str>,
  shown: Option<&'a str>,
  reply_to: Option<i64>,
}

// The form one try of a voice goes up in, its caption read as the try asks for one or as the voice's own.
async fn voice_form(voice: &Voiced<'_>, captioned: bool) -> Result<Form, bmo_chat::ChatFailure> {
  let mut form = Form::new().text("chat_id", voice.chat_id.to_string());

  match (voice.first, voice.shown, captioned) {
    (Some(first), _, true) => form = form.text("caption", first.to_owned()).text("parse_mode", "HTML"),
    (None, Some(shown), _) => form = form.text("caption", shown.to_owned()),
    _ => {}
  }

  let field = file_part(voice.audio, voice.file_name, voice.mime).await?;

  Ok(reply_field(form.part("voice", field), voice.reply_to))
}

// The field a file goes up as: bytes in memory as they are, a file kept on this machine's disk streamed off it
// while it goes up, so a large one is never held whole (docs/telegram.md, 「Sends」). A file named without its
// bytes is refused with the reason it was named for.
async fn file_part(body: &FileBody, name: &str, mime: &str) -> Result<Field, bmo_chat::ChatFailure> {
  let refused = |why: std::io::Error| -> bmo_chat::ChatFailure {
    format!("{name} was not read for sending: {why}").into()
  };
  let size = body.size().await.map_err(refused)?;
  let part = match body {
    FileBody::Inline(bytes) => Field::bytes(bytes.clone()),
    FileBody::Stored(stored) => match tokio::fs::File::open(stored.path()).await {
      Err(why) => return Err(refused(why)),
      Ok(file) => Field::stream_with_length(Body::from(file), size),
    },
    // The size above refused it already, since a file with no bytes has no size to tell.
    FileBody::Withheld => {
      return Err(refused(std::io::Error::other("the file was named without its bytes")));
    }
  };

  Ok(part.file_name(name.to_owned()).mime_str(mime)?)
}

// A sound's first bytes, read wherever the sound stands: enough of them to tell one type from another, and no
// more of a file kept on disk than the few bytes asked for.
const SOUND_HEAD: usize = 12;

pub(crate) async fn sound_head(body: &FileBody) -> Vec<u8> {
  if let FileBody::Inline(bytes) = body {
    return bytes.iter().take(SOUND_HEAD).copied().collect();
  }

  let Ok(mut reader) = body.reader().await else {
    return Vec::new();
  };
  let mut head = Vec::with_capacity(SOUND_HEAD);

  match tokio::io::AsyncReadExt::read_buf(&mut reader, &mut head).await {
    Ok(_) => head,
    Err(_) => Vec::new(),
  }
}

impl Speaker {
  // A file sent as `field` by `method`: the message it went out as, or Telegram's refusal.
  async fn file(
    &self,
    method: &str,
    chat_id: i64,
    field: Field,
    name: &'static str,
    reply_to: Option<i64>,
  ) -> Sent {
    let form = reply_field(Form::new().text("chat_id", chat_id.to_string()), reply_to).part(name, field);
    let answered = self.call_form(method, form).await?;
    let Some(went) = message_id(&answered) else {
      return turned_down(method, &answered);
    };

    self.spoke_in(chat_id);

    Ok(sent(Some(went)))
  }

  /// A file sent as a document under its own name.
  pub async fn document(
    &self,
    chat_id: i64,
    bytes: &FileBody,
    file_name: &str,
    reply_to: Option<i64>,
  ) -> Sent {
    let field = file_part(bytes, file_name, "application/octet-stream").await?;

    self.file("sendDocument", chat_id, field, "document", reply_to).await
  }

  /// A sound sent as a file the room can play and download, under the type its bytes read as.
  pub async fn audio(
    &self,
    chat_id: i64,
    audio: &FileBody,
    file_name: &str,
    reply_to: Option<i64>,
  ) -> Sent {
    let (_, mime) = audio_type(&sound_head(audio).await);
    let field = file_part(audio, file_name, mime).await?;

    self.file("sendAudio", chat_id, field, "audio", reply_to).await
  }

  /// A spoken line as a voice message, with the first part of the reading spoken as its caption and the rest as
  /// quiet replies under it — its own caption the room reads only where no reading takes its place. A reading
  /// that did not all go under a voice that went is the send's failure, naming the voice that went.
  pub async fn voice(
    &self,
    chat_id: i64,
    audio: &FileBody,
    caption: Option<&str>,
    transcript: Option<&str>,
    reply_to: Option<i64>,
  ) -> Sent {
    let (extension, mime) = audio_type(&sound_head(audio).await);
    let file_name = format!("voice.{extension}");
    let mut parts = transcript.map(transcript_parts).unwrap_or_default();
    let first = (!parts.is_empty()).then(|| parts.remove(0));
    let shown = caption.filter(|caption| first.is_none() && !caption.trim().is_empty());
    let voiced = Voiced {
      chat_id,
      audio,
      file_name: &file_name,
      mime,
      first: first.as_deref(),
      shown,
      reply_to,
    };
    let mut answered = self.call_form("sendVoice", voice_form(&voiced, true).await?).await?;

    // Only an explicit refusal of the caption is safe to try again: a voice that may already have gone must not
    // go twice.
    if let Some(first) = &first
      && !answered.ok
      && answered.error_code == Some(400)
      && caption_refused(answered.description.as_deref().unwrap_or_default())
    {
      parts.insert(0, first.clone());
      answered = self.call_form("sendVoice", voice_form(&voiced, false).await?).await?;
    }

    let Some(voice) = message_id(&answered) else {
      return turned_down("sendVoice", &answered);
    };

    self.spoke_in(chat_id);

    for part in parts {
      if let Err(why) = self.transcript_part(chat_id, voice, &part).await {
        return Err(format!("voice {voice} was sent; transcript continuation failed: Error: {why}").into());
      }
    }

    Ok(sent(Some(voice)))
  }

  // One part of a voice's reading, sent quietly as a reply to the voice. A long reading can meet the room's
  // flood limit; Telegram then names the wait, and only that part is asked again after it, as often as Telegram
  // names one, until it goes.
  async fn transcript_part(&self, chat_id: i64, voice: i64, markup: &str) -> Result<(), String> {
    let body = json!({
      "chat_id": chat_id,
      "text": markup,
      "parse_mode": "HTML",
      "reply_parameters": { "message_id": voice },
      "disable_notification": true,
      "link_preview_options": { "is_disabled": true },
    });
    loop {
      let answered: Answered = self.call("sendMessage", &body).await.map_err(|why| why.to_string())?;
      let wait = answered.parameters.as_ref().and_then(|parameters| parameters.retry_after);

      match wait {
        Some(wait) if !answered.ok && answered.error_code == Some(429) => {
          tokio::time::sleep(Duration::from_secs(wait)).await;
        }
        _ if message_id(&answered).is_some() => return Ok(()),
        _ => {
          return Err(answered.description.unwrap_or_else(|| "no transcript message returned".to_owned()));
        }
      }
    }
  }
}
