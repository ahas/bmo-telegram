//! The Telegram provider as bmo-chat's contract states one (docs/telegram.md, 「The declaration」): what it
//! declares, the room questions, a send in the voice it asks for, and a message's picture or file fetched again
//! through the owner's account. The bot account reads and speaks; the reader stands behind it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use bmo_chat::{
  ChatCapability, ChatDialect, ChatFailure, ChatHealth, ChatMediaRequest, ChatMediaResult,
  ChatModeStanding, ChatNumberSpace, ChatNumbering, ChatProvider, ChatReach, ChatReadMode, ChatReceiver,
  ChatSend, ChatSendRequest, ChatSendResult, ChatVoice, RoomKind,
};
use serde::Serialize;
use telegram_dialect::TelegramHtml;
use tokio::task::JoinHandle;

use crate::owner;
use crate::policy::POLICY;
use crate::reader::{ReaderClient, ReaderFetch, ReaderHealth};
use crate::seam::Standing;
use crate::seen::Seen;
use crate::send::{Sending, Speaker, refused};
use crate::telegram::Telegram;

/// Why the owner's account is not read by where no reader stands behind the service.
pub const NO_READER: &str = "no reader stands behind the service";

/// The provider: the bot account, how it speaks, the reader behind it and where each of the reader's parts
/// stands.
pub struct TelegramProvider {
  telegram: Arc<Telegram>,
  speaker: Speaker,
  reader: Option<ReaderClient>,
  reader_health: Mutex<ReaderHealth>,
  standing: Arc<Standing>,
  reading: Mutex<Option<JoinHandle<()>>>,
}

// The name a voice or a verb crosses the wire by.
fn wire_name(value: &impl Serialize) -> String {
  serde_json::to_value(value)
    .ok()
    .and_then(|named| named.as_str().map(str::to_owned))
    .unwrap_or_default()
}

impl TelegramProvider {
  /// The provider over the bot account, speaking as `sending` configures it, with the reader where one stands
  /// behind it, the record of what has been seen and where the reader's parts stand.
  pub fn new(
    telegram: Arc<Telegram>,
    sending: Sending,
    reader: Option<ReaderClient>,
    seen: Arc<Seen>,
    standing: Arc<Standing>,
  ) -> Self {
    let reason = if reader.is_some() { "the reader has not been asked yet" } else { NO_READER };

    Self {
      speaker: Speaker::new(telegram.api().clone(), sending, seen),
      telegram,
      reader,
      reader_health: Mutex::new(ReaderHealth {
        connected: false,
        reason: Some(reason.to_owned()),
      }),
      standing,
      reading: Mutex::default(),
    }
  }

  /// The reader asked whether the owner's account is connected, and the answer kept for the declaration.
  pub async fn refresh_reader(&self) -> ReaderHealth {
    let Some(reader) = &self.reader else {
      return self.reader_health();
    };
    let health = reader.health().await;

    *self.reader_health.lock().unwrap_or_else(PoisonError::into_inner) = health.clone();

    health
  }

  /// Whether the owner's account was connected when the reader was last asked.
  pub fn reader_health(&self) -> ReaderHealth {
    self.reader_health.lock().unwrap_or_else(PoisonError::into_inner).clone()
  }

  // A send in the owner's own voice: words and albums alone, through the reader, while it is connected.
  async fn as_owner(&self, request: &ChatSendRequest) -> Result<ChatSendResult, ChatFailure> {
    let health = self.refresh_reader().await;
    let Some(reader) = self.reader.as_ref().filter(|_| health.connected) else {
      let why = health.reason.as_deref().unwrap_or(NO_READER);

      return Ok(refused(POLICY.mode_down(&wire_name(&ChatVoice::HumanReader), Some(why))));
    };

    match &request.send {
      ChatSend::Text { text, .. } => owner::say(reader, request.chat_id, text).await,
      ChatSend::Album { items, caption, .. } => {
        owner::album(&self.speaker, reader, request.chat_id, items, caption.as_deref()).await
      }
      other => Ok(refused(POLICY.owner_verb(&wire_name(&other.verb())))),
    }
  }
}

#[async_trait]
impl ChatProvider for TelegramProvider {
  fn key(&self) -> &str {
    crate::receiver::VIA
  }

  fn label(&self) -> &str {
    &POLICY.telegram.label
  }

  fn configured(&self) -> Option<String> {
    self.telegram.configured()
  }

  async fn start(&self, receiver: Arc<dyn ChatReceiver>) -> Result<(), ChatFailure> {
    self.refresh_reader().await;

    let reading = tokio::spawn(self.telegram.clone().read(receiver));

    if let Some(earlier) = self.reading.lock().unwrap_or_else(PoisonError::into_inner).replace(reading) {
      earlier.abort();
    }

    Ok(())
  }

