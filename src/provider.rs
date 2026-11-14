//! The Telegram provider as bmo-chat's contract states one (docs/telegram.md, 「The declaration」): what it
//! declares, the room questions and the rooms of the owner's account, its contacts and a conversation started
//! with them, a send in the voice it asks for, a message's picture or file fetched again through the owner's
//! account, and the profile pictures it keeps. The bot account reads and speaks; the reader stands behind it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use bmo_chat::{
  ChatAvatar, ChatAvatarKind, ChatCapability, ChatContact, ChatContactKind, ChatDialect, ChatFailure,
  ChatHealth, ChatMediaRequest, ChatMediaResult, ChatModeStanding, ChatNumberRanges, ChatNumberSpace,
  ChatNumbering, ChatPicture, ChatProvider, ChatReach, ChatReadMode, ChatReceiver, ChatRefusal,
  ChatSendRequest, ChatSendResult, ChatStartRequest, ChatStartResult, ChatVoice, MessageFile, ProviderRoom,
  RoomQuery,
};
use serde::Serialize;
use telegram_dialect::TelegramHtml;
use tokio::task::{JoinHandle, JoinSet};

use crate::owner::OwnerVoice;
use crate::policy::POLICY;
use crate::reader::{
  ReaderClient, ReaderContact, ReaderFetch, ReaderHealth, ReaderMessage, ReaderRead, ReaderRoom,
  ReaderStart,
};
use crate::seam::{Standing, read_asked};
use crate::seen::Seen;
use crate::send::{Sending, Speaker, refused};
use crate::telegram::Telegram;

/// Why the owner's account is not read by where no reader stands behind the service.
pub const NO_READER: &str = "no reader stands behind the service";

// How many rooms Telegram is asked about at once when the rooms are listed.
const ASKED_AT_ONCE: usize = 8;

/// What a chat read came to: its recent messages, the chats and contacts a name leaves to pick among, or the
/// reader's refusal with the status it answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadAnswer {
  Messages(Vec<ReaderMessage>),
  Candidates(Vec<ReaderCandidate>),
  Refused { status: u16, reason: String },
}

/// One chat or contact a name could mean, so the owner picks among them by number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderCandidate {
  pub chat_id: i64,
  pub name: String,
  pub kind: String,
}

// The chats and contacts a name could mean: those whose title or name holds the words asked, and whether any
// did; each chat once. Where none did, every one listed stands so the owner picks by number.
fn candidates_for(
  wanted: &str,
  dialogs: &[ReaderRoom],
  contacts: &[ReaderContact],
) -> (bool, Vec<ReaderCandidate>) {
  let wanted = wanted.trim().to_lowercase();
  let mut candidates = Vec::new();

  for room in dialogs {
    let name = room
      .title
      .clone()
      .filter(|title| !title.trim().is_empty())
      .unwrap_or_else(|| format!("chat {}", room.chat_id));

    candidates.push(ReaderCandidate {
      chat_id: room.chat_id,
      name,
      kind: "room".to_owned(),
    });
  }

  for contact in contacts {
    candidates.push(ReaderCandidate {
      chat_id: contact.chat_id.unwrap_or(contact.id),
      name: contact.name.clone(),
      kind: "contact".to_owned(),
    });
  }

  let matching: Vec<ReaderCandidate> = candidates
    .iter()
    .filter(|candidate| candidate.name.to_lowercase().contains(&wanted))
    .cloned()
    .collect();
  let matched = !matching.is_empty();
  let chosen = if matched { matching } else { candidates };
  let mut seen = BTreeSet::new();

  (matched, chosen.into_iter().filter(|candidate| seen.insert(candidate.chat_id)).collect())
}

