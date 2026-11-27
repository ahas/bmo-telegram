//! The bot account's reading (docs/telegram.md, 「Reading a room」): the long poll of the Bot API, and each
//! update turned into what the receiver is handed over — the words at once, a picture with its bytes, a file
//! promised and gathered after. The room questions the chat protocol asks are answered here too.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bmo_chat::{ChatMember, ChatReceiver, ChatVoice, GatheredPatch, MessageFile, ProviderRoom, RoomKind};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::albums::AlbumNames;
use crate::avatar::AvatarLines;
use crate::bot_api::BotApi;
use crate::echoes::{Echoes, OwnerAct};
use crate::file_ids::FileIds;
use crate::gather::Gatherer;
use crate::kinds::Message;
use crate::lines::Lines;
use crate::members::{group_members, private_members, still_member};
use crate::policy::POLICY;
use crate::reader::ReaderClient;
use crate::reading::{Reading, SAFE_INTEGER, read, read_edit};
use crate::seam::Presence;
use crate::seen::Seen;

/// The bot account as its configuration gives it.
pub struct Telegram {
  api: BotApi,
  token_set: bool,
  owner_id: i64,
  me: AtomicI64,
  albums: Mutex<AlbumNames>,
  gatherer: Arc<Gatherer>,
  seen: Arc<Seen>,
  file_ids: Arc<FileIds>,
  // What the sends in the owner's voice went out as, so their echo is not handed over.
  echoes: Arc<Echoes>,
  // Whose numbering each line of a room numbered per account stands under.
  lines: Arc<Lines>,
  // The avatar's lines, so one read back from the owner's number is handed over as the agent's own.
  avatar: AvatarLines,
  // Why the bot account is not reading yet, or null once it is.
  standing: Mutex<Option<String>>,
  // Whether Telegram said the bot is in a room, and when it said so.
  presence: Mutex<HashMap<i64, (bool, Instant)>>,
  // What Telegram told the bot each room is titled, whether it answered, and when; held while Telegram is asked,
  // so one asker asks at a time and the others take its answer.
  titles: tokio::sync::Mutex<HashMap<i64, KeptTitle>>,
}

/// One update the Bot API delivered.
#[derive(Deserialize)]
struct Update {
  update_id: i64,
  message: Option<Value>,
  edited_message: Option<Value>,
  message_reaction: Option<Value>,
}

/// A person's reactions on a message as a reaction update tells them (docs/telegram.md, 「Reading a room」): the
/// room, the message by the bot account's number, the person, and every emoji they have on it now, a custom emoji
/// left out. None where the update names no person, as an anonymous administrator's does.
pub fn reaction_change(update: &Value) -> Option<bmo_chat::ReactionChange> {
  let person = update["user"]["id"].as_i64()?;
  let emojis = update["new_reaction"]
    .as_array()
    .into_iter()
    .flatten()
    .filter(|reaction| reaction["type"] == "emoji")
    .filter_map(|reaction| reaction["emoji"].as_str().map(str::to_owned))
    .collect();

  Some(bmo_chat::ReactionChange {
    via: crate::VIA.to_owned(),
    room_id: update["chat"]["id"].as_i64()?,
    message_id: update["message_id"].as_i64()?,
    stamp: None,
    reactions: vec![bmo_chat::PersonReactions {
      person_id: Some(person),
      emojis,
    }],
    whole: false,
  })
}

#[derive(Deserialize)]
struct Me {
  id: i64,
  username: Option<String>,
}

/// Why the bot account cannot run at all.
pub const TOKEN_MISSING: &str = "BMO_BOT_TOKEN not set";

/// How long Telegram's answer to whether the bot is in a room stands before it is asked again.
pub const PRESENCE_FRESH: Duration = Duration::from_secs(600);
// A room's title as Telegram told it, whether Telegram answered, and when.
type KeptTitle = (Option<String>, bool, Instant);

/// How long a room's title as Telegram told it stands before Telegram is asked again.
pub const TITLE_FRESH: Duration = Duration::from_secs(600);
/// How long it stands that Telegram did not answer, so a Telegram out of reach is not asked for every room.
pub const UNANSWERED_FRESH: Duration = Duration::from_secs(30);

