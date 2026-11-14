//! The bot account as it speaks (docs/telegram.md, 「Sends」): one send of any verb carried on the Bot API, and
//! a text in the parts the dialect divides it into, each sent as its HTML and again as its plain text where
//! Telegram refuses the markup. What Telegram turns down for what was asked is a refusal under the label; what
//! it could not answer at all is the send's failure.

use std::path::PathBuf;
use std::sync::Arc;

use bmo_chat::{ChatFailure, ChatRefusal, ChatSend, ChatSendResult, ChatSent, ChatSticker};
use serde_json::{Value, json};

use crate::bot_api::{Answered, BotApi, kept_alive};
use crate::compose::text_parts;
use crate::policy::POLICY;
use crate::seen::Seen;

/// What a send came to, or why it could not be performed at all.
pub type Sent = Result<ChatSendResult, ChatFailure>;

/// What the bot account's sends stand on beside the Bot API: the folder a picture named by a path may be read
/// from, and the sticker set a sticker is picked out of, each where the configuration names one.
#[derive(Debug, Clone, Default)]
pub struct Sending {
  pub picture_root: Option<PathBuf>,
  pub sticker_set: Option<String>,
}

/// The bot account as it speaks.
pub struct Speaker {
  pub(crate) api: BotApi,
  pub(crate) sending: Sending,
  pub(crate) web: reqwest::Client,
  seen: Arc<Seen>,
}

/// A send that went, as the message it went out as where there is one.
pub fn sent(message_id: Option<i64>) -> ChatSendResult {
  ChatSendResult::Sent(ChatSent {
    message_id,
    missed: Vec::new(),
  })
}

/// A send refused.
pub fn refused(refusal: ChatRefusal) -> ChatSendResult {
  ChatSendResult::Refused(refusal)
}

/// What a call Telegram did not take comes to: a refusal in Telegram's own words where it turned the call down
/// for what it asked, and the send's failure where it could not answer it — its flood limit, or its own fault.
pub fn turned_down(method: &str, answered: &Answered) -> Sent {
  let description = answered.description.as_deref().unwrap_or("no description");

  match answered.error_code {
    Some(code) if (400..500).contains(&code) && code != 429 => Ok(refused(POLICY.refused(description))),
    code => Err(format!("{method} failed: {description} ({})", code.unwrap_or_default()).into()),
  }
}

/// The message a call went out as, where Telegram took it.
pub fn message_id(answered: &Answered) -> Option<i64> {
  answered
    .result
    .as_ref()
    .filter(|_| answered.ok)
    .and_then(|result| result.get("message_id"))
    .and_then(Value::as_i64)
}

// What Telegram says of a reply target it has no message for.
const REPLY_TARGET_GONE: &str = "message to be replied not found";

impl Speaker {
  pub fn new(api: BotApi, sending: Sending, seen: Arc<Seen>) -> Self {
    Self {
      api,
      sending,
      web: kept_alive(),
      seen,
    }
  }

  /// One method asked with its parameters; a call that reached nobody is the send's failure.
  pub(crate) async fn call(&self, method: &str, body: &Value) -> Result<Answered, ChatFailure> {
    self.api.answered(method, body).await.map_err(|why| format!("{method}: {why}").into())
  }

  /// One method asked with a form, which is how a file goes up.
  pub(crate) async fn call_form(
    &self,
    method: &str,
    form: reqwest::multipart::Form,
  ) -> Result<Answered, ChatFailure> {
    self
      .api
      .answered_form(method, form)
      .await
      .map_err(|why| format!("{method}: {why}").into())
  }

  /// A room sent into, kept in the record of rooms seen.
  pub(crate) fn spoke_in(&self, chat_id: i64) {
    self.seen.room(chat_id);
  }

