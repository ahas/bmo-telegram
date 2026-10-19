//! The reader as the service reaches it over the seam (docs/boundary-api.md, 「The reader's routes, called by
//! the service」): whether the owner's account is connected, a picture or a file the Bot API will not hand the
//! bot, and a send in the owner's own voice.

use bmo_chat::{FileItem, MessageFile};
use serde::Serialize;
use serde_json::{Value, json};

use crate::wire::Wire;

/// The reader on the loopback, behind the seam's token.
#[derive(Debug, Clone)]
pub struct ReaderClient {
  wire: Wire,
}

/// Whether the owner's account is connected, and why not where it is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderHealth {
  pub connected: bool,
  pub reason: Option<String>,
}

/// What a fetch through the owner's account came to: the file, a reason it could not be had, or no account
/// connected to ask, each with the reader's English reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaderFetch {
  Fetched(MessageFile),
  Unavailable(String),
  NoReader(String),
}

/// The message a send in the owner's voice answers, as the receiver holds it: its number, when it was said and
/// what it says, which the owner's account finds it in its own history by.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnerReply {
  pub message_id: i64,
  pub sent_at: i64,
  pub text: String,
}

/// What a send in the owner's voice came to: what went, or the name the account refused it by — not connected,
/// or a reply its history cannot find — with the sentence saying why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsOwner<T> {
  Sent(T),
  Refused { name: String, error: String },
}

/// Why the reader did not carry a send at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the reader did not carry {route}: {why}")]
pub struct NotCarried {
  pub route: String,
  pub why: String,
}

impl ReaderClient {
  pub fn new(wire: Wire) -> Self {
    Self { wire }
  }

  /// Whether the owner's account is connected; a reader that does not answer is one with no account to ask.
  pub async fn health(&self) -> ReaderHealth {
    match self.wire.get("/health").await {
      Ok(answer) if answer.ok() => ReaderHealth {
        connected: answer.body.get("connected").and_then(Value::as_bool) == Some(true),
        reason: answer.body.get("reason").and_then(Value::as_str).map(str::to_owned),
      },
      Ok(answer) => ReaderHealth {
        connected: false,
        reason: Some(format!("the reader's health {}", answer.reason())),
      },
      Err(why) => ReaderHealth {
        connected: false,
        reason: Some(why),
      },
    }
  }

  async fn fetch(&self, route: &str, body: &Value) -> ReaderFetch {
    let answer = match self.wire.post(route, body).await {
      Ok(answer) if answer.ok() => answer.body,
      Ok(answer) => return ReaderFetch::Unavailable(answer.reason()),
      Err(why) => return ReaderFetch::Unavailable(why),
    };
    let reason = || answer.get("reason").and_then(Value::as_str).unwrap_or_default().to_owned();

    match answer.get("outcome").and_then(Value::as_str) {
      Some("fetched") => serde_json::from_value(answer.clone())
        .map_or_else(|why| ReaderFetch::Unavailable(why.to_string()), ReaderFetch::Fetched),
      Some("no-reader") => ReaderFetch::NoReader(reason()),
      _ => ReaderFetch::Unavailable(reason()),
    }
  }

  /// A message's picture fetched through the owner's account.
  pub async fn picture(&self, chat_id: i64, message_id: i64, sent_at: Option<i64>) -> ReaderFetch {
    let body = json!({ "chatId": chat_id, "messageId": message_id, "sentAt": sent_at });

    self.fetch("/reader/picture", &body).await
  }

  /// A message's file fetched through the owner's account: one past the Bot API's ceiling for a bot.
  pub async fn file(&self, chat_id: i64, message_id: i64, sent_at: i64) -> ReaderFetch {
    let body = json!({ "chatId": chat_id, "messageId": message_id, "sentAt": sent_at });

    self.fetch("/reader/file", &body).await
  }

  async fn as_owner(&self, route: &str, body: &Value) -> Result<AsOwner<Value>, NotCarried> {
    let not_carried = |why: String| NotCarried {
      route: route.to_owned(),
      why,
    };
    let answer = self.wire.post(route, body).await.map_err(not_carried)?;

    if !answer.ok() {
      return Err(not_carried(answer.reason()));
    }

    Ok(match answer.body.get("refused").and_then(Value::as_str) {
      Some(name) => AsOwner::Refused {
        name: name.to_owned(),
        error: answer.body.get("error").and_then(Value::as_str).unwrap_or(name).to_owned(),
      },
      None => AsOwner::Sent(answer.body),
    })
  }

  /// One piece of words said in a room in the owner's voice, answering the message named: the message it
  /// went out as.
  pub async fn owner_say(
    &self,
    chat_id: i64,
    text: &str,
    reply: Option<&OwnerReply>,
  ) -> Result<AsOwner<Option<i64>>, NotCarried> {
    let body = json!({ "chatId": chat_id, "text": text, "reply": reply });
    let sent = self.as_owner("/owner/say", &body).await?;

    Ok(map_sent(sent, |answer| answer.get("messageId").and_then(Value::as_i64)))
  }

  /// One album of files sent in a room in the owner's voice, with its caption and the message it answers: the
  /// message each file went out as, none where the account could not name it.
  pub async fn owner_album(
    &self,
    chat_id: i64,
    files: &[FileItem],
    caption: &str,
    reply: Option<&OwnerReply>,
  ) -> Result<AsOwner<Vec<Option<i64>>>, NotCarried> {
    let body = json!({ "chatId": chat_id, "files": files, "caption": caption, "reply": reply });
    let sent = self.as_owner("/owner/album", &body).await?;

    Ok(map_sent(sent, |answer| {
      answer
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().map(Value::as_i64).collect())
        .unwrap_or_default()
    }))
  }
}

fn map_sent<T>(sent: AsOwner<Value>, read: impl FnOnce(Value) -> T) -> AsOwner<T> {
  match sent {
    AsOwner::Sent(answer) => AsOwner::Sent(read(answer)),
    AsOwner::Refused { name, error } => AsOwner::Refused { name, error },
  }
}
