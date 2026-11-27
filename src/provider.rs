//! The Telegram provider as bmo-chat's contract states one (docs/telegram.md, 「The declaration」): what it
//! declares, the room questions and the rooms of the owner's account, its contacts and a conversation started
//! with them, a room's members, a send in the voice it asks for, a message's picture or file fetched again
//! through the owner's account, and the profile pictures it keeps. The bot account reads and speaks; the reader
//! stands behind it.

use std::collections::{BTreeMap, BTreeSet};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bmo_chat::{
  ChatAvatar, ChatAvatarKind, ChatCapability, ChatContact, ChatContactKind, ChatDialect, ChatFailure,
  ChatHealth, ChatMediaRequest, ChatMediaResult, ChatMember, ChatMembersResult, ChatModeStanding,
  ChatNumberRanges, ChatNumberSpace, ChatNumbering, ChatPicture, ChatProvider, ChatReach, ChatReadMode,
  ChatReceiver, ChatRefusal, ChatSend, ChatSendRequest, ChatSendResult, ChatStartRequest, ChatStartResult,
  ChatVoice, MessageFile, ProviderRoom, RoomQuery,
};
use serde::Serialize;
use telegram_dialect::TelegramHtml;
use tokio::task::{JoinHandle, JoinSet};

use crate::members::merged;
use crate::owner::OwnerVoice;
use crate::policy::POLICY;
use crate::reader::{
  ReaderClient, ReaderContact, ReaderFetch, ReaderHealth, ReaderMessage, ReaderRead, ReaderRoom,
  ReaderStart,
};
use crate::seam::{Standing, read_asked};
use crate::seen::Seen;
use crate::send::{Sending, Speaker, refused};
use crate::telegram::{Telegram, UNANSWERED_FRESH};

/// Why the owner's account is not read by where no reader stands behind the service.
pub const NO_READER: &str = "no reader stands behind the service";

// How many rooms or people Telegram is asked about at once.
const ASKED_AT_ONCE: usize = 8;

// How many of a room's recent lines the owner's account reads for who spoke there when its members are asked.
const RECENT_LINES: usize = 200;

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
  // Each supervisor's own bot, by the supervisor's name (docs/telegram.md, 「A bot for each supervisor」).
  bots: std::collections::BTreeMap<String, Speaker>,
  reader: Option<ReaderClient>,
  reader_health: Mutex<ReaderHealth>,
  standing: Arc<Standing>,
  reading: Mutex<Option<JoinHandle<()>>>,
  // The receiving side the provider was started with, asked whether it reads a room the bot is not in.
  receiver: Mutex<Option<Arc<dyn ChatReceiver>>>,
  // The folder the reader keeps the profile pictures in.
  avatars: PathBuf,
  // The owner's account's chats as the reader last listed them, or why it could not, and when; held while the
  // reader is asked, so one asker asks at a time and the others take its answer.
  listed: tokio::sync::Mutex<Option<KeptDialogs>>,
}

// The owner's account's chats as the reader listed them, or why it could not, and when.
type KeptDialogs = (Instant, Result<Vec<ReaderRoom>, String>);

/// How long the list of the owner's account's chats stands before the reader is asked again.
const DIALOGS_FRESH: Duration = Duration::from_secs(60);

// Whether the receiving side reads a room: its `read`, and not where it does not answer or holds nothing of it.
async fn read_of(receiver: &dyn ChatReceiver, chat_id: i64) -> bool {
  let query = RoomQuery {
    via: crate::VIA.to_owned(),
  };

  matches!(receiver.room(chat_id, query).await, Ok(Some(held)) if read_asked(&held))
}