  async fn stop(&self) {
    if let Some(reading) = self.reading.lock().unwrap_or_else(PoisonError::into_inner).take() {
      reading.abort();
    }
  }

  fn health(&self) -> ChatHealth {
    match self.configured().or_else(|| self.telegram.standing()) {
      None => ChatHealth::ready(),
      Some(reason) => ChatHealth::not_ready(reason),
    }
  }

  fn modes(&self) -> Vec<ChatReadMode> {
    let reader = self.reader_health();
    let mode = |voice: ChatVoice, standing: ChatModeStanding, reason: Option<String>| ChatReadMode {
      voice,
      reach: ChatReach::AllMessages,
      standing,
      reason,
      can_reply_to: !POLICY.telegram.unpinned.contains(&voice),
    };
    let (standing, reason) = if reader.connected {
      (ChatModeStanding::Active, None)
    } else {
      (ChatModeStanding::Inactive, reader.reason.or_else(|| Some(NO_READER.to_owned())))
    };

    vec![
      mode(ChatVoice::BotAccount, ChatModeStanding::Active, None),
      mode(ChatVoice::HumanReader, standing, reason),
    ]
  }

  fn can(&self) -> BTreeSet<ChatCapability> {
    let reading = self.reader_health().connected;
    let mut can: BTreeSet<ChatCapability> = POLICY.telegram.can.iter().copied().collect();

    if !reading {
      can.remove(&ChatCapability::FetchMedia);
    }

    if !reading || self.standing.history().is_some() {
      can.remove(&ChatCapability::History);
    }

    if !reading || self.standing.avatars().is_some() {
      can.remove(&ChatCapability::Avatars);
    }

    can
  }

  fn dialect(&self) -> &dyn ChatDialect {
    &TelegramHtml
  }

  fn numbering(&self) -> ChatNumbering {
    ChatNumbering {
      rooms: ChatNumberSpace::Native,
      people: ChatNumberSpace::Native,
      messages: ChatNumberSpace::Native,
    }
  }

  async fn owns_room(&self, chat_id: i64) -> Result<bool, ChatFailure> {
    Ok(self.telegram.owns_room(chat_id))
  }

  async fn identity_of(&self, _chat_id: i64) -> Result<Option<i64>, ChatFailure> {
    Ok(Some(self.telegram.me()))
  }

  async fn is_owner_room(&self, chat_id: i64) -> Result<RoomKind, ChatFailure> {
    Ok(self.telegram.room_kind(chat_id))
  }

  async fn private_room_of(&self, chat_id: i64) -> Result<Option<i64>, ChatFailure> {
    Ok(self.telegram.private_room_of(chat_id))
  }

  async fn owner_room(&self) -> Result<Option<i64>, ChatFailure> {
    Ok(Some(self.telegram.owner_id()))
  }

  async fn room_title(&self, chat_id: i64) -> Result<Option<String>, ChatFailure> {
    Ok(self.telegram.room_title(chat_id).await)
  }

  async fn send(&self, request: ChatSendRequest) -> Result<ChatSendResult, ChatFailure> {
    match request.voice {
      None | Some(ChatVoice::BotAccount) => self.speaker.send(request.chat_id, &request.send).await,
      Some(ChatVoice::HumanReader) => self.as_owner(&request).await,
      Some(other) => Ok(refused(POLICY.mode_down(&wire_name(&other), None))),
    }
  }

  async fn fetch_media(&self, request: ChatMediaRequest) -> Result<ChatMediaResult, ChatFailure> {
    let Some(reader) = &self.reader else {
      return Ok(ChatMediaResult::Refused {
        refusal: self.unsupported(ChatCapability::FetchMedia),
      });
    };
    let ChatMediaRequest {
      chat_id,
      message_id,
      sent_at,
    } = request;
    let fetched = match reader.picture(chat_id, message_id, sent_at).await {
      ReaderFetch::Unavailable(_) => reader.file(chat_id, message_id, sent_at.unwrap_or_default()).await,
      other => other,
    };

    Ok(match fetched {
      ReaderFetch::Fetched(file) => ChatMediaResult::Fetched(file),
      ReaderFetch::Unavailable(reason) => ChatMediaResult::Unavailable { reason },
      ReaderFetch::NoReader(reason) => ChatMediaResult::Refused {
        refusal: POLICY.mode_down(&wire_name(&ChatVoice::HumanReader), Some(&reason)),
      },
    })
  }
}
