//! The messages tool: the owner's word alone, the provider's token and port read from the data root's env
//! file, a read answered newest first with a picture's placeholder, an unresolved chat refused, and a name
//! answered as the candidates to pick among.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use axum::Router;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::http::header::AUTHORIZATION;
use axum::response::IntoResponse;
use axum::routing::get;
use messages_tool::{ENTRY, NOT_OWNER, PROTOCOL, Service, Session, answer};
use serde_json::{Value, json};
use tempfile::tempdir;

const TOKEN: &str = "provider-token";

async fn serve(router: Router) -> u16 {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
  let port = listener.local_addr().expect("a bound address").port();

  tokio::spawn(async move { axum::serve(listener, router).await.expect("the stand-in serves") });

  port
}

// A provider that keeps what it was asked, answers a resolved chat's lines newest first, a name's candidates,
// and refuses one chat by the reason the reader gives.
fn provider(asked: Arc<std::sync::Mutex<Vec<(String, String)>>>) -> Router {
  Router::new().route(
    "/messages",
    get(
      move |headers: axum::http::HeaderMap, Query(query): Query<HashMap<String, String>>| {
        let asked = asked.clone();

        async move {
          let bearer = headers.get(AUTHORIZATION).and_then(|value| value.to_str().ok()).unwrap_or_default();

          asked.lock().unwrap().push((bearer.to_owned(), query.get("chat").cloned().unwrap_or_default()));

          match query.get("chat").map(String::as_str) {
            Some("-9") => (
              StatusCode::NOT_FOUND,
              axum::Json(json!({ "code": "not-found", "error": "the owner's account cannot resolve chat -9" })),
            )
              .into_response(),
            Some("Named") => axum::Json(json!({ "candidates": [
              { "chatId": 55, "name": "Someone", "kind": "room" }
            ] }))
            .into_response(),
            _ => axum::Json(json!({ "messages": [
              { "messageId": 2, "senderId": 55, "senderName": "Someone", "sentAt": 2_000, "text": "newer" },
              { "messageId": 1, "senderId": null, "senderName": "unknown", "sentAt": 1_000, "text": "[Photo]" }
            ] }))
            .into_response(),
          }
        }
      },
    ),
  )
}

fn data_root(port: u16) -> tempfile::TempDir {
  let root = tempdir().expect("a temporary folder");

  std::fs::create_dir_all(root.path().join("env")).expect("the env folder is made");
  std::fs::write(
    root.path().join("env/telegram.env"),
    format!("BMO_TELEGRAM_TOKEN={TOKEN}\nBMO_TELEGRAM_PORT={port}\n"),
  )
  .expect("the env file is written");

  root
}

fn handed(root: &Path, chat: &str, privileged: &str) -> Value {
  json!({
    "protocol": PROTOCOL,
    "kind": "call",
    "entry": ENTRY,
    "arguments": { "chat": chat, "max": 5 },
    "session": {
      "variables": { "BMO_DATA_DIR": root.to_string_lossy(), "BMO_TURN_PRIVILEGED": privileged }
    }
  })
}

fn said(answer: &Value) -> &str {
  answer["content"][0]["text"].as_str().unwrap_or_default()
}

#[test]
fn a_turn_that_is_not_the_owner_is_refused_and_no_grant_names_reading() {
  let member = json!({ "variables": { "BMO_TURN_PRIVILEGED": "0" } });
  let session = Session::of(&member);

  assert_eq!(session.admitted(), Err(NOT_OWNER.to_owned()));

  let unmarked = json!({ "variables": {} });
  let none = Session::of(&unmarked);

  assert_eq!(none.admitted(), Err(NOT_OWNER.to_owned()));
  assert!(NOT_OWNER.contains("no grant in force names it"));
}

#[test]
fn a_resident_programs_run_says_whether_the_turn_is_the_owners() {
  let place = tempdir().expect("a place");

  std::fs::write(
    place.path().join("run.json"),
    json!({ "person": false, "env": { "BMO_TURN_PRIVILEGED": "1" } }).to_string(),
  )
  .expect("the run is written");

  let record = json!({
    "variables": { "BMO_TURN_PRIVILEGED": "0" },
    "labels": { "place": place.path().to_string_lossy() }
  });
  let from_place = Session::of(&record);

  assert_eq!(from_place.admitted(), Ok(()));

  std::fs::write(
    place.path().join("run.json"),
    json!({ "person": true, "env": { "BMO_TURN_PRIVILEGED": "1" } }).to_string(),
  )
  .expect("the run is written");

  assert!(from_place.admitted().is_err(), "a person's turn reads no run variables");
}

#[test]
fn the_provider_is_read_from_the_env_file_under_the_data_root() {
  let root = data_root(3_220);
  let service = Service::of(root.path()).expect("the env file names the provider");

  assert_eq!((service.port, service.token.as_str()), (3_220, TOKEN));
}

#[tokio::test]
async fn one_chats_recent_messages_are_read_newest_first_with_a_pictures_placeholder() {
  let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
  let port = serve(provider(asked.clone())).await;
  let root = data_root(port);
  let answer = answer(&handed(root.path(), "55", "1")).await.expect("the call answers");

  assert_eq!(answer["isError"], false);

  let text = said(&answer);

  assert!(text.starts_with("2 messages, newest first:"), "{text}");
  assert!(text.contains("2000 Someone (55): newer"), "{text}");
  assert!(text.contains("1000 unknown: [Photo]"), "{text}");
  assert_eq!(
    asked.lock().unwrap().as_slice(),
    [("Bearer provider-token".to_owned(), "55".to_owned())]
  );
}

#[tokio::test]
async fn a_chat_the_account_cannot_resolve_is_refused_with_the_reason() {
  let port = serve(provider(Arc::new(std::sync::Mutex::new(Vec::new())))).await;
  let root = data_root(port);
  let answer = answer(&handed(root.path(), "-9", "1")).await.expect("the call answers");

  assert_eq!(answer["isError"], true);
  assert!(said(&answer).contains("cannot resolve chat -9"), "{}", said(&answer));
}

#[tokio::test]
async fn a_name_answers_the_candidates_to_pick_among() {
  let port = serve(provider(Arc::new(std::sync::Mutex::new(Vec::new())))).await;
  let root = data_root(port);
  let answer = answer(&handed(root.path(), "Named", "1")).await.expect("the call answers");

  assert_eq!(answer["isError"], false);
  assert!(said(&answer).contains("55 Someone (room)"), "{}", said(&answer));
}

#[tokio::test]
async fn a_turn_that_is_not_the_owners_asks_no_provider() {
  let asked = Arc::new(std::sync::Mutex::new(Vec::new()));
  let port = serve(provider(asked.clone())).await;
  let root = data_root(port);
  let answer = answer(&handed(root.path(), "55", "0")).await.expect("the call answers");

  assert_eq!(answer["isError"], true);
  assert_eq!(said(&answer), NOT_OWNER);
  assert!(asked.lock().unwrap().is_empty(), "the provider was asked nothing");
}

// The answer of a route that is not the shape the tool reads is a refusal, never a panic.
#[tokio::test]
async fn an_answer_the_tool_does_not_read_is_refused() {
  let router =
    Router::new().route("/messages", get(|| async { (StatusCode::OK, "not json").into_response() }));
  let port = serve(router).await;
  let root = data_root(port);
  let answer = answer(&handed(root.path(), "55", "1")).await.expect("the call answers");

  assert_eq!(answer["isError"], true);
}
