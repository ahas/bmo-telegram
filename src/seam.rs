//! The service's routes the reader calls (docs/boundary-api.md, 「The service's routes, called by the
//! reader」): what the owner's account read handed on to the receiver, and what the receiver says of its rooms,
//! asked here because the service is the only process that talks to another unit. Every route takes the seam's
//! token. The router holds no fallback of its own, so the service mounts it beside bmo-chat's provider routes.

use std::sync::{Arc, Mutex, PoisonError};

use async_trait::async_trait;
use axum::Router;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use bmo_chat::{
  AvatarChange, ChatFailure, ChatReceiver, ErrorCode, GatheredPatch, HandedMessage, MessageOrigin,
  PersonPresence, PresenceChange, ReceiverRoom, RoomQuery, RoomsQuery, TypingMark, WireError,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::VIA;
use crate::avatar::AvatarLines;
use crate::lines::Lines;
use crate::policy::POLICY;
use crate::seen::{Seen, Sighted};

/// One hand-over the reader makes: a message, whatever its origin, or the files a message's gathering
/// promised.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", rename_all_fields = "camelCase")]
pub enum Inbound {
  Message {
    room_id: i64,
    message_id: i64,
    message: Box<HandedMessage>,
  },
  Gathered {
    room_id: i64,
    message_id: i64,
    patch: GatheredPatch,
  },
}

/// Why a part the reader runs stands down, where it does: the history walk and the word of changed profile
/// pictures, each null while it stands.
#[derive(Debug, Default)]
pub struct Standing {
  history: Mutex<Option<String>>,
  avatars: Mutex<Option<String>>,
}

impl Standing {
  fn set(part: &Mutex<Option<String>>, reason: Option<String>) {
    *part.lock().unwrap_or_else(PoisonError::into_inner) = reason;
  }

  fn read(part: &Mutex<Option<String>>) -> Option<String> {
    part.lock().unwrap_or_else(PoisonError::into_inner).clone()
  }

  /// Why the history walk stands down, or null.
  pub fn history(&self) -> Option<String> {
    Self::read(&self.history)
  }

  /// Why the word of changed profile pictures stands down, or null.
  pub fn avatars(&self) -> Option<String> {
    Self::read(&self.avatars)
  }
}

/// Whether the bot account is in a room, as the provider knows it (docs/telegram.md, 「The rooms」).
#[async_trait]
pub trait Presence: Send + Sync {
  async fn bot_in(&self, chat_id: i64) -> bool;
}

/// What the seam's routes stand on.
pub struct Seam {
  pub token: String,
  pub receiver: Arc<dyn ChatReceiver>,
  pub seen: Arc<Seen>,
  pub standing: Arc<Standing>,
  pub presence: Arc<dyn Presence>,
  /// The avatar's lines, so one read back through the owner's account goes on as the agent's own.
  pub avatar: AvatarLines,
  /// Whose numbering each line of a room numbered per account stands under: a line the reader hands over is
  /// under the owner's account's.
  pub lines: Arc<Lines>,
}

/// Whether the receiving side asked for a room to be read through the owner's account: its `read`. Being
/// listened to is no such request.
pub fn read_asked(held: &ReceiverRoom) -> bool {
  held.read
}

/// The seam's routes over `seam`.
pub fn seam_router(seam: Arc<Seam>) -> Router {
  Router::new()
    .route("/reader/inbound", post(inbound))
    .route("/reader/listened", post(listened))
    .route("/reader/rooms", get(rooms))
    .route("/reader/seen", get(seen))
    .route("/reader/avatars", post(avatars))
    .route("/reader/reactions", post(reactions))
    .route("/reader/deleted", post(deleted))
    .route("/reader/typing", post(typing))
    .route("/reader/presence", post(presence))
    // A gathered file crosses whole in its hand-over, so the seam takes a body as large as one the receiver keeps.
    .layer(DefaultBodyLimit::max(POLICY.gathering.seam_body_bytes))
    .layer(from_fn_with_state(seam.clone(), authorized))
    .with_state(seam)
}

fn failure(code: ErrorCode, error: impl Into<String>) -> Response {
  WireError::new(code, error).into_response()
}

async fn authorized(State(seam): State<Arc<Seam>>, request: Request, next: Next) -> Response {
  let expected = format!("Bearer {}", seam.token);
  let given = request
    .headers()
    .get(axum::http::header::AUTHORIZATION)
    .and_then(|value| value.to_str().ok());

  if given == Some(expected.as_str()) {
    next.run(request).await
  } else {
    failure(ErrorCode::Unauthorized, "the token is missing or wrong")
  }
}

// A body read as the route takes it, or why it is not the shape the route takes.
fn body_of<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, String> {
  serde_json::from_slice(body).map_err(|why| why.to_string())
}

fn handed(result: Result<(), ChatFailure>) -> Response {
  match result {
    Ok(()) => Json(json!({})).into_response(),
    Err(why) => failure(ErrorCode::Failed, why.to_string()),
  }
}

async fn inbound(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let inbound: Inbound = match body_of(&body) {
    Ok(inbound) => inbound,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };

  match inbound {
    Inbound::Message {
      room_id,
      message_id,
      mut message,
    } => {
      let history = message.origin == MessageOrigin::History;

      // A room the bot is not in carries the facts its own answer gives (docs/telegram.md, 「The rooms」): where the
      // receiver reads it, the agent's identity and no person of a one-to-one; where it does not, no words of the
      // provider's own stand there.
      if !seam.presence.bot_in(room_id).await {
        let query = RoomQuery {
          via: VIA.to_owned(),
        };
        let read = matches!(seam.receiver.room(room_id, query).await, Ok(Some(held)) if read_asked(&held));

        if read {
          message.room.private_to = None;
        } else {
          message.room.identity = None;
        }
      }

      // A line the avatar sent goes on as the agent's own (docs/telegram.md, 「The avatar」).
      seam.avatar.take_as_agent_s(&mut message);
      seam.lines.owners(room_id, message_id);

      if message.origin == MessageOrigin::Live {
        seam.seen.message(room_id, message.sender_id);
      }

      match seam.receiver.hand_over(room_id, message_id, *message).await {
        Ok(held) if history => {
          Standing::set(&seam.standing.history, None);
          Json(json!({ "kept": held.created })).into_response()
        }
        Ok(_) => Json(json!({})).into_response(),
        Err(why) => failure(ErrorCode::Failed, why.to_string()),
      }
    }
    Inbound::Gathered {
      room_id,
      message_id,
      patch,
    } => handed(seam.receiver.gathered(room_id, message_id, patch).await.map(drop)),
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Room {
  chat_id: i64,
}

// Whether the receiver listens to a room; a room it holds nothing of is one it does not listen to, and a
// receiver that does not answer counts the room as listened.
async fn listened(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let room: Room = match body_of(&body) {
    Ok(room) => room,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };
  let query = RoomQuery {
    via: VIA.to_owned(),
  };
  let (listened, asked) = match seam.receiver.room(room.chat_id, query).await {
    Ok(held) => (held.as_ref().is_some_and(|held| held.listened), held.as_ref().is_some_and(read_asked)),
    Err(_) => (true, false),
  };
  // A person's words are the reader's to hand over only in a room the bot is not in that the receiving side
  // asked to be read; a person's typing and presence wherever the room is listened to, but in a room the bot is
  // not in only where the receiving side asked for it to be read (docs/telegram.md, 「Typing and presence」).
  let bot_in = (asked || listened) && seam.presence.bot_in(room.chat_id).await;
  let people = asked && !bot_in;
  let tells = listened && (asked || bot_in);

  Json(json!({ "listened": listened, "people": people, "tells": tells })).into_response()
}

// The rooms whose history may be walked, each with the oldest message the receiver holds of it.
async fn rooms(State(seam): State<Arc<Seam>>) -> Response {
  let query = RoomsQuery {
    via: VIA.to_owned(),
    after: None,
    limit: None,
  };

  match seam.receiver.rooms(query).await {
    Ok(page) => {
      Standing::set(&seam.standing.history, None);
      let rooms: Vec<Value> = page
        .items
        .into_iter()
        .map(|room| {
          json!({
            "chatId": room.id,
            "oldestMessageId": room.oldest_message_id,
            "oldestSentAt": room.oldest_sent_at,
          })
        })
        .collect();

      Json(json!({ "rooms": rooms })).into_response()
    }
    Err(why) => {
      let reason = format!("the history walk stands down: {why}");

      Standing::set(&seam.standing.history, Some(reason));
      failure(ErrorCode::NotReady, why.to_string())
    }
  }
}

// The rooms and the senders seen, whose pictures the reader keeps.
async fn seen(State(seam): State<Arc<Seam>>) -> Response {
  let Sighted { rooms, people, .. } = seam.seen.sighted();

  Json(json!({ "rooms": rooms, "people": people })).into_response()
}

#[derive(Deserialize)]
struct Changed {
  rooms: Vec<i64>,
  people: Vec<i64>,
}

// Word of changed profile pictures carried to the receiver; where it does not take it, the pictures are kept
// current all the same and the word stands down.
async fn avatars(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let changed: Changed = match body_of(&body) {
    Ok(changed) => changed,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };
  let change = AvatarChange {
    via: VIA.to_owned(),
    room_ids: changed.rooms,
    person_ids: changed.people,
  };
  let reason = seam
    .receiver
    .avatars_changed(change)
    .await
    .err()
    .map(|why| format!("word of changed profile pictures stands down: {why}"));

  Standing::set(&seam.standing.avatars, reason);

  Json(Value::Object(serde_json::Map::new())).into_response()
}

/// The body of `POST /reader/reactions`: a message's reactions as the account sees them, the message by its number
/// or its stamp (docs/boundary-api.md, 「The service's routes, called by the reader」).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReactionsSeen {
  room_id: i64,
  #[serde(default)]
  message_id: i64,
  #[serde(default)]
  stamp: Option<bmo_chat::MessageStamp>,
  reactions: Vec<bmo_chat::PersonReactions>,
  #[serde(default)]
  whole: bool,
}

async fn reactions(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let seen: ReactionsSeen = match body_of(&body) {
    Ok(seen) => seen,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };
  let change = bmo_chat::ReactionChange {
    via: VIA.to_owned(),
    room_id: seen.room_id,
    message_id: seen.message_id,
    stamp: seen.stamp,
    reactions: seen.reactions,
    whole: seen.whole,
  };

  match seam.receiver.reactions_changed(change).await {
    Ok(()) => Json(Value::Object(serde_json::Map::new())).into_response(),
    Err(why) => failure(ErrorCode::Failed, format!("the reactions were not handed over: {why}")),
  }
}

/// The body of `POST /reader/deleted`: messages of one room deleted, by their numbers or their stamps.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeletedSeen {
  room_id: i64,
  #[serde(default)]
  message_ids: Vec<i64>,
  #[serde(default)]
  stamps: Vec<bmo_chat::MessageStamp>,
}

async fn deleted(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let seen: DeletedSeen = match body_of(&body) {
    Ok(seen) => seen,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };
  let deletion = bmo_chat::Deletion {
    via: VIA.to_owned(),
    room_id: seen.room_id,
    message_ids: seen.message_ids,
    stamps: seen.stamps,
  };

  match seam.receiver.deleted(deletion).await {
    Ok(()) => Json(Value::Object(serde_json::Map::new())).into_response(),
    Err(why) => failure(ErrorCode::Failed, format!("the deletion was not handed over: {why}")),
  }
}

/// The body of `POST /reader/typing`: a person writing in a room now, or no longer.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypingHeard {
  room_id: i64,
  person_id: i64,
  on: bool,
}