impl Telegram {
  /// The bot account with its token, asking the Bot API at `api`, the owner's number, the reader where one
  /// stands behind it, the record of what it has seen, that of the ids its files crossed with, and the folder
  /// gathered files and the files of a multipart send stand in on this machine's disk.
  pub fn new(
    token: &str,
    api: &str,
    owner_id: i64,
    reader: Option<ReaderClient>,
    seen: Arc<Seen>,
    file_ids: Arc<FileIds>,
    spool: PathBuf,
  ) -> Self {
    let api = BotApi::new(api, token);
    let me = api.token_identity().unwrap_or_default();

    Self {
      gatherer: Arc::new(Gatherer::new(api.clone(), reader, file_ids.clone(), spool)),
      file_ids,
      api,
      token_set: !token.trim().is_empty(),
      owner_id,
      me: AtomicI64::new(me),
      albums: Mutex::default(),
      seen,
      echoes: Arc::default(),
      lines: Arc::new(Lines::in_memory()),
      avatar: AvatarLines::new(owner_id, None),
      standing: Mutex::new(Some("the bot account has not been read yet".to_owned())),
      presence: Mutex::default(),
      titles: tokio::sync::Mutex::default(),
    }
  }

  /// The bot account whose avatar keeps the prefixes the sends name in `file`, read back after a restart.
  pub fn with_avatar_kept_in(mut self, file: std::path::PathBuf) -> Self {
    self.avatar = AvatarLines::new(self.owner_id, None).kept_in(file);
    self
  }

  /// The avatar's lines as the bot account reads them back.
  pub fn avatar(&self) -> &AvatarLines {
    &self.avatar
  }

  /// The bot account whose ledger of numberings is `lines`, rather than one kept in memory alone.
  pub fn with_lines(mut self, lines: Arc<Lines>) -> Self {
    self.lines = lines;
    self
  }

  /// Whose numbering each line of a room numbered per account stands under (docs/telegram.md, 「The owner's
  /// voice」).
  pub fn lines(&self) -> &Arc<Lines> {
    &self.lines
  }

  /// Null where the bot account can run, else why not.
  pub fn configured(&self) -> Option<String> {
    (!self.token_set).then(|| TOKEN_MISSING.to_owned())
  }

  /// The bot's own number: as the platform said it once asked, as its token says it until then.
  pub fn me(&self) -> i64 {
    self.me.load(Ordering::Relaxed)
  }

  pub fn owner_id(&self) -> i64 {
    self.owner_id
  }

  /// Why the bot account is not reading yet, or null once it is.
  pub fn standing(&self) -> Option<String> {
    self.standing.lock().unwrap_or_else(PoisonError::into_inner).clone()
  }

  /// Whether the receiver listens to a room at all: the owner's one-to-one, or a room whose number the chat
  /// protocol's numbers carry whole. Asked only to keep a file from being fetched for a room nobody listens to.
  pub fn listened(&self, chat_id: i64) -> bool {
    chat_id == self.owner_id || (-SAFE_INTEGER..=SAFE_INTEGER).contains(&chat_id)
  }

  /// `owner` for the owner's one-to-one with the bot, `group` for any other room.
  pub fn room_kind(&self, chat_id: i64) -> RoomKind {
    if chat_id == self.owner_id { RoomKind::Owner } else { RoomKind::Group }
  }

  /// The person of a one-to-one, which Telegram numbers by that person.
  pub fn private_room_of(&self, chat_id: i64) -> Option<i64> {
    (chat_id > 0).then_some(chat_id)
  }

  /// Whether a room is one this provider has handed a message of over or sent into.
  pub fn has_seen(&self, chat_id: i64) -> bool {
    self.seen.has_room(chat_id)
  }

  /// Whether the bot account is in a room (docs/telegram.md, 「The rooms」): the owner's one-to-one always,
  /// another one-to-one once its person has written to the bot privately, and a group or a channel as Telegram
  /// last said within `PRESENCE_FRESH`, asked again past it; where Telegram cannot be asked, a room the provider
  /// has seen.
  pub async fn bot_in(&self, chat_id: i64) -> bool {
    if chat_id == self.owner_id {
      return true;
    }

    if let Some(person) = self.private_room_of(chat_id) {
      return self.seen.has_written_privately(person);
    }

    let kept = self.presence.lock().unwrap_or_else(PoisonError::into_inner).get(&chat_id).copied();

    if let Some((present, _)) = kept.filter(|(_, at)| at.elapsed() < PRESENCE_FRESH) {
      return present;
    }

    match self.ask_presence(chat_id).await {
      Some(present) => {
        self
          .presence
          .lock()
          .unwrap_or_else(PoisonError::into_inner)
          .insert(chat_id, (present, Instant::now()));
        present
      }
      None => self.has_seen(chat_id),
    }
  }