// One chat read through the reader, its refusal kept with the status the reader answered.
async fn read_one(reader: &ReaderClient, chat_id: i64, max: usize, since: Option<i64>) -> ReadAnswer {
  match reader.messages(chat_id, max, since).await {
    Ok(ReaderRead::Messages(messages)) => ReadAnswer::Messages(messages),
    Ok(ReaderRead::Refused { status, reason }) => ReadAnswer::Refused { status, reason },
    Err(why) => ReadAnswer::Refused {
      status: 503,
      reason: why,
    },
  }
}

/// The provider: the bot account, how it speaks, the reader behind it and where each of the reader's parts
/// stands.
pub struct TelegramProvider {
  telegram: Arc<Telegram>,
  speaker: Speaker,
  reader: Option<ReaderClient>,
  reader_health: Mutex<ReaderHealth>,
  standing: Arc<Standing>,
  reading: Mutex<Option<JoinHandle<()>>>,
  // The receiving side the provider was started with, asked whether it reads a room the bot is not in.
  receiver: Mutex<Option<Arc<dyn ChatReceiver>>>,
  // The folder the reader keeps the profile pictures in.
  avatars: PathBuf,
}

// Whether the receiving side reads a room: its `read`, and not where it does not answer or holds nothing of it.
async fn read_of(receiver: &dyn ChatReceiver, chat_id: i64) -> bool {
  let query = RoomQuery {
    via: crate::VIA.to_owned(),
  };

  matches!(receiver.room(chat_id, query).await, Ok(Some(held)) if read_asked(&held))
}

// Each of `chat_ids` asked by `ask`, `ASKED_AT_ONCE` at a time; one whose asking came to no answer counts false.
async fn asked_each<F, Asking>(chat_ids: &[i64], ask: F) -> BTreeMap<i64, bool>
where
  F: Fn(i64) -> Asking,
  Asking: Future<Output = bool> + Send + 'static,
{
  let mut answers = BTreeMap::new();

  for some in chat_ids.chunks(ASKED_AT_ONCE) {
    let mut asking = JoinSet::new();

    for &chat_id in some {
      let answer = ask(chat_id);

      asking.spawn(async move { (chat_id, answer.await) });
    }

    while let Some(answer) = asking.join_next().await {
      if let Ok((chat_id, yes)) = answer {
        answers.insert(chat_id, yes);
      }
    }
  }

  answers
}

/// The name a voice or a verb crosses the wire by.
pub fn wire_name(value: &impl Serialize) -> String {
  serde_json::to_value(value)
    .ok()
    .and_then(|named| named.as_str().map(str::to_owned))
    .unwrap_or_default()
}