async fn typing(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let heard: TypingHeard = match body_of(&body) {
    Ok(heard) => heard,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };
  let mark = TypingMark {
    via: VIA.to_owned(),
    room_id: heard.room_id,
    person_id: heard.person_id,
    on: heard.on,
    lasts_ms: Some(POLICY.typing.lasts_ms),
  };

  match seam.receiver.typing_marked(mark).await {
    Ok(()) => Json(Value::Object(serde_json::Map::new())).into_response(),
    Err(why) => failure(ErrorCode::Failed, format!("the typing mark was not handed over: {why}")),
  }
}

/// The body of `POST /reader/presence`: people's presence, one as it changes or everyone told of as a whole list.
#[derive(Deserialize)]
struct PresenceHeard {
  people: Vec<PersonPresence>,
  #[serde(default)]
  whole: bool,
}

async fn presence(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let heard: PresenceHeard = match body_of(&body) {
    Ok(heard) => heard,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };
  let change = PresenceChange {
    via: VIA.to_owned(),
    people: heard.people,
    whole: heard.whole,
  };

  match seam.receiver.presence_changed(change).await {
    Ok(()) => Json(Value::Object(serde_json::Map::new())).into_response(),
    Err(why) => failure(ErrorCode::Failed, format!("the presence was not handed over: {why}")),
  }
}
