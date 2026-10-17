//! The bot account's files (docs/telegram.md, 「Sends」): what a sound's bytes are, read off their first bytes —
//! the extension it is named with and the type it goes up under; Telegram wants OGG/Opus for a voice, which is
//! what a synthesiser writes — and a document, an audio file and a voice sent, the voice with the reading
//! actually spoken folded under it.

use std::time::Duration;

use regex::RegexBuilder;
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

fn file_part(bytes: &[u8], name: &str, mime: &str) -> Result<Field, bmo_chat::ChatFailure> {
  Ok(Field::bytes(bytes.to_vec()).file_name(name.to_owned()).mime_str(mime)?)
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
  pub async fn document(&self, chat_id: i64, bytes: &[u8], file_name: &str, reply_to: Option<i64>) -> Sent {
    let field = file_part(bytes, file_name, "application/octet-stream")?;

    self.file("sendDocument", chat_id, field, "document", reply_to).await
  }

  /// A sound sent as a file the room can play and download, under the type its bytes read as.
  pub async fn audio(&self, chat_id: i64, audio: &[u8], file_name: &str, reply_to: Option<i64>) -> Sent {
    let (_, mime) = audio_type(audio);
    let field = file_part(audio, file_name, mime)?;

    self.file("sendAudio", chat_id, field, "audio", reply_to).await
  }

  /// A spoken line as a voice message, with the first part of the reading spoken as its caption and the rest as
  /// quiet replies under it — its own caption the room reads only where no reading takes its place. A reading
  /// that did not all go under a voice that went is the send's failure, naming the voice that went.
  pub async fn voice(
    &self,
    chat_id: i64,
    audio: &[u8],
    caption: Option<&str>,
    transcript: Option<&str>,
    reply_to: Option<i64>,
  ) -> Sent {
    let (extension, mime) = audio_type(audio);
    let file_name = format!("voice.{extension}");
    let mut parts = transcript.map(transcript_parts).unwrap_or_default();
    let first = (!parts.is_empty()).then(|| parts.remove(0));
    let shown = caption.filter(|caption| first.is_none() && !caption.trim().is_empty());
    let form = |captioned: bool| -> Result<Form, bmo_chat::ChatFailure> {
      let mut form = Form::new().text("chat_id", chat_id.to_string());

      match (&first, shown) {
        (Some(first), _) if captioned => {
          form = form.text("caption", first.clone()).text("parse_mode", "HTML");
        }
        (None, Some(shown)) => form = form.text("caption", shown.to_owned()),
        _ => {}
      }

      Ok(reply_field(form.part("voice", file_part(audio, &file_name, mime)?), reply_to))
    };
    let mut answered = self.call_form("sendVoice", form(true)?).await?;

    // Only an explicit refusal of the caption is safe to try again: a voice that may already have gone must not
    // go twice.
    if let Some(first) = &first
      && !answered.ok
      && answered.error_code == Some(400)
      && caption_refused(answered.description.as_deref().unwrap_or_default())
    {
      parts.insert(0, first.clone());
      answered = self.call_form("sendVoice", form(false)?).await?;
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
  // flood limit; Telegram then names the wait, and only that part is asked again after it, twice at most.
  async fn transcript_part(&self, chat_id: i64, voice: i64, markup: &str) -> Result<(), String> {
    let body = json!({
      "chat_id": chat_id,
      "text": markup,
      "parse_mode": "HTML",
      "reply_parameters": { "message_id": voice },
      "disable_notification": true,
      "link_preview_options": { "is_disabled": true },
    });
    let mut attempt = 0;

    loop {
      let answered: Answered = self.call("sendMessage", &body).await.map_err(|why| why.to_string())?;
      let wait = answered.parameters.as_ref().and_then(|parameters| parameters.retry_after);

      match wait {
        Some(wait) if !answered.ok && answered.error_code == Some(429) && attempt < 2 => {
          tokio::time::sleep(Duration::from_secs(wait)).await;
          attempt += 1;
        }
        _ if message_id(&answered).is_some() => return Ok(()),
        _ => {
          return Err(answered.description.unwrap_or_else(|| "no transcript message returned".to_owned()));
        }
      }
    }
  }
}