  // Telegram asked whether the bot is a member of a group or a channel. Null where Telegram gave no answer
  // either way.
  async fn ask_presence(&self, chat_id: i64) -> Option<bool> {
    let answer = self
      .api
      .answered("getChatMember", &json!({ "chat_id": chat_id, "user_id": self.me() }))
      .await
      .ok()?;

    match (answer.ok, answer.error_code) {
      (true, _) => Some(answer.result.as_ref().is_some_and(still_member)),
      // The chat is not one the bot has, or the bot is not or no longer a member.
      (false, Some(400 | 403)) => Some(false),
      _ => None,
    }
  }

  // A message the bot account read in a room lets go of what Telegram last said of the bot there.
  fn forget_presence(&self, chat_id: i64) {
    self.presence.lock().unwrap_or_else(PoisonError::into_inner).remove(&chat_id);
  }

  /// People seen speaking in a room, kept as the room's senders, whom the bot account checks as its members.
  pub fn spoke(&self, chat_id: i64, senders: impl IntoIterator<Item = i64>) {
    self.seen.spoke(chat_id, senders);
  }

  /// A room's members as the bot account tells them (docs/telegram.md, 「The members of a room」): a one-to-one's
  /// person and the bot, and a group's or a channel's administrators and the senders seen in it still there.
  pub async fn bot_members(&self, chat_id: i64) -> Result<Vec<ChatMember>, String> {
    match self.private_room_of(chat_id) {
      Some(person) => Ok(private_members(&self.api, person, self.me()).await),
      None => group_members(&self.api, chat_id, &self.seen.senders_of(chat_id)).await,
    }
  }

  /// What Telegram calls a room, or null where it cannot or will not say.
  pub async fn room_title(&self, chat_id: i64) -> Option<String> {
    let mut titles = self.titles.lock().await;
    let kept = titles
      .get(&chat_id)
      .filter(|(_, answered, at)| at.elapsed() < if *answered { TITLE_FRESH } else { UNANSWERED_FRESH });

    if let Some((title, _, _)) = kept {
      return title.clone();
    }

    let chat: Result<Value, _> = self.api.call("getChat", &json!({ "chat_id": chat_id })).await;
    let title = chat
      .as_ref()
      .ok()
      .and_then(|chat| chat.get("title").and_then(Value::as_str).map(str::to_owned));

    titles.insert(chat_id, (title.clone(), chat.is_ok(), Instant::now()));

    title
  }

  /// A room of the bot's as the protocol shapes it, under `title` and taking `modes`: the number the bot's words
  /// carry there, its kind and the person of a one-to-one.
  pub fn room_of(&self, chat_id: i64, title: Option<String>, modes: Vec<ChatVoice>) -> ProviderRoom {
    ProviderRoom {
      id: chat_id,
      owns: true,
      identity: Some(self.me()),
      kind: self.room_kind(chat_id),
      private_to: self.private_room_of(chat_id),
      title,
      modes: Some(modes),
    }
  }

  /// A room of the owner's account the bot is not in, as the protocol shapes it under `title` (docs/telegram.md,
  /// 「The rooms」). Where the receiving side reads it, it can be answered in the avatar: it takes the owner's own
  /// voice and the avatar's, the bot's number stands as the agent's identity, and no person of a one-to-one is
  /// named, since the person speaks with the owner there. Otherwise it is read-only: the owner's own voice alone,
  /// and no number of the provider's own, so its identity is null.
  pub fn account_room_of(&self, chat_id: i64, title: Option<String>, read: bool) -> ProviderRoom {
    if read {
      return ProviderRoom {
        private_to: None,
        ..self.room_of(chat_id, title, vec![ChatVoice::HumanReader, ChatVoice::HumanAvatar])
      };
    }

    ProviderRoom {
      identity: None,
      ..self.room_of(chat_id, title, vec![ChatVoice::HumanReader])
    }
  }

  /// Every room the provider has seen.
  pub fn seen_rooms(&self) -> Vec<i64> {
    self.seen.sighted().rooms.into_iter().collect()
  }

