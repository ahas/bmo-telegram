//! The bot account's reading (docs/telegram.md, 「Reading a room」): the long poll of the Bot API, and each
//! update turned into what the receiver is handed over — the words at once, a picture with its bytes, a file
//! promised and gathered after. The room questions the chat protocol asks are answered here too.

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use bmo_chat::{ChatReceiver, GatheredPatch, MessageFile, ProviderRoom, RoomKind};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::albums::AlbumNames;
use crate::bot_api::BotApi;
use crate::echoes::Echoes;
use crate::file_ids::FileIds;
use crate::gather::Gatherer;
use crate::kinds::Message;
use crate::policy::POLICY;
use crate::reader::ReaderClient;
use crate::reading::{Reading, SAFE_INTEGER, read, read_edit};
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
  // Why the bot account is not reading yet, or null once it is.
  standing: Mutex<Option<String>>,
}

/// One update the Bot API delivered.
#[derive(Deserialize)]
struct Update {
  update_id: i64,
  message: Option<Value>,
  edited_message: Option<Value>,
}

#[derive(Deserialize)]
struct Me {
  id: i64,
  username: Option<String>,
}

/// Why the bot account cannot run at all.
pub const TOKEN_MISSING: &str = "BMO_BOT_TOKEN not set";

impl Telegram {
  /// The bot account with its token, asking the Bot API at `api`, the owner's number, the reader where one
  /// stands behind it, the record of what it has seen and that of the ids its files crossed with.
  pub fn new(
    token: &str,
    api: &str,
    owner_id: i64,
    reader: Option<ReaderClient>,
    seen: Arc<Seen>,
    file_ids: Arc<FileIds>,
  ) -> Self {
    let api = BotApi::new(api, token);
    let me = api.token_identity().unwrap_or_default();

    Self {
      gatherer: Arc::new(Gatherer::new(api.clone(), reader, file_ids.clone())),
      file_ids,
      api,
      token_set: !token.trim().is_empty(),
      owner_id,
      me: AtomicI64::new(me),
      albums: Mutex::default(),
      seen,
      echoes: Arc::default(),
      standing: Mutex::new(Some("the bot account has not been read yet".to_owned())),
    }
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
  pub fn owns_room(&self, chat_id: i64) -> bool {
    chat_id == self.owner_id || self.seen.has_room(chat_id)
  }

  /// What Telegram calls a room, or null where it cannot or will not say.
  pub async fn room_title(&self, chat_id: i64) -> Option<String> {
    let chat: Value = self.api.call("getChat", &json!({ "chat_id": chat_id })).await.ok()?;

    chat.get("title").and_then(Value::as_str).map(str::to_owned)
  }

  /// What the provider knows of a room: its number space, the number the bot's words carry there, the person
  /// of a one-to-one and what Telegram calls it. Null where the room is none this provider has seen.
  pub async fn room(&self, chat_id: i64) -> Option<ProviderRoom> {
    self.owns_room(chat_id).then(|| ProviderRoom {
      id: chat_id,
      owns: true,
      identity: Some(self.me()),
      kind: self.room_kind(chat_id),
      private_to: self.private_room_of(chat_id),
      title: None,
    })
  }

  /// The room as the protocol answers it, its title asked of Telegram where the room is known.
  pub async fn provider_room(&self, chat_id: i64) -> Option<ProviderRoom> {
    let mut room = self.room(chat_id).await?;

    room.title = self.room_title(chat_id).await;

    Some(room)
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

  // The files gathered for a message, handed over as what its promise came to.
  async fn hand_gathered(
    receiver: &dyn ChatReceiver,
    room_id: i64,
    message_id: i64,
    gathered: Result<Vec<MessageFile>, String>,
  ) {
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

      Self::hand_gathered(receiver.as_ref(), room_id, message_id, gathered).await;
    });
  }

  /// One new message: handed over as it reads, its picture with it, its file and assets promised and gathered
  /// after; the echo of a send in the owner's voice is not handed over.
  pub async fn message(&self, receiver: Arc<dyn ChatReceiver>, raw: Value) -> Result<(), String> {
    let message: Message = serde_json::from_value(raw.clone()).map_err(|why| why.to_string())?;
    let chat_id = message.chat.id;
    let message_id = message.message_id;

    // The owner's own message, sent in the owner's voice: the sender holds it already.
    if message.from.as_ref().is_some_and(|from| from.id == self.owner_id) {
      let words = message.text.as_deref().or(message.caption.as_deref()).unwrap_or_default();

      if self.echoes.echo(chat_id, message_id, words).await {
        return Ok(());
      }
    }

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

    if !self.listened(chat_id) {
      return Ok(());
    }

    let edit = read_edit(&message, &raw, self.me(), self.owner_id);
    let Some(mut handed) = edit.said else {
      return Ok(());
    };
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
      let mut asked = json!({ "timeout": 30, "allowed_updates": ["message", "edited_message"] });

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