impl TelegramProvider {
  /// The provider over the bot account, speaking as `sending` configures it, with the reader where one stands
  /// behind it, the record of what has been seen, where the reader's parts stand, and the folder the pictures
  /// are kept in.
  pub fn new(
    telegram: Arc<Telegram>,
    sending: Sending,
    reader: Option<ReaderClient>,
    seen: Arc<Seen>,
    standing: Arc<Standing>,
    avatars: PathBuf,
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
      receiver: Mutex::default(),
      avatars,
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

  // The voices a room of the bot's is read by and may be sent in: the declaration's active ones.
  fn active_voices(&self) -> Vec<ChatVoice> {
    self
      .modes()
      .into_iter()
      .filter(|mode| mode.standing == ChatModeStanding::Active)
      .map(|mode| mode.voice)
      .collect()
  }

  // Every chat of the owner's account, as the reader lists them.
  async fn dialogs(&self) -> Result<Vec<ReaderRoom>, String> {
    let Some(reader) = &self.reader else {
      return Err(NO_READER.to_owned());
    };

    reader.dialogs().await
  }

  // Every contact of the owner's account, as the reader lists them.
  async fn reader_contacts(&self) -> Result<Vec<ReaderContact>, String> {
    let Some(reader) = &self.reader else {
      return Err(NO_READER.to_owned());
    };

    reader.contacts().await
  }

  /// One chat's recent messages on the owner's word, read through the reader without a room standing for it.
  /// A chat named by a person or a handle is read where exactly one chat matches; where several could, the
  /// chats and contacts are answered so the owner picks. Nothing is sent into the chat.
  pub async fn read_messages(&self, chat: &str, max: usize, since: Option<i64>) -> ReadAnswer {
    let Some(reader) = &self.reader else {
      return ReadAnswer::Refused {
        status: 503,
        reason: NO_READER.to_owned(),
      };
    };
    let chat = chat.trim();

    if let Ok(chat_id) = chat.parse::<i64>() {
      return read_one(reader, chat_id, max, since).await;
    }

    match (reader.dialogs().await, reader.contacts().await) {
      (Ok(dialogs), Ok(contacts)) => {
        let (matched, candidates) = candidates_for(chat, &dialogs, &contacts);

        // A name that means exactly one chat reads it; one that could mean several leaves the owner to pick.
        if matched && candidates.len() == 1 {
          return read_one(reader, candidates[0].chat_id, max, since).await;
        }

        ReadAnswer::Candidates(candidates)
      }
      (Err(why), _) | (_, Err(why)) => ReadAnswer::Refused {
        status: 503,
        reason: why,
      },
    }
  }

  // Whether the bot is in each of `chat_ids`, Telegram asked `ASKED_AT_ONCE` rooms at a time; a room whose asking
  // came to no answer counts as one the bot is not in.
  async fn bot_in_each(&self, chat_ids: &[i64]) -> BTreeMap<i64, bool> {
    let telegram = self.telegram.clone();

    asked_each(chat_ids, move |chat_id| {
      let telegram = telegram.clone();

      async move { telegram.bot_in(chat_id).await }
    })
    .await
  }

  // The receiving side the provider was started with, where it has been.
  fn receiver(&self) -> Option<Arc<dyn ChatReceiver>> {
    self.receiver.lock().unwrap_or_else(PoisonError::into_inner).clone()
  }

  /// Whether the receiving side reads a room through the owner's account: its `read` of the room. One that has not
  /// started the provider, does not answer, or holds nothing of the room reads it not (docs/telegram.md, 「The
  /// rooms」).
  pub async fn read_by_receiver(&self, chat_id: i64) -> bool {
    match self.receiver() {
      Some(receiver) => read_of(receiver.as_ref(), chat_id).await,
      None => false,
    }
  }

  // Whether the receiving side reads each of `chat_ids`, asked `ASKED_AT_ONCE` rooms at a time.
  async fn read_each(&self, chat_ids: &[i64]) -> BTreeMap<i64, bool> {
    let Some(receiver) = self.receiver() else {
      return chat_ids.iter().map(|&chat_id| (chat_id, false)).collect();
    };

    asked_each(chat_ids, move |chat_id| {
      let receiver = receiver.clone();

      async move { read_of(receiver.as_ref(), chat_id).await }
    })
    .await
  }

  // A room of the owner's account the bot is not in, under `title`, as the receiving side's reading of it makes
  // it.
  async fn account_room(&self, chat_id: i64, title: Option<String>) -> ProviderRoom {
    self.telegram.account_room_of(chat_id, title, self.read_by_receiver(chat_id).await)
  }

  // A room a conversation came to: the bot's where the bot is in it, and the owner's account's where it is not
  // (docs/telegram.md, 「Contacts and starting a conversation」).
  async fn started_room(&self, chat_id: i64, title: Option<String>) -> ProviderRoom {
    if self.telegram.bot_in(chat_id).await {
      self.telegram.room_of(chat_id, title, self.active_voices())
    } else {
      self.account_room(chat_id, title).await
    }
  }

  /// The prefix the avatar's words carry (docs/telegram.md, 「The avatar」): the send's own where it is not blank,
  /// else the deployment's, else the policy's.
  pub fn avatar_prefix<'a>(&'a self, request: &'a ChatSendRequest) -> &'a str {
    request
      .prefix
      .as_deref()
      .filter(|prefix| !prefix.trim().is_empty())
      .unwrap_or(self.telegram.avatar().prefix())
  }

  // A send in the owner's account, in `voice`, through the reader while it has the account connected; never by
  // the bot instead. The owner's own words go as they are, the avatar's behind its prefix. It is going into its
  // room until it is answered, so its echo waits for it.
  async fn in_owner_account(
    &self,
    request: &ChatSendRequest,
    voice: ChatVoice,
  ) -> Result<ChatSendResult, ChatFailure> {
    let health = self.refresh_reader().await;
    let Some(reader) = self.reader.as_ref().filter(|_| health.connected) else {
      let why = health.reason.as_deref().unwrap_or(NO_READER);

      return Ok(refused(POLICY.mode_down(&wire_name(&voice), Some(why))));
    };
    let echoes = self.telegram.echoes();
    let _going = echoes.going(request.room_id);
    let prefix = (voice == ChatVoice::HumanAvatar).then(|| self.avatar_prefix(request));
    let voice = OwnerVoice {
      speaker: &self.speaker,
      reader,
      echoes,
      voice,
      prefix,
    };

    voice.send(request).await
  }
}

#[async_trait]
impl ChatProvider for TelegramProvider {
  fn key(&self) -> &str {
    crate::VIA
  }

