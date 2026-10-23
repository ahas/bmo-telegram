//! What the send tests share: a stand-in of the Bot API that keeps every call it was asked — a form's files by
//! their name, type and bytes — and answers each as the scenario says, a stand-in of the web pictures are named
//! on, and the bot account speaking to them.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Json, Response};
use bmo_chat::{ChatProvider, ChatSend, ChatSendResult, provider_router};
use bmo_telegram::bot_api::BotApi;
use bmo_telegram::file_ids::FileIds;
use bmo_telegram::provider::TelegramProvider;
use bmo_telegram::seen::Seen;
use bmo_telegram::send::{Sending, Speaker};
use bmo_telegram::telegram::Telegram;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{KeptReceiver, OWNER, TOKEN, serve};

pub const ROOM: i64 = -100;
pub const BOT: i64 = 777;
pub const SENT_AT: i64 = 1_700_000_000;

/// A file a form carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
  pub name: String,
  pub mime: String,
  pub bytes: Vec<u8>,
}

/// One call the stand-in was asked: its method, its parameters — a form's text fields as text, its `media` and
/// `reply_parameters` read as JSON, and each file as its name, type and digest — and the files themselves.
#[derive(Debug, Clone)]
pub struct Asked {
  pub method: String,
  pub body: Value,
  pub uploaded: bool,
  pub files: BTreeMap<String, Upload>,
}

/// How the stand-in answers a call: with what the scenario says, or, where it says nothing, as Telegram answers
/// a call that went.
pub type Answer = Box<dyn Fn(&Asked) -> Option<Value> + Send + Sync>;

/// The Bot API's stand-in for sends.
pub struct BotSends {
  asked: Mutex<Vec<Asked>>,
  answer: Mutex<Answer>,
  next: Mutex<i64>,
}

/// A message Telegram made of a send, numbered in the order they came.
pub fn message(id: i64, chat_id: &Value, extra: Value) -> Value {
  let chat_id = chat_id.as_i64().or_else(|| chat_id.as_str().and_then(|id| id.parse().ok()));
  let mut message = json!({ "message_id": id, "date": SENT_AT, "chat": { "id": chat_id, "type": "supergroup" },
    "from": { "id": BOT, "is_bot": true, "first_name": "Helper" } });

  message.as_object_mut().unwrap().extend(extra.as_object().cloned().unwrap_or_default());

  message
}

/// A refusal as Telegram words one.
pub fn refusal(code: u16, description: &str) -> Value {
  json!({ "ok": false, "error_code": code, "description": description })
}

pub fn sha(bytes: &[u8]) -> String {
  Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

impl BotSends {
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      asked: Mutex::default(),
      answer: Mutex::new(Box::new(|_| None)),
      next: Mutex::new(500),
    })
  }

  /// The answers a scenario gives, the message numbers counted from 500 again and nothing asked yet.
  pub fn answering(&self, answer: impl Fn(&Asked) -> Option<Value> + Send + Sync + 'static) {
    *self.answer.lock().unwrap() = Box::new(answer);
    *self.next.lock().unwrap() = 500;
    self.asked.lock().unwrap().clear();
  }

  pub fn asked(&self) -> Vec<Asked> {
    self.asked.lock().unwrap().clone()
  }

  pub fn methods(&self) -> Vec<String> {
    self.asked().into_iter().map(|asked| asked.method).collect()
  }

  pub fn bodies(&self) -> Vec<Value> {
    self.asked().into_iter().map(|asked| asked.body).collect()
  }

  fn next_id(&self) -> i64 {
    let mut next = self.next.lock().unwrap();
    let id = *next;

    *next += 1;
    id
  }

  // What Telegram answers a call that went: the message it went out as, an album's messages, or true.
  fn went(&self, asked: &Asked) -> Value {
    let chat_id = &asked.body["chat_id"];
    let photo = |id: i64| {
      json!({ "photo": [
        { "file_id": format!("small-{id}"), "file_unique_id": format!("us-{id}"), "file_size": 10, "width": 90, "height": 90 },
        { "file_id": format!("large-{id}"), "file_unique_id": format!("ul-{id}"), "file_size": 100, "width": 4, "height": 4 },
      ] })
    };
    let result = match asked.method.as_str() {
      "sendMessage" => {
        let id = self.next_id();

        message(id, chat_id, json!({ "text": asked.body["text"] }))
      }
      "sendPhoto" => {
        let id = self.next_id();

        message(id, chat_id, photo(id))
      }
      "sendMediaGroup" => {
        let media = asked.body["media"].as_array().cloned().unwrap_or_default();

        Value::Array(
          media
            .iter()
            .map(|_| {
              let id = self.next_id();
              let mut one = message(id, chat_id, photo(id));

              one["media_group_id"] = json!("album-1");
              one
            })
            .collect(),
        )
      }
      "sendDocument" | "sendAudio" | "sendVoice" | "sendSticker" => {
        message(self.next_id(), chat_id, json!({}))
      }
      "getMe" => json!({ "id": BOT, "is_bot": true, "first_name": "Helper", "username": "helper_bot" }),
      _ => json!(true),
    };

    json!({ "ok": true, "result": result })
  }

  fn answer(&self, asked: Asked) -> Response {
    let answer = (self.answer.lock().unwrap())(&asked);

    self.asked.lock().unwrap().push(asked.clone());

    Json(answer.unwrap_or_else(|| self.went(&asked))).into_response()
  }
}