  /// A room the bot is in as the protocol answers it, taking `modes`, its title asked of Telegram.
  pub async fn bot_room(&self, chat_id: i64, modes: Vec<ChatVoice>) -> ProviderRoom {
    self.room_of(chat_id, self.room_title(chat_id).await, modes)
  }

  /// The Bot API this account asks.
  pub fn api(&self) -> &BotApi {
    &self.api
  }

  /// The notes of what the sends in the owner's voice went out as.
  pub fn echoes(&self) -> &Arc<Echoes> {
    &self.echoes
  }

  /// The id a message's picture or file crossed with, where this account handed one over.
  pub fn file_id_of(&self, chat_id: i64, message_id: i64) -> Option<String> {
    self.file_ids.of(chat_id, message_id)
  }

  /// The folder this account's files stand in on this machine's disk: what the gathering writes a gathered file
  /// to, and what the protocol server writes the file parts of a multipart send to as they arrive.
  pub fn spool(&self) -> PathBuf {
    self.gatherer.spool()
  }

  // The files gathered for a message, handed over as what its promise came to. A file the gathering wrote to
  // this machine's disk goes with the patch and is removed once the receiving side took it.
  async fn hand_gathered(
    gatherer: &Gatherer,
    receiver: &dyn ChatReceiver,
    room_id: i64,
    message_id: i64,
    gathered: Result<Vec<MessageFile>, String>,
  ) {
    // What the guard removes is its own list, dropped when the patch has been taken.
    let _held = match &gathered {
      Ok(files) => gatherer.hold(files),
      Err(_) => gatherer.hold(&[]),
    };
    let patch = match gathered {
      Ok(files) if !files.is_empty() => GatheredPatch::kept(files),
      Ok(_) => GatheredPatch::not_kept(POLICY.gathering.file_not_kept.clone()),
      Err(why) => {
        eprintln!("telegram: keeping the file of chat {room_id} message {message_id} failed: {why}");
        GatheredPatch::not_kept(POLICY.gathering.file_not_kept.clone())
      }
    };

    if let Err(why) = receiver.gathered(room_id, message_id, patch).await {
      eprintln!("telegram: chat {room_id} message {message_id}: the gathered file was not taken: {why}");
    }
  }

  // What is promised for a message is gathered on its own, so the poll never stands still for a file.
  fn gather_later(&self, receiver: Arc<dyn ChatReceiver>, raw: Value, file: bool, assets: bool) {
    let gatherer = self.gatherer.clone();
    let room_id = raw.pointer("/chat/id").and_then(Value::as_i64).unwrap_or_default();
    let message_id = raw.get("message_id").and_then(Value::as_i64).unwrap_or_default();

    tokio::spawn(async move {
      let mut files = if assets { gatherer.assets(room_id, message_id, &raw).await } else { Vec::new() };
      let gathered = if file {
        gatherer.file(room_id, message_id, &raw).await.map(|item| {
          files.extend(item);
          files
        })
      } else {
        Ok(files)
      };

      Self::hand_gathered(gatherer.as_ref(), receiver.as_ref(), room_id, message_id, gathered).await;
    });
  }

  // Whether a message the bot account read, showing `act`, is the owner's and the echo of a send in his voice.
  async fn owners_echo(&self, act: OwnerAct, message: &Message) -> bool {
    if !message.from.as_ref().is_some_and(|from| from.id == self.owner_id) {
      return false;
    }

    let words = message.text.as_deref().or(message.caption.as_deref()).unwrap_or_default();

    self.echoes.echo(act, message.chat.id, message.message_id, words).await
  }

