//! The service's routes the reader calls (docs/boundary-api.md, 「The service's routes, called by the
//! reader」): what the owner's account read handed on to the receiver, and what the receiver says of its rooms,
//! asked here because the service is the only process that talks to another unit. Every route takes the seam's
//! token. The router holds no fallback of its own, so the service mounts it beside bmo-chat's provider routes.

use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use bmo_chat::{
  AvatarChange, ChatFailure, ChatReceiver, ErrorCode, GatheredPatch, HandedMessage, MessageOrigin,
  RoomQuery, RoomsQuery, WireError,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::VIA;
use crate::seen::Seen;

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

/// What the seam's routes stand on.
pub struct Seam {
  pub token: String,
  pub receiver: Arc<dyn ChatReceiver>,
  pub seen: Arc<Seen>,
  pub standing: Arc<Standing>,
}

/// The seam's routes over `seam`.
pub fn seam_router(seam: Arc<Seam>) -> Router {
  Router::new()
    .route("/reader/inbound", post(inbound))
    .route("/reader/listened", post(listened))
    .route("/reader/rooms", get(rooms))
    .route("/reader/seen", get(seen))
    .route("/reader/avatars", post(avatars))
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
      message,
    } => {
      let history = message.origin == MessageOrigin::History;

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
  let listened = match seam.receiver.room(room.chat_id, query).await {
    Ok(held) => held.is_some_and(|held| held.listened),
    Err(_) => true,
  };

  Json(json!({ "listened": listened })).into_response()
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

async fn seen(State(seam): State<Arc<Seam>>) -> Response {
  Json(seam.seen.sighted()).into_response()
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
