//! What the bot account's tests share: a stand-in of the Bot API that keeps what it was asked, a receiver that
//! keeps every hand-over and says when one came, and a router served on the loopback. The sends' own stand-ins
//! are in `speaking`.

pub mod speaking;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use axum::Router;
use axum::body::Bytes;
use axum::extract::Request;
use axum::response::{IntoResponse, Json, Response};
use bmo_chat::{
  AvatarChange, ChatFailure, ChatReceiver, GatheredPatch, HandedMessage, HeldMessage, MessageFile, Page,
  ReceiverRoom, RoomQuery, RoomsQuery,
};
use serde_json::{Value, json};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub const TOKEN: &str = "777:fixture";
pub const OWNER: i64 = 42;

/// A router served on the loopback at a port of its own, for as long as the test runs.
pub async fn serve(router: Router) -> u16 {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
  let port = listener.local_addr().expect("a bound address").port();

  tokio::spawn(async move { axum::serve(listener, router).await.expect("the stand-in serves") });

  port
}

/// One method the stand-in was asked, with its parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Called {
  pub method: String,
  pub body: Value,
}

/// The Bot API's stand-in: who the bot is, the updates it delivers once, a file's path and bytes by its id, and
/// the custom emoji it is asked for.
#[derive(Default)]
pub struct BotApiStandIn {
  pub called: Mutex<Vec<Called>>,
  pub updates: Mutex<Option<Value>>,
  pub files: Mutex<HashMap<String, Vec<u8>>>,
}

impl BotApiStandIn {
  pub fn methods(&self) -> Vec<String> {
    self.called.lock().unwrap().iter().map(|called| called.method.clone()).collect()
  }

  pub fn forget(&self) {
    self.called.lock().unwrap().clear();
  }

  fn answer(&self, method: &str, body: &Value) -> Response {
    match method {
      "getMe" => Json(json!({ "ok": true, "result": { "id": 7_000_000_001_i64, "is_bot": true, "first_name": "Helper", "username": "helper_bot" } })).into_response(),
      "getFile" => {
        let id = body["file_id"].as_str().unwrap_or_default();

        Json(json!({ "ok": true, "result": { "file_id": id, "file_unique_id": format!("u-{id}"), "file_path": format!("files/{id}") } })).into_response()
      }
      "getCustomEmojiStickers" => {
        let stickers: Vec<Value> = body["custom_emoji_ids"]
          .as_array()
          .cloned()
          .unwrap_or_default()
          .iter()
          .map(|id| {
            let id = id.as_str().unwrap_or_default();

            json!({ "file_id": format!("emoji-file-{id}"), "file_unique_id": format!("emoji-unique-{id}"), "type": "custom_emoji",
              "custom_emoji_id": id, "emoji": "😀", "is_animated": false, "is_video": false, "width": 100, "height": 100 })
          })
          .collect();

        Json(json!({ "ok": true, "result": stickers })).into_response()
      }
      _ => Json(json!({ "ok": false, "error_code": 404, "description": "Not Found" })).into_response(),
    }
  }

  // The bytes of a file by its path: the bytes given for its id, or bytes naming the path.
  fn file(&self, path: &str) -> Vec<u8> {
    let id = path.rsplit('/').next().unwrap_or_default();

    self
      .files
      .lock()
      .unwrap()
      .get(id)
      .cloned()
      .unwrap_or_else(|| format!("bytes of {path}").into_bytes())
  }
}

/// The stand-in served, its address the one the bot account is pointed at.
pub async fn bot_api(stand_in: Arc<BotApiStandIn>) -> String {
  let router = Router::new().fallback(move |request: Request| {
    let stand_in = stand_in.clone();

    async move {
      let path = request.uri().path().to_owned();
      let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .unwrap_or_else(|_| Bytes::new());

      if let Some(file) = path.strip_prefix(&format!("/file/bot{TOKEN}/")) {
        return stand_in.file(file).into_response();
      }

      let method = path.rsplit('/').next().unwrap_or_default().to_owned();
      let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);

      if method == "getUpdates" {
        // The updates go out once; after them a long poll that never ends while the test runs.
        let updates = stand_in.updates.lock().unwrap().take();

        return match updates {
          Some(updates) => Json(json!({ "ok": true, "result": updates })).into_response(),
          None => std::future::pending::<Response>().await,
        };
      }

      stand_in.called.lock().unwrap().push(Called {
        method: method.clone(),
        body: body.clone(),
      });

      stand_in.answer(&method, &body)
    }
  });

  format!("http://127.0.0.1:{}", serve(router).await)
}