  fn label(&self) -> &str {
    &POLICY.telegram.label
  }

  fn version(&self) -> &str {
    env!("CARGO_PKG_VERSION")
  }

  fn configured(&self) -> Option<String> {
    self.telegram.configured()
  }

  async fn start(&self, receiver: Arc<dyn ChatReceiver>) -> Result<(), ChatFailure> {
    self.refresh_reader().await;
    *self.receiver.lock().unwrap_or_else(PoisonError::into_inner) = Some(receiver.clone());

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
      None => ChatHealth::ready(self.version()),
      Some(reason) => ChatHealth::not_ready(self.version(), reason),
    }
  }

  fn modes(&self) -> Vec<ChatReadMode> {
    let reader = self.reader_health();
    let mode = |voice: ChatVoice, standing: ChatModeStanding, reason: Option<String>| ChatReadMode {
      voice,
      reach: ChatReach::AllMessages,
      standing,
      reason,
      // An answer can be pinned in every voice: the owner's account finds the line by the message's number.
      can_reply_to: true,
    };
    let (standing, reason) = if reader.connected {
      (ChatModeStanding::Active, None)
    } else {
      (ChatModeStanding::Inactive, reader.reason.or_else(|| Some(NO_READER.to_owned())))
    };

    // The owner's own words and the avatar's both stand on the owner's account, and so stand or fall together.
    vec![
      mode(ChatVoice::BotAccount, ChatModeStanding::Active, None),
      mode(ChatVoice::HumanReader, standing, reason.clone()),
      mode(ChatVoice::HumanAvatar, standing, reason),
    ]
  }

  fn can(&self) -> BTreeSet<ChatCapability> {
    let reading = self.reader_health().connected;
    let mut can: BTreeSet<ChatCapability> = POLICY.telegram.can.iter().copied().collect();

    if !reading {
      can.remove(&ChatCapability::FetchMedia);
      can.remove(&ChatCapability::RoomList);
      can.remove(&ChatCapability::ContactList);
      can.remove(&ChatCapability::StartChat);
      can.remove(&ChatCapability::StartGroup);
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
      ranges: ChatNumberRanges::default(),
    }
  }