  /// One new message: handed over as it reads, its picture with it, its file and assets promised and gathered
  /// after; the echo of a send in the owner's voice is not handed over.
  pub async fn message(&self, receiver: Arc<dyn ChatReceiver>, raw: Value) -> Result<(), String> {
    let message: Message = serde_json::from_value(raw.clone()).map_err(|why| why.to_string())?;
    let chat_id = message.chat.id;
    let message_id = message.message_id;

    self.forget_presence(chat_id);

    // A person writing in their private chat with the bot makes that one-to-one the bot's.
    if message.from.as_ref().is_some_and(|from| from.id == chat_id) {
      self.seen.wrote_privately(chat_id);
    }

    // The owner's own message, or the service message of his pin, made in the owner's voice: the sender holds
    // it already.
    let act = if message.pinned_message.is_some() { OwnerAct::Pinned } else { OwnerAct::Said };

    if self.owners_echo(act, &message).await {
      return Ok(());
    }

    self.lines.bots(&message);

    let listened = self.listened(chat_id);
    let assets = listened && Gatherer::has_assets(&raw);
    let mut reading = read(&message, self.me(), self.owner_id, listened);

    // Named before anything is awaited, in the order the updates came.
    if let (Some(group), Some(handed)) = (message.media_group_id.as_deref(), reading.message_mut()) {
      let mut albums = self.albums.lock().unwrap_or_else(PoisonError::into_inner);

      handed.album = Some(albums.name(chat_id, group, message_id));
    }

    if let (Some(handed), Some(kept)) =
      (reading.message_mut(), listened.then(|| crate::reading::metadata(&raw)).flatten())
    {
      handed.context.metadata = Some(kept);
    }

    // A line the avatar sent, read back past its echo, is the agent's own (docs/telegram.md, 「The avatar」).
    if let Some(handed) = reading.message_mut() {
      self.avatar.take_as_agent_s(handed);
    }

    match reading {
      Reading::Said(mut handed) => {
        handed.gathering = assets;
        self.seen.message(chat_id, handed.sender_id);
        receiver.hand_over(chat_id, message_id, handed).await.map_err(|why| why.to_string())?;

        if assets {
          self.gather_later(receiver, raw, false, true);
        }

        Ok(())
      }
      Reading::Picture(mut handed, picture) => {
        let mut files = Vec::new();

        if let Some(picture) = &picture {
          files.extend(self.gatherer.picture(chat_id, message_id, message.date, picture).await);
        }

        if assets {
          files.extend(self.gatherer.assets(chat_id, message_id, &raw).await);
        }

        handed.context.carries_file = !files.is_empty();
        handed.files = files;
        self.seen.message(chat_id, handed.sender_id);

        receiver
          .hand_over(chat_id, message_id, handed)
          .await
          .map(drop)
          .map_err(|why| why.to_string())
      }
      Reading::Attachment(mut handed) => {
        handed.gathering = true;
        self.seen.message(chat_id, handed.sender_id);

        receiver.hand_over(chat_id, message_id, handed).await.map_err(|why| why.to_string())?;
        self.gather_later(receiver, raw, true, assets);

        Ok(())
      }
      Reading::FileOnly | Reading::Nothing => Ok(()),
    }
  }

  /// One edited message: said again with its words, its formatting and the files it carries that were not
  /// gathered already as the same files — a file that changed under its name is gathered again.
  pub async fn edited(&self, receiver: Arc<dyn ChatReceiver>, raw: Value) -> Result<(), String> {
    let message: Message = serde_json::from_value(raw.clone()).map_err(|why| why.to_string())?;
    let chat_id = message.chat.id;
    let message_id = message.message_id;

    self.forget_presence(chat_id);

    self.lines.edited(&message);

    // The owner's own edit, made in the owner's voice: the sender holds it already.
    if self.owners_echo(OwnerAct::Edited, &message).await || !self.listened(chat_id) {
      return Ok(());
    }

    let edit = read_edit(&message, &raw, self.me(), self.owner_id);
    let Some(mut handed) = edit.said else {
      return Ok(());
    };

    self.avatar.take_as_agent_s(&mut handed);
    let mut files = self.gatherer.assets(chat_id, message_id, &raw).await;

    if edit.carries_file {
      match self.gatherer.file(chat_id, message_id, &raw).await {
        Ok(file) => files.extend(file),
        Err(why) => {
          eprintln!("telegram: keeping the file of chat {chat_id} message {message_id} failed: {why}")
        }
      }
    }

    handed.context.carries_file = edit.carries_file;
    handed.files = files;
    self.seen.message(chat_id, handed.sender_id);

    receiver
      .hand_over(chat_id, message_id, handed)
      .await
      .map(drop)
      .map_err(|why| why.to_string())
  }

  // One update, as it came; a failure is said and the next one read.
  async fn handle(&self, receiver: &Arc<dyn ChatReceiver>, update: Update) {
    if let Some(reacted) = update.message_reaction.as_ref() {
      if let Some(change) = reaction_change(reacted)
        && let Err(why) = receiver.reactions_changed(change).await
      {
        eprintln!("telegram: update {}: the reaction was not handed over: {why}", update.update_id);
      }

      return;
    }

    let handled = match (update.message, update.edited_message) {
      (Some(message), _) => self.message(receiver.clone(), message).await,
      (None, Some(edited)) => self.edited(receiver.clone(), edited).await,
      (None, None) => Ok(()),
    };

    if let Err(why) = handled {
      eprintln!("telegram: update {}: {why}", update.update_id);
    }
  }