  /// One send in the bot account's voice, answering the message the send names.
  pub async fn send(&self, chat_id: i64, reply_to: Option<i64>, send: &ChatSend) -> Sent {
    match send {
      ChatSend::Text { text, silent, fold } => self.text(chat_id, text, reply_to, *silent, *fold).await,
      ChatSend::Photo { image_url, caption } => {
        self.photo(chat_id, image_url, caption.as_deref(), reply_to).await
      }
      ChatSend::Album { items, caption } => self.album(chat_id, items, caption.as_deref(), reply_to).await,
      ChatSend::Document { bytes, file_name } => self.document(chat_id, bytes, file_name, reply_to).await,
      ChatSend::Audio { audio, file_name } => self.audio(chat_id, audio, file_name, reply_to).await,
      ChatSend::Voice {
        audio,
        caption,
        transcript,
        ..
      } => self.voice(chat_id, audio, caption.as_deref(), transcript.as_deref(), reply_to).await,
      ChatSend::Sticker(ChatSticker::Emoji { emoji }) => self.sticker(chat_id, emoji).await,
      ChatSend::Sticker(ChatSticker::Item { item }) => self.sticker_item(chat_id, &item.id).await,
      ChatSend::Reaction { message_id, emoji } => self.reaction(chat_id, *message_id, emoji).await,
      ChatSend::Pin {
        message_id,
        on,
        silent,
      } => self.pin(chat_id, *message_id, *on, *silent).await,
      ChatSend::Typing { on } => self.typing(chat_id, *on).await,
      ChatSend::Edit { message_id, text } => self.edit(chat_id, *message_id, text).await,
      ChatSend::Delete { message_id } => self.delete(chat_id, *message_id).await,
    }
  }

  // One message sent, pinned to the message it answers where it names one; where Telegram has no such message
  // it goes out pinned to nothing and reaches the room either way, the wrong number said in the log.
  async fn send_pinned(&self, body: &Value, reply_to: Option<i64>) -> Result<Answered, ChatFailure> {
    let Some(reply_to) = reply_to else {
      return self.call("sendMessage", body).await;
    };
    let mut pinned = body.clone();

    pinned["reply_parameters"] = json!({ "message_id": reply_to });

    let answered = self.call("sendMessage", &pinned).await?;

    if answered.ok || !answered.description.as_deref().is_some_and(|said| said.contains(REPLY_TARGET_GONE))
    {
      return Ok(answered);
    }

    eprintln!(
      "telegram: chat {}: no message {reply_to} to reply to; the answer went out unpinned",
      body.get("chat_id").and_then(Value::as_i64).unwrap_or_default()
    );

    self.call("sendMessage", body).await
  }

  /// A text sent to its room in the parts it divides into, the first pinned to the message it answers, each in
  /// its markup and as its plain text where Telegram refuses the markup, without a notification where it is
  /// silent; the number of the first part, which is the one a caller edits or pins. A part that fails ends the
  /// send there: a later part standing without the one before it reads as another answer.
  pub async fn text(
    &self,
    chat_id: i64,
    markdown: &str,
    reply_to: Option<i64>,
    silent: bool,
    fold: bool,
  ) -> Sent {
    let mut first: Option<i64> = None;

    for part in text_parts(markdown, fold) {
      let pinned = if first.is_none() { reply_to } else { None };
      let mut plain = json!({ "chat_id": chat_id, "text": part.plain });

      if silent {
        plain["disable_notification"] = json!(true);
      }

      let answered = match &part.markup {
        None => self.send_pinned(&plain, pinned).await?,
        Some(markup) => {
          let mut marked = plain.clone();

          marked["text"] = json!(markup);
          marked["parse_mode"] = json!("HTML");

          let answered = self.send_pinned(&marked, pinned).await?;

          if message_id(&answered).is_some() {
            answered
          } else {
            self.send_pinned(&plain, pinned).await?
          }
        }
      };
      let Some(went) = message_id(&answered) else {
        return turned_down("sendMessage", &answered);
      };

      first.get_or_insert(went);
      self.spoke_in(chat_id);
    }

    Ok(sent(first))
  }
}