  async fn room(&self, room_id: i64) -> Result<Option<ProviderRoom>, ChatFailure> {
    if self.telegram.bot_in(room_id).await {
      return Ok(Some(self.telegram.bot_room(room_id, self.active_voices()).await));
    }

    // A room the bot is not in is the owner's account's: one the account lists, under its title, or one seen.
    let listed = self
      .dialogs()
      .await
      .unwrap_or_default()
      .into_iter()
      .find(|room| room.chat_id == room_id);

    if let Some(room) = listed {
      return Ok(Some(self.account_room(room_id, room.title).await));
    }

    if self.telegram.has_seen(room_id) {
      return Ok(Some(self.account_room(room_id, None).await));
    }

    // A one-to-one with a contact stands before its first message, under the contact's number.
    if self.telegram.private_room_of(room_id).is_none() {
      return Ok(None);
    }

    let contact = self
      .reader_contacts()
      .await
      .unwrap_or_default()
      .into_iter()
      .find(|contact| contact.id == room_id);

    let Some(contact) = contact else {
      return Ok(None);
    };

    Ok(Some(self.account_room(room_id, Some(contact.name)).await))
  }

  async fn owner_room(&self) -> Result<Option<i64>, ChatFailure> {
    Ok(Some(self.telegram.owner_id()))
  }

  // The bot's rooms and every chat of the owner's account, each once (docs/telegram.md, 「The rooms」): a chat the
  // account lists is the bot's where the bot is in it and read-only where it is not; a room of the bot's the
  // account does not list is titled as Telegram tells the bot.
  async fn account_rooms(&self) -> Result<Vec<ProviderRoom>, ChatFailure> {
    let dialogs = self
      .dialogs()
      .await
      .map_err(|why| format!("the owner's account's rooms were not read: {why}"))?;
    let mut titles = BTreeMap::new();

    for ReaderRoom { chat_id, title } in dialogs {
      titles.entry(chat_id).or_insert(title);
    }

    let bot_rooms: BTreeSet<i64> = std::iter::once(self.telegram.owner_id())
      .chain(self.telegram.seen_rooms())
      .filter(|chat_id| !titles.contains_key(chat_id))
      .collect();
    let asked: Vec<i64> = titles.keys().chain(&bot_rooms).copied().collect();
    let present = self.bot_in_each(&asked).await;
    let bot_in = |chat_id: &i64| present.get(chat_id).copied().unwrap_or(false);
    let account_only: Vec<i64> = titles.keys().copied().filter(|chat_id| !bot_in(chat_id)).collect();
    let read = self.read_each(&account_only).await;
    let voices = self.active_voices();
    let mut rooms = BTreeMap::new();

    for (chat_id, title) in titles {
      let room = if bot_in(&chat_id) {
        self.telegram.room_of(chat_id, title, voices.clone())
      } else {
        self
          .telegram
          .account_room_of(chat_id, title, read.get(&chat_id).copied().unwrap_or(false))
      };

      rooms.insert(chat_id, room);
    }

    for chat_id in bot_rooms.into_iter().filter(bot_in) {
      rooms.insert(chat_id, self.telegram.bot_room(chat_id, voices.clone()).await);
    }

    Ok(rooms.into_values().collect())
  }

  // The owner's account's contacts, whichever account a listing names: it is the one person's account this
  // provider holds (bmo-chat's docs/contract.md, 「The account a call is for」).
  async fn contacts(&self, _account_id: Option<i64>) -> Result<Vec<ChatContact>, ChatFailure> {
    let contacts = self
      .reader_contacts()
      .await
      .map_err(|why| format!("the owner's account's contacts were not read: {why}"))?;

    Ok(
      contacts
        .into_iter()
        .map(|contact| ChatContact {
          id: contact.id,
          name: contact.name,
          kind: if contact.bot { ChatContactKind::Bot } else { ChatContactKind::Person },
          room_id: contact.chat_id,
        })
        .collect(),
    )
  }

