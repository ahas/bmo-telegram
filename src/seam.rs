//! The service's routes the reader calls (docs/boundary-api.md, 「The service's routes, called by the
//! reader」): what the owner's account read handed on to the receiver, and what the receiver says of its rooms,
//! asked here because the service is the only process that talks to another unit. Every route takes the seam's
//! token. The router holds no fallback of its own, so the service mounts it beside bmo-chat's provider routes.

use std::sync::{Arc, Mutex, PoisonError};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use bmo_chat::{ChatGathered, ChatHandover, ChatReceiver, ErrorCode, WireError};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::receiver::ReceiverNeeds;
use crate::seen::Seen;

/// One hand-over the reader makes: a message, an attachment, an edit, a file gathered, or a line out of a
/// room's history.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Inbound {
  Message(ChatHandover),
  Attachment(ChatHandover),
  Edited(ChatHandover),
  History(ChatHandover),
  Gathered(ChatGathered),
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
  pub needs: ReceiverNeeds,
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
  let status = StatusCode::from_u16(code.status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

  (status, Json(WireError::new(code, error))).into_response()
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

fn handed(result: Result<(), bmo_chat::ChatFailure>) -> Response {
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
    Inbound::Message(handover) => {
      seam.seen.message(&handover.message);
      handed(seam.receiver.message(handover).await)
    }
    Inbound::Attachment(handover) => {
      seam.seen.message(&handover.message);
      handed(seam.receiver.attachment(handover).await)
    }
    Inbound::Edited(handover) => handed(seam.receiver.edited(handover).await),
    Inbound::Gathered(gathered) => handed(seam.receiver.gathered(gathered).await),
    Inbound::History(handover) => match seam.needs.history(&handover).await {
      Ok(kept) => {
        Standing::set(&seam.standing.history, None);
        Json(json!({ "kept": kept })).into_response()
      }
      Err(why) => {
        let reason = format!("the history walk stands down: {why}");

        Standing::set(&seam.standing.history, Some(reason.clone()));
        failure(ErrorCode::NotReady, reason)
      }
    },
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Room {
  chat_id: i64,
}

// Whether the receiver listens to a room; a room it does not say of counts as listened.
async fn listened(State(seam): State<Arc<Seam>>, body: axum::body::Bytes) -> Response {
  let room: Room = match body_of(&body) {
    Ok(room) => room,
    Err(why) => return failure(ErrorCode::BadRequest, why),
  };

  Json(json!({ "listened": seam.needs.listened(room.chat_id).await.unwrap_or(true) })).into_response()
}

async fn rooms(State(seam): State<Arc<Seam>>) -> Response {
  match seam.needs.rooms().await {
    Ok(rooms) => {
      Standing::set(&seam.standing.history, None);
      Json(json!({ "rooms": rooms })).into_response()
    }
    Err(why) => {
      let reason = format!("the history walk stands down: {why}");

      Standing::set(&seam.standing.history, Some(reason.clone()));
      failure(ErrorCode::NotReady, why)
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
  let reason = seam
    .needs
    .avatars_updated(&changed.rooms, &changed.people)
    .await
    .err()
    .map(|why| format!("word of changed profile pictures stands down: {why}"));

  Standing::set(&seam.standing.avatars, reason);

  Json(Value::Object(serde_json::Map::new())).into_response()
}