  /// The bot account read: who it is asked first, asking again until the platform answers, then the long poll
  /// for as long as the process runs, every update handed over in order.
  pub async fn read(self: Arc<Self>, receiver: Arc<dyn ChatReceiver>) {
    loop {
      match self.api.call::<Me>("getMe", &json!({})).await {
        Ok(me) => {
          self.me.store(me.id, Ordering::Relaxed);
          *self.standing.lock().unwrap_or_else(PoisonError::into_inner) = None;
          println!("telegram: reading as @{} (long polling)", me.username.unwrap_or_default());
          break;
        }
        Err(unanswered) => {
          *self.standing.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(format!("the Bot API did not say who the bot is: {}", unanswered.why));
          eprintln!("telegram: getMe: {}; asking again", unanswered.why);
          tokio::time::sleep(unanswered.wait).await;
        }
      }
    }

    let mut offset: Option<i64> = None;

    loop {
      let mut asked =
        json!({ "timeout": 30, "allowed_updates": ["message", "edited_message", "message_reaction"] });

      if let Some(offset) = offset {
        asked["offset"] = json!(offset);
      }

      match self.api.call::<Vec<Update>>("getUpdates", &asked).await {
        Ok(updates) => {
          for update in updates {
            offset = Some(update.update_id + 1);
            self.handle(&receiver, update).await;
          }
        }
        Err(unanswered) => {
          eprintln!("telegram: getUpdates: {}; asking again", unanswered.why);
          tokio::time::sleep(unanswered.wait).await;
        }
      }
    }
  }
}

impl Telegram {
  /// A supervisor's bot read for its one-to-ones alone (docs/telegram.md, 「A bot for each supervisor」): the long
  /// poll of its messages and edits, each in a private chat handed over under the room of its own that one-to-one
  /// is given, and every other update passed over, since a group's words are the service's own bot's to read.
  pub async fn read_private(
    self: Arc<Self>,
    receiver: Arc<dyn ChatReceiver>,
    supervisor: String,
    rooms: Arc<crate::dm_rooms::DmRooms>,
  ) {
    let mut offset: Option<i64> = None;

    loop {
      let mut asked = json!({ "timeout": 30, "allowed_updates": ["message", "edited_message"] });

      if let Some(offset) = offset {
        asked["offset"] = json!(offset);
      }

      match self.api.call::<Vec<Update>>("getUpdates", &asked).await {
        Ok(updates) => {
          for update in updates {
            offset = Some(update.update_id + 1);

            let (raw, edited) = match (update.message, update.edited_message) {
              (Some(message), _) => (message, false),
              (None, Some(edited)) => (edited, true),
              (None, None) => continue,
            };
            let Some(raw) = private_in_room(raw, &supervisor, &rooms) else { continue };
            let handled = if edited {
              self.edited(receiver.clone(), raw).await
            } else {
              self.message(receiver.clone(), raw).await
            };

            if let Err(why) = handled {
              eprintln!("telegram: {supervisor}'s bot: update {}: {why}", update.update_id);
            }
          }
        }
        Err(unanswered) => {
          eprintln!("telegram: {supervisor}'s bot: getUpdates: {}; asking again", unanswered.why);
          tokio::time::sleep(unanswered.wait).await;
        }
      }
    }
  }
}

/// A message of a supervisor's bot's private chat, its chat numbered as the room that one-to-one is given; none for
/// a message of any other chat.
pub fn private_in_room(
  mut raw: Value,
  supervisor: &str,
  rooms: &crate::dm_rooms::DmRooms,
) -> Option<Value> {
  let chat = raw.get("chat")?;

  if chat.get("type").and_then(Value::as_str) != Some("private") {
    return None;
  }

  let person = chat.get("id").and_then(Value::as_i64)?;

  raw["chat"]["id"] = json!(rooms.room_of(supervisor, person));
  Some(raw)
}

#[async_trait]
impl Presence for Telegram {
  async fn bot_in(&self, chat_id: i64) -> bool {
    Telegram::bot_in(self, chat_id).await
  }
}