  // A conversation started by the owner's account, never by the bot, which cannot start a one-to-one
  // (docs/telegram.md, 「Contacts and starting a conversation」).
  async fn start_chat(&self, request: ChatStartRequest) -> Result<ChatStartResult, ChatFailure> {
    let start = request.start;
    let health = self.refresh_reader().await;
    let Some(reader) = self.reader.as_ref().filter(|_| health.connected) else {
      let why = health.reason.as_deref().unwrap_or(NO_READER);

      return Ok(ChatStartResult::Refused(
        POLICY.mode_down(&wire_name(&ChatVoice::HumanReader), Some(why)),
      ));
    };
    let started = reader
      .start(&start)
      .await
      .map_err(|why| format!("the owner's account did not start the conversation: {why}"))?;

    Ok(match started {
      ReaderStart::Started { chat_id, title } => {
        ChatStartResult::Started(self.started_room(chat_id, title).await)
      }
      ReaderStart::NoContact(contact_id) => ChatStartResult::NoContact { contact_id },
      ReaderStart::Refused(reason) => ChatStartResult::Refused(POLICY.refused(&reason)),
    })
  }

  async fn send(&self, request: ChatSendRequest) -> Result<ChatSendResult, ChatFailure> {
    if let Some(speaker) = &request.speaker {
      return Ok(refused(ChatRefusal::SpeakerNotAllowed {
        label: self.label().to_owned(),
        speaker: speaker.clone(),
      }));
    }

    // A room the bot is not in takes the owner's own words, and the avatar's where the receiving side reads it
    // (docs/telegram.md, 「The rooms」).
    if request.mode != Some(ChatVoice::HumanReader) && !self.telegram.bot_in(request.room_id).await {
      let answered_as_owner =
        request.mode == Some(ChatVoice::HumanAvatar) && self.read_by_receiver(request.room_id).await;

      if !answered_as_owner {
        return Ok(refused(POLICY.read_only()));
      }
    }

    match request.mode {
      None | Some(ChatVoice::BotAccount) => {
        self.speaker.send(request.room_id, request.reply_to_message_id, &request.send).await
      }
      Some(voice @ (ChatVoice::HumanReader | ChatVoice::HumanAvatar)) => {
        self.in_owner_account(&request, voice).await
      }
      Some(other) => Ok(refused(POLICY.mode_down(&wire_name(&other), None))),
    }
  }

  async fn fetch_media(&self, request: ChatMediaRequest) -> Result<ChatMediaResult, ChatFailure> {
    let Some(reader) = &self.reader else {
      return Ok(ChatMediaResult::Refused(self.unsupported(ChatCapability::FetchMedia)));
    };
    let ChatMediaRequest {
      room_id,
      message_id,
      sent_at,
    } = request;
    let fetched = match reader.picture(room_id, message_id, sent_at).await {
      ReaderFetch::Unavailable(_) => reader.file(room_id, message_id, sent_at.unwrap_or_default()).await,
      other => other,
    };

    Ok(match fetched {
      // A file the bot account handed over is fetched again under the id it crossed with.
      ReaderFetch::Fetched(file) => ChatMediaResult::Fetched(vec![MessageFile {
        id: self.telegram.file_id_of(room_id, message_id).unwrap_or(file.id),
        ..file
      }]),
      ReaderFetch::Unavailable(reason) => ChatMediaResult::Unavailable { reason },
      ReaderFetch::NoReader(reason) => {
        ChatMediaResult::Refused(POLICY.mode_down(&wire_name(&ChatVoice::HumanReader), Some(&reason)))
      }
    })
  }

  async fn avatars(&self) -> Result<Vec<ChatAvatar>, ChatFailure> {
    let Some(reader) = &self.reader else {
      return Ok(Vec::new());
    };

    reader
      .avatars()
      .await
      .map_err(|why| format!("the reader's profile pictures were not read: {why}").into())
  }

  async fn avatar(&self, kind: ChatAvatarKind, id: i64) -> Result<Option<ChatPicture>, ChatFailure> {
    let path = self.avatars.join(kind.to_string()).join(format!("{id}.jpg"));

    match tokio::fs::read(&path).await {
      Ok(bytes) => Ok(Some(ChatPicture {
        mime: "image/jpeg".to_owned(),
        bytes,
      })),
      Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
      Err(error) => Err(format!("the picture {} could not be read: {error}", path.display()).into()),
    }
  }
}