/// Each of `ids` asked by `ask`, `ASKED_AT_ONCE` at a time, its answer under its id; one whose asking came to no
/// answer stands with none.
pub(crate) async fn asked_each<T, F, Asking>(ids: &[i64], ask: F) -> BTreeMap<i64, T>
where
  T: Send + 'static,
  F: Fn(i64) -> Asking,
  Asking: Future<Output = T> + Send + 'static,
{
  let mut answers = BTreeMap::new();

  for some in ids.chunks(ASKED_AT_ONCE) {
    let mut asking = JoinSet::new();

    for &id in some {
      let answer = ask(id);

      asking.spawn(async move { (id, answer.await) });
    }

    while let Some(answer) = asking.join_next().await {
      if let Ok((id, answered)) = answer {
        answers.insert(id, answered);
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

/// Whether a send is a typing mark in the bot's voice, or naming none, for an account: that account's own typing,
/// not the answerer's.
pub fn its_own_act(send: &ChatSend) -> bool {
  matches!(
    send,
    ChatSend::Typing { .. }
      | ChatSend::Reaction { .. }
      | ChatSend::Pin { .. }
      | ChatSend::Edit { .. }
      | ChatSend::Delete { .. }
  )
}

fn typing_for_an_account(request: &ChatSendRequest) -> bool {
  matches!(request.send, ChatSend::Typing { .. })
    && matches!(request.mode, None | Some(ChatVoice::BotAccount))
    && request.account_id.is_some()
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
      speaker: Speaker::new(telegram.api().clone(), sending, seen, telegram.lines().clone()),
      bots: Default::default(),
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
      listed: tokio::sync::Mutex::default(),
    }
  }

  /// The provider whose supervisors named in `tokens` speak through bots of their own, each asking the Bot API at
  /// `api` and keeping its messages in the service's ledger (docs/telegram.md, 「A bot for each supervisor」).
  pub fn with_supervisor_bots(
    mut self,
    api: &str,
    tokens: &std::collections::BTreeMap<String, String>,
  ) -> Self {
    self.bots = tokens
      .iter()
      .map(|(name, token)| {
        let bot = Speaker::new(
          crate::bot_api::BotApi::new(api, token),
          self.speaker.sending.clone(),
          self.speaker.seen().clone(),
          self.speaker.lines.clone(),
        );

        (name.clone(), bot)
      })
      .collect();
    self
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

    let mut listed = self.listed.lock().await;
    let kept = listed
      .as_ref()
      .filter(|(at, answer)| at.elapsed() < if answer.is_ok() { DIALOGS_FRESH } else { UNANSWERED_FRESH });

    if let Some((_, answer)) = kept {
      return answer.clone();
    }

    let answer = reader.dialogs().await;

    *listed = Some((Instant::now(), answer.clone()));

    answer
  }

  // Everyone the owner's account lists in a room among its chats, the senders of its recent lines kept first as the
  // room's; none where no reader stands, the account's chats are not read, or the room is not among them.
  async fn account_members(&self, chat_id: i64) -> Option<Result<Vec<ChatMember>, String>> {
    let reader = self.reader.as_ref()?;
    let rooms = self.dialogs().await.ok()?;

    if !rooms.iter().any(|room| room.chat_id == chat_id) {
      return None;
    }

    // Telegram may leave a member out of the account's list, so whoever spoke in the room's recent lines is kept as
    // one of its senders, for the bot account to check.
    if let Ok(ReaderRead::Messages(lines)) = reader.messages(chat_id, RECENT_LINES, None).await {
      self.telegram.spoke(chat_id, lines.iter().filter_map(|line| line.sender_id));
    }

    Some(reader.members(chat_id).await)
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
    match request.prefix.as_deref().filter(|prefix| !prefix.trim().is_empty()) {
      // A prefix the send names is learnt, so the line is read back as the agent's whatever its tag says.
      Some(prefix) => {
        self.telegram.avatar().learn(prefix);
        prefix
      }
      None => self.telegram.avatar().prefix(),
    }
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
    crate::VERSION
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
      // The bot account hears nobody's typing or status: only the owner's account does.
      can.remove(&ChatCapability::PeopleTyping);
      can.remove(&ChatCapability::Presence);
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

  fn speakers(&self) -> bool {
    !self.bots.is_empty()
  }

  fn numbering(&self) -> ChatNumbering {
    ChatNumbering {
      rooms: ChatNumberSpace::Native,
      people: ChatNumberSpace::Native,
      messages: ChatNumberSpace::Native,
      ranges: ChatNumberRanges::default(),
    }
  }

  // Each file of a multipart send is written here as it arrives, and each file the gathering fetched past the
  // Bot API's ceiling stands here until the receiving side took it (docs/telegram.md, 「Reading a room」).
  fn spool(&self) -> PathBuf {
    self.telegram.spool()
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

  // A room's members (docs/telegram.md, 「The members of a room」): everyone the owner's account lists where it is
  // in the room, then whoever the bot account can tell is still there. Where neither gave any, the room's members
  // are not held, for the reason of each account that tried.
  async fn members(&self, room_id: i64) -> Result<ChatMembersResult, ChatFailure> {
    let mut lists = Vec::new();
    let mut unread = Vec::new();

    match self.account_members(room_id).await {
      Some(Ok(listed)) => lists.push(listed),
      Some(Err(why)) => unread.push(format!("the owner's account read none: {why}")),
      None => {}
    }

    if self.telegram.bot_in(room_id).await {
      match self.telegram.bot_members(room_id).await {
        Ok(listed) => lists.push(listed),
        Err(why) => unread.push(format!("the bot account read none: {why}")),
      }
    }

    if lists.is_empty() {
      let reason = if unread.is_empty() {
        format!("neither the bot nor the owner's account is in room {room_id}")
      } else {
        unread.join("; ")
      };

      return Ok(ChatMembersResult::Unheld { reason });
    }

    Ok(ChatMembersResult::Listed(merged(lists)))
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
    // A send naming a supervisor goes through its own bot, or the service's where it has none; any other speaker
    // is refused, so another account's words go in the service's voice with their name before them
    // (docs/telegram.md, 「A bot for each supervisor」).
    let bot_voice = matches!(request.mode, None | Some(ChatVoice::BotAccount));
    let own_bot = match &request.speaker {
      None => None,
      Some(speaker) if bot_voice && !speaker.contains(':') => self.bots.get(speaker),
      Some(speaker) => {
        return Ok(refused(ChatRefusal::SpeakerNotAllowed {
          label: self.label().to_owned(),
          speaker: speaker.clone(),
        }));
      }
    };

    // A supervisor's bot not in the room, or turned down, gives way to the service's own bot for what it says; an act
    // in the supervisor's own name — a typing mark, a reaction, a pin, an edit or a delete — is its bot's alone, so
    // its refusal stands rather than the service's bot doing it as another (docs/telegram.md, 「A bot for each
    // supervisor」).
    if let Some(bot) = own_bot {
      let said = bot.send(request.room_id, request.reply_to_message_id, &request.send).await;

      if matches!(said, Ok(ChatSendResult::Sent(_))) || its_own_act(&request.send) {
        return said;
      }
    }

    // A person's typing goes only in their own voice: one in the bot's voice for an account is that account's,
    // and the provider holds no Telegram account of theirs (docs/telegram.md, 「Sends」).
    if typing_for_an_account(&request) {
      return Ok(refused(POLICY.typing_account()));
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
