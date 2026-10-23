//! The reader as the service reaches it over the seam (docs/boundary-api.md, 「The reader's routes, called by
//! the service」): whether the owner's account is connected, the chats it is in, a picture or a file the Bot API
//! will not hand the bot, and a send in the owner's own voice.

use bmo_chat::{ChatAvatar, ChatAvatarKind, FileItem, MessageFile};
use serde_json::{Value, json};

use crate::compose::Part;
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

/// One chat the owner's account is in: the room's number as the service numbers it, and the account's title
/// for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderRoom {
  pub chat_id: i64,
  pub title: Option<String>,
}

/// What a fetch through the owner's account came to: the file, a reason it could not be had, or no account
/// connected to ask, each with the reader's English reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReaderFetch {
  Fetched(MessageFile),
  Unavailable(String),
  NoReader(String),
}

/// What a send in the owner's voice came to: what went, or the name the account refused it by — not connected,
/// or a reply its history cannot find — with the sentence saying why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsOwner<T> {
  Sent(T),
  Refused { name: String, error: String },
}

/// One message a send in the owner's voice went out as: the number the account gave it, where it named one,
/// and the words Telegram holds for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnerSent {
  pub message_id: Option<i64>,
  pub words: String,
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

  /// Every profile picture the owner's account keeps, with when it was taken.
  pub async fn avatars(&self) -> Result<Vec<ChatAvatar>, String> {
    let answer = self.wire.get("/reader/avatars").await?;

    if !answer.ok() {
      return Err(answer.reason());
    }

    let kept = answer
      .body
      .get("avatars")
      .and_then(Value::as_array)
      .ok_or_else(|| "the reader's /reader/avatars answered no list of pictures".to_owned())?;

    kept
      .iter()
      .map(|one| {
        let kind = match one.get("kind").and_then(Value::as_str) {
          Some("room") => ChatAvatarKind::Room,
          Some("person") => ChatAvatarKind::Person,
          other => return Err(format!("the reader answered an unknown picture kind: {other:?}")),
        };
        let id = one
          .get("id")
          .and_then(Value::as_i64)
          .ok_or_else(|| "the reader answered a picture with no number".to_owned())?;
        let taken_at = one
          .get("takenAt")
          .and_then(Value::as_i64)
          .ok_or_else(|| "the reader answered a picture with no moment".to_owned())?;

        Ok(ChatAvatar { kind, id, taken_at })
      })
      .collect()
  }

  /// Every chat the owner's account is in; a reader that does not answer, or answers no list, is the reason.
  pub async fn dialogs(&self) -> Result<Vec<ReaderRoom>, String> {
    let answer = self.wire.get("/reader/dialogs").await?;

    if !answer.ok() {
      return Err(answer.reason());
    }

    let rooms = answer
      .body
      .get("rooms")
      .and_then(Value::as_array)
      .ok_or_else(|| "the reader's /reader/dialogs answered no list of rooms".to_owned())?;

    rooms
      .iter()
      .map(|one| {
        let chat_id = one
          .get("chatId")
          .and_then(Value::as_i64)
          .ok_or_else(|| "the reader answered a room with no number".to_owned())?;
        let title = one.get("title").and_then(Value::as_str).map(str::to_owned);

        Ok(ReaderRoom { chat_id, title })
      })
      .collect()
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

  /// One part of words said in a room in the owner's voice, in its markup where it has one, without a
  /// notification where `silent`, answering the message named: the message it went out as.
  pub async fn owner_say(
    &self,
    chat_id: i64,
    part: &Part,
    silent: bool,
    reply_to: Option<i64>,
  ) -> Result<AsOwner<OwnerSent>, NotCarried> {
    let body = json!({ "chatId": chat_id, "text": part.plain, "markup": part.markup, "silent": silent,
      "replyToMessageId": reply_to });
    let sent = self.as_owner("/owner/say", &body).await?;

    Ok(map_sent(sent, |answer| OwnerSent {
      message_id: answer.get("messageId").and_then(Value::as_i64),
      words: answer.get("text").and_then(Value::as_str).unwrap_or_default().to_owned(),
    }))
  }

  /// One album of files sent in a room in the owner's voice, with its caption and the message it answers: the
  /// message each file went out as, none where the account could not name it.
  pub async fn owner_album(
    &self,
    chat_id: i64,
    files: &[FileItem],
    caption: &Part,
    reply_to: Option<i64>,
  ) -> Result<AsOwner<Vec<OwnerSent>>, NotCarried> {
    let body = json!({ "chatId": chat_id, "files": files, "caption": caption.plain,
      "captionMarkup": caption.markup, "replyToMessageId": reply_to });
    let sent = self.as_owner("/owner/album", &body).await?;

    Ok(map_sent(sent, |answer| {
      let texts = answer.get("texts").and_then(Value::as_array).cloned().unwrap_or_default();

      answer
        .get("ids")
        .and_then(Value::as_array)
        .map(|ids| {
          ids
            .iter()
            .enumerate()
            .map(|(at, id)| OwnerSent {
              message_id: id.as_i64(),
              words: texts.get(at).and_then(Value::as_str).unwrap_or_default().to_owned(),
            })
            .collect()
        })
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
