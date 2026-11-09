//! The service as one router (docs/boundary-api.md, 「Routes served」): bmo-chat's provider routes, the seam's
//! routes the reader calls, and this repository's own `/status`, all under the service's token. The binary and
//! the service's tests stand on this one assembly.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Query, Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::{Next, from_fn_with_state};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use bmo_chat::{ChatHealth, ChatProvider, ErrorCode, WireError, provider_router};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::task::JoinHandle;

use crate::provider::{ReadAnswer, TelegramProvider};
use crate::read::{DEFAULT_MAX, LARGEST_MAX};
use crate::seam::{Seam, seam_router};

/// Why a route that takes the token was asked without it.
const WRONG_TOKEN: &str = "the token is missing or wrong";

/// What the service's own routes stand on: the provider and the seam.
pub struct Served {
  provider: Arc<TelegramProvider>,
  seam: Arc<Seam>,
}

/// The service's routes over `provider` and the seam.
pub fn service(provider: Arc<TelegramProvider>, seam: Arc<Seam>) -> Router {
  let own = Arc::new(Served {
    provider: provider.clone(),
    seam: seam.clone(),
  });
  let protocol: Arc<dyn ChatProvider> = provider;

  provider_router(protocol, seam.token.as_str())
    .merge(seam_router(seam))
    .merge(own_routes(own))
}

/// The repository's own routes, under the service's token like every other route of it.
fn own_routes(served: Arc<Served>) -> Router {
  Router::new()
    .route("/status", get(status))
    .route("/messages", get(messages))
    .layer(from_fn_with_state(served.clone(), authorized))
    .with_state(served)
}

/// The reader asked for its health on a beat, so the declaration and `/status` answer with what the owner's
/// account stands on now.
pub fn keep_reader_current(provider: Arc<TelegramProvider>, every: std::time::Duration) -> JoinHandle<()> {
  tokio::spawn(async move {
    loop {
      provider.refresh_reader().await;
      tokio::time::sleep(every).await;
    }
  })
}

fn failure(code: ErrorCode, error: impl Into<String>) -> Response {
  WireError::new(code, error).into_response()
}

async fn authorized(State(served): State<Arc<Served>>, request: Request, next: Next) -> Response {
  let expected = format!("Bearer {}", served.seam.token);
  let given = request.headers().get(AUTHORIZATION).and_then(|value| value.to_str().ok());

  if given == Some(expected.as_str()) {
    next.run(request).await
  } else {
    failure(ErrorCode::Unauthorized, WRONG_TOKEN.to_owned())
  }
}

// An answer of one part of the service: where it stands, and why it stands down where it does.
fn standing(active: bool, reason: Option<String>) -> Value {
  json!({
    "standing": if active { "active" } else { "inactive" },
    "reason": reason,
  })
}

/// What one chat read is asked for: the chat by its number or by a name, how many messages at most, and the
/// earliest moment it asks for.
#[derive(Deserialize)]
struct MessagesQuery {
  chat: String,
  max: Option<usize>,
  since: Option<i64>,
}

/// One chat's recent messages on the owner's word, read through the reader without a room standing for it and
/// nothing sent into the chat; or, where the chat is named by a person or a handle, the chats and contacts that
/// name could mean, so the owner picks.
async fn messages(State(served): State<Arc<Served>>, Query(asked): Query<MessagesQuery>) -> Response {
  let chat = asked.chat.trim();

  if chat.is_empty() {
    return failure(ErrorCode::BadRequest, "chat is required".to_owned());
  }

  let max = asked.max.unwrap_or(DEFAULT_MAX).clamp(1, LARGEST_MAX);

  match served.provider.read_messages(chat, max, asked.since).await {
    ReadAnswer::Messages(messages) => Json(json!({ "messages": messages })).into_response(),
    ReadAnswer::Candidates(candidates) => Json(json!({ "candidates": candidates })).into_response(),
    ReadAnswer::Refused { status, reason } => {
      let code = match status {
        404 => ErrorCode::NotFound,
        503 => ErrorCode::NotReady,
        _ => ErrorCode::Failed,
      };

      failure(code, reason)
    }
  }
}

/// What stands of each part: the bot account, the reader, the history walk and the word of changed profile
/// pictures. The reader is asked again before the parts are written down.
async fn status(State(served): State<Arc<Served>>) -> Response {
  let reader = served.provider.refresh_reader().await;
  let ChatHealth { ready, reason, .. } = served.provider.health();
  let history = served.seam.standing.history();
  let avatars = served.seam.standing.avatars();

  Json(json!({
    "bot": standing(ready, reason),
    "reader": standing(reader.connected, reader.reason),
    "history": standing(history.is_none(), history),
    "avatars": standing(avatars.is_none(), avatars),
  }))
  .into_response()
}