/// A receiver that keeps every hand-over by the route it came on, and says when each came. Its rooms and its
/// refusals are set by the test that needs them.
pub struct KeptReceiver {
  pub handed: Mutex<Vec<(String, Value)>>,
  pub held: Mutex<HashMap<(i64, i64), HandedMessage>>,
  pub rooms: Mutex<Vec<ReceiverRoom>>,
  pub avatar_changes: Mutex<Vec<AvatarChange>>,
  pub reaction_changes: Mutex<Vec<bmo_chat::ReactionChange>>,
  pub deletions: Mutex<Vec<bmo_chat::Deletion>>,
  pub typing_marks: Mutex<Vec<bmo_chat::TypingMark>>,
  pub presence_changes: Mutex<Vec<bmo_chat::PresenceChange>>,
  pub hand_over_failure: Mutex<Option<String>>,
  pub room_failure: Mutex<Option<String>>,
  pub rooms_failure: Mutex<Option<String>>,
  pub avatars_failure: Mutex<Option<String>>,
  came: UnboundedSender<(String, Value)>,
}

impl KeptReceiver {
  pub fn new() -> (Arc<Self>, UnboundedReceiver<(String, Value)>) {
    let (came, heard) = unbounded_channel();

    (
      Arc::new(Self {
        handed: Mutex::default(),
        held: Mutex::default(),
        rooms: Mutex::default(),
        avatar_changes: Mutex::default(),
        reaction_changes: Mutex::default(),
        deletions: Mutex::default(),
        typing_marks: Mutex::default(),
        presence_changes: Mutex::default(),
        hand_over_failure: Mutex::default(),
        room_failure: Mutex::default(),
        rooms_failure: Mutex::default(),
        avatars_failure: Mutex::default(),
        came,
      }),
      heard,
    )
  }

  fn keep(&self, route: &str, body: &impl serde::Serialize) -> Result<(), ChatFailure> {
    let body = serde_json::to_value(body)?;

    self.handed.lock().unwrap().push((route.to_owned(), body.clone()));
    let _ = self.came.send((route.to_owned(), body));

    Ok(())
  }
}

#[async_trait]
impl ChatReceiver for KeptReceiver {
  async fn hand_over(
    &self,
    room_id: i64,
    message_id: i64,
    message: HandedMessage,
  ) -> Result<HeldMessage, ChatFailure> {
    if let Some(why) = self.hand_over_failure.lock().unwrap().clone() {
      return Err(why.into());
    }

    let mut kept = serde_json::to_value(&message)?;
    kept["roomId"] = json!(room_id);
    kept["messageId"] = json!(message_id);

    self.keep("message", &kept)?;
    let created = self.held.lock().unwrap().insert((room_id, message_id), message.clone()).is_none();

    Ok(HeldMessage { created, message })
  }

  async fn gathered(
    &self,
    room_id: i64,
    message_id: i64,
    patch: GatheredPatch,
  ) -> Result<HandedMessage, ChatFailure> {
    self.keep("gathered", &patch)?;
    let mut held = self.held.lock().unwrap();
    let message = held.get_mut(&(room_id, message_id)).ok_or("the receiver holds no such message")?;

    message.files = patch.files;
    message.gather_failure = patch.gather_failure;
    message.gathering = false;

    Ok(message.clone())
  }

  async fn avatars_changed(&self, change: AvatarChange) -> Result<(), ChatFailure> {
    if let Some(why) = self.avatars_failure.lock().unwrap().clone() {
      return Err(why.into());
    }

    self.avatar_changes.lock().unwrap().push(change);

    Ok(())
  }

  async fn reactions_changed(&self, change: bmo_chat::ReactionChange) -> Result<(), ChatFailure> {
    self.reaction_changes.lock().unwrap().push(change);

    Ok(())
  }

  async fn deleted(&self, deletion: bmo_chat::Deletion) -> Result<(), ChatFailure> {
    self.deletions.lock().unwrap().push(deletion);

    Ok(())
  }

  async fn typing_marked(&self, mark: bmo_chat::TypingMark) -> Result<(), ChatFailure> {
    self.typing_marks.lock().unwrap().push(mark);

    Ok(())
  }

  async fn presence_changed(&self, change: bmo_chat::PresenceChange) -> Result<(), ChatFailure> {
    self.presence_changes.lock().unwrap().push(change);

    Ok(())
  }

  async fn room(&self, room_id: i64, _query: RoomQuery) -> Result<Option<ReceiverRoom>, ChatFailure> {
    if let Some(why) = self.room_failure.lock().unwrap().clone() {
      return Err(why.into());
    }

    Ok(self.rooms.lock().unwrap().iter().find(|room| room.id == room_id).copied())
  }

  async fn rooms(&self, _query: RoomsQuery) -> Result<Page<ReceiverRoom>, ChatFailure> {
    if let Some(why) = self.rooms_failure.lock().unwrap().clone() {
      return Err(why.into());
    }

    Ok(Page::whole(self.rooms.lock().unwrap().clone()))
  }
}

/// The next hand-over, waited for no longer than a hang would take to show.
pub async fn next(heard: &mut UnboundedReceiver<(String, Value)>) -> (String, Value) {
  tokio::time::timeout(Duration::from_secs(20), heard.recv())
    .await
    .expect("a hand-over came")
    .expect("the receiver stands")
}

/// A message file as it crossed the wire, its bytes read back.
pub fn file_of(item: &Value) -> MessageFile {
  serde_json::from_value(item.clone()).expect("a message file")
}