// A form read into the call's parameters and files.
async fn form(request: Request) -> (Value, BTreeMap<String, Upload>) {
  let mut multipart = Multipart::from_request(request, &()).await.expect("a multipart form");
  let mut body = serde_json::Map::new();
  let mut files = BTreeMap::new();

  while let Some(field) = multipart.next_field().await.expect("a form field") {
    let name = field.name().unwrap_or_default().to_owned();
    let file_name = field.file_name().map(str::to_owned);
    let mime = field.content_type().unwrap_or_default().to_owned();
    let bytes = field.bytes().await.expect("a field's bytes").to_vec();

    match file_name {
      Some(file_name) => {
        body.insert(name.clone(), json!({ "file": file_name, "type": mime, "sha": sha(&bytes) }));
        files.insert(
          name,
          Upload {
            name: file_name,
            mime,
            bytes,
          },
        );
      }
      None => {
        let text = String::from_utf8(bytes).expect("a text field");
        let value = if name == "media" || name == "reply_parameters" {
          serde_json::from_str(&text).expect("a JSON field")
        } else {
          Value::String(text)
        };

        body.insert(name, value);
      }
    }
  }

  (Value::Object(body), files)
}

/// The stand-in served, its address the one the bot account is pointed at.
pub async fn bot_sends(stand_in: Arc<BotSends>) -> String {
  let router = Router::new().fallback(move |request: Request| {
    let stand_in = stand_in.clone();

    async move {
      let method = request.uri().path().rsplit('/').next().unwrap_or_default().to_owned();

      // No update ever comes: a long poll that never ends while the test runs.
      if method == "getUpdates" {
        return std::future::pending::<Response>().await;
      }

      // Whether the bot is in a room is no send: the bot is a member of every group the sends go to.
      if method == "getChatMember" {
        return Json(json!({ "ok": true, "result": { "status": "member" } })).into_response();
      }

      let uploaded = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|kind| kind.to_str().ok())
        .is_some_and(|kind| kind.starts_with("multipart/form-data"));
      let (body, files) = if uploaded {
        form(request).await
      } else {
        let bytes = axum::body::to_bytes(request.into_body(), usize::MAX).await.unwrap_or_default();

        (serde_json::from_slice(&bytes).unwrap_or(Value::Null), BTreeMap::new())
      };

      stand_in.answer(Asked {
        method,
        body,
        uploaded,
        files,
      })
    }
  });

  format!("http://127.0.0.1:{}", serve(router).await)
}

/// The web pictures are named on: each path served as its bytes under its type, anything else not found.
pub async fn web(pages: BTreeMap<String, (Vec<u8>, String)>) -> String {
  let pages = Arc::new(pages);
  let router = Router::new().fallback(move |request: Request| {
    let pages = pages.clone();

    async move {
      let name = request.uri().path().rsplit('/').next().unwrap_or_default().to_owned();

      match pages.get(&name) {
        Some((bytes, kind)) => ([(CONTENT_TYPE, kind.clone())], bytes.clone()).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "gone").into_response(),
      }
    }
  });

  format!("http://127.0.0.1:{}", serve(router).await)
}

/// The bot account speaking to the stand-in, as `sending` configures it.
pub async fn speaker(stand_in: Arc<BotSends>, sending: Sending) -> Speaker {
  let api = BotApi::new(bot_sends(stand_in).await, TOKEN);

  Speaker::new(api, sending, Arc::new(Seen::in_memory()))
}

/// The token the provider's routes take in a test.
pub const ROUTES_TOKEN: &str = "routes-token";

/// The provider over the stand-in, started and ready, its routes served: where they are, and the provider.
pub async fn provider_served(stand_in: Arc<BotSends>, sending: Sending) -> (String, Arc<TelegramProvider>) {
  let seen = Arc::new(Seen::in_memory());
  let telegram = Arc::new(Telegram::new(
    TOKEN,
    &bot_sends(stand_in).await,
    OWNER,
    None,
    seen.clone(),
    Arc::new(FileIds::in_memory()),
  ));
  let provider = Arc::new(TelegramProvider::new(
    telegram,
    sending,
    None,
    seen,
    Arc::default(),
    std::path::PathBuf::from("avatars"),
  ));
  let (receiver, _heard) = KeptReceiver::new();

  provider.start(receiver).await.expect("the provider starts");

  // The bot account is ready once Telegram has said who it is, which the stand-in answers at once.
  for _ in 0..500 {
    if provider.health().ready {
      break;
    }

    tokio::task::yield_now().await;
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
  }

  assert!(provider.health().ready, "the provider is ready");

  let routes = provider_router(provider.clone(), ROUTES_TOKEN);

  (format!("http://127.0.0.1:{}", serve(routes).await), provider)
}

/// One send posted to the provider's `/sends` route as the receiving side posts it: the status and the
/// answer.
pub async fn post_send(base: &str, body: Value) -> (u16, Value) {
  let answer = reqwest::Client::new()
    .post(format!("{base}/sends"))
    .bearer_auth(ROUTES_TOKEN)
    .header(CONTENT_TYPE, "application/json")
    .body(body.to_string())
    .send()
    .await
    .expect("the provider answers");
  let status = answer.status().as_u16();
  let bytes = answer.bytes().await.expect("an answer's body");

  (status, serde_json::from_slice(&bytes).expect("a JSON answer"))
}

/// One send in the bot account's voice into the test's room, answering `reply_to` where one is named.
pub async fn send(
  speaker: &Speaker,
  reply_to: Option<i64>,
  send: ChatSend,
) -> Result<ChatSendResult, String> {
  speaker.send(ROOM, reply_to, &send).await.map_err(|why| why.to_string())
}
