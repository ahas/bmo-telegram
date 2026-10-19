//! The service as one router (docs/boundary-api.md): bmo-chat's provider routes, the seam's routes the reader
//! calls and the repository's own, served together and asked over HTTP as the receiver and the reader ask them.

mod support;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::Request;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use bmo_chat::{ChatProvider, ChatReceiver, ChatReceiverLink};
use bmo_telegram::file_ids::FileIds;
use bmo_telegram::provider::TelegramProvider;
use bmo_telegram::reader::ReaderClient;
use bmo_telegram::receiver::ReceiverNeeds;
use bmo_telegram::routes::service;
use bmo_telegram::seam::{Seam, Standing};
use bmo_telegram::seen::Seen;
use bmo_telegram::send::Sending;
use bmo_telegram::telegram::Telegram;
use bmo_telegram::wire::Wire;
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};
use tokio::net::TcpListener;

use telegram_dialect::WRITING;

use support::TOKEN as BOT_TOKEN;
use support::serve;

const TOKEN: &str = "served-token";
const RECEIVER_TOKEN: &str = "receiver-token";
const ROOM: i64 = -100;
const PERSON: i64 = 55;
const OWNER: i64 = 42;
const BOT: i64 = 7_000_000_001_i64;
const SENT_AT: i64 = 1_700_000_000;

/// A port nothing listens on: the reader that does not stand behind the service.
async fn closed_port() -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");

  listener.local_addr().expect("a bound address").port()
}

// The Bot API: who the bot is, what it calls a room, and no updates at all.
async fn bot_api() -> String {
  let router = Router::new().fallback(move |request: Request| {
    async move {
      let path = request.uri().path().to_owned();
      let method = path.rsplit('/').next().unwrap_or_default().to_owned();
      let _asked = axum::body::to_bytes(request.into_body(), usize::MAX).await;

      if method == "getUpdates" {
        return std::future::pending::<Response>().await;
      }

      let answer = match method.as_str() {
        "getMe" => json!({ "ok": true, "result": { "id": BOT, "is_bot": true, "first_name": "Helper", "username": "helper_bot" } }),
        "getChat" => json!({ "ok": true, "result": { "title": "Board room" } }),
        _ => json!({ "ok": true, "result": true }),
      };

      Json(answer).into_response()
    }
  });

  format!("http://127.0.0.1:{}", serve(router).await)
}

// The receiver: every hand-over and every need kept, each answered with an empty body.
#[derive(Default)]
struct Kept {
  handed: Mutex<Vec<(String, Value)>>,
}

async fn receiver_served(kept: Arc<Kept>) -> String {
  let router = Router::new().fallback(move |request: Request| {
    let kept = kept.clone();

    async move {
      let path = request.uri().path().to_owned();
      let bytes = axum::body::to_bytes(request.into_body(), usize::MAX).await.unwrap_or_default();

      kept
        .handed
        .lock()
        .unwrap()
        .push((path, serde_json::from_slice(&bytes).unwrap_or(Value::Null)));

      Json(json!({})).into_response()
    }
  });

  format!("http://127.0.0.1:{}", serve(router).await)
}

// A reader with its account connected, that answers each seam route as the reader does and keeps the sends.
fn fake_reader(asked: Arc<Mutex<Vec<Value>>>) -> Router {
  let keep = move |body: Value| asked.lock().unwrap().push(body);
  let say = keep.clone();

  Router::new()
    .route("/health", get(|| async { Json(json!({ "connected": true, "reason": null })) }))
    .route(
      "/reader/picture",
      post(|| async {
        Json(json!({ "outcome": "fetched", "id": "photo-9", "name": "picture.jpg", "mime": "image/jpeg", "bytes": "AQID" }))
      }),
    )
    .route(
      "/owner/say",
      post(move |Json(body): Json<Value>| async move {
        say(body);

        Json(json!({ "messageId": 900 }))
      }),
    )
    .route("/owner/album", post(|| async { Json(json!({ "ids": [901] })) }))
}

/// The service stood up whole: its routes served on the loopback, with the reader standing or not, and the
/// folders it reads kept for as long as the test runs.
struct Served {
  base: String,
  provider: Arc<TelegramProvider>,
  file_ids: Arc<FileIds>,
  kept: Arc<Kept>,
  asked: Arc<Mutex<Vec<Value>>>,
  _pictures: TempDir,
}

/// The whole service served: the provider reading the stand-in's rooms, handing over to the stand-in receiver,
/// with the reader standing behind it when `reader_standing` says so.
async fn stood(reader_standing: bool) -> Served {
  let bot = bot_api().await;
  let handed = Arc::new(Kept::default());
  let receiver_base = receiver_served(handed.clone()).await;
  let asked = Arc::new(Mutex::new(vec![]));
  let reader_port = if reader_standing {
    serve(fake_reader(asked.clone())).await
  } else {
    closed_port().await
  };

  let seen = Arc::new(Seen::in_memory());
  let standing = Arc::new(Standing::default());
  let reader = ReaderClient::new(Wire::loopback(reader_port, TOKEN));
  let file_ids = Arc::new(FileIds::in_memory());
  let telegram = Arc::new(Telegram::new(
    BOT_TOKEN,
    &bot,
    OWNER,
    Some(reader.clone()),
    seen.clone(),
    file_ids.clone(),
  ));
  let link = ChatReceiverLink::new(receiver_base.as_str(), RECEIVER_TOKEN);
  let receiver: Arc<dyn ChatReceiver> = Arc::new(link);
  let seam = Arc::new(Seam {
    token: TOKEN.to_owned(),
    receiver: receiver.clone(),
    needs: ReceiverNeeds::new(Wire::new(receiver_base.as_str(), RECEIVER_TOKEN)),
    seen: seen.clone(),
    standing: standing.clone(),
  });
  let provider =
    Arc::new(TelegramProvider::new(telegram, Sending::default(), Some(reader), seen, standing));

  provider.start(receiver).await.expect("the provider starts");

  // The bot account is ready once Telegram has said who it is, which the stand-in answers at once.
  for _ in 0..500 {
    if provider.health().ready {
      break;
    }

    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
  }

  assert!(provider.health().ready, "the provider is ready");

  let pictures = tempdir().expect("a temporary folder");
  let avatars = pictures.path().join("avatars");

  let person = avatars.join("person");

  std::fs::create_dir_all(&person).expect("the pictures folder is made");
  std::fs::write(person.join(format!("{PERSON}.jpg")), [0xff, 0xd8]).expect("a picture is kept");

  let app = service(provider.clone(), seam, avatars);

  Served {
    base: format!("http://127.0.0.1:{}", serve(app).await),
    provider,
    file_ids,
    kept: handed,
    asked,
    _pictures: pictures,
  }
}

/// One route of the served service asked as the receiver asks it: the status and the JSON answer.
async fn asked(base: &str, route: &str, body: Value) -> (u16, Value) {
  let answer = Wire::new(base, TOKEN).post(route, &body).await.expect("the service answers");

  (answer.status, answer.body)
}

async fn asked_get(base: &str, route: &str) -> (u16, Value) {
  let answer = Wire::new(base, TOKEN).get(route).await.expect("the service answers");

  (answer.status, answer.body)
}

/// The declaration as the served service answers it: the provider's key and label, its modes with their
/// standing, its capabilities, its dialect and its numbering.
#[tokio::test]
async fn the_declaration_names_the_provider_its_modes_and_its_dialect() {
  let served = stood(true).await;
  served.provider.refresh_reader().await;

  let (status, answer) = asked_get(&served.base, "/declaration").await;

  assert_eq!(status, 200);
  assert_eq!(answer["key"], "telegram");
  assert_eq!(answer["label"], "Telegram");
  assert_eq!(answer["configured"], Value::Null);
  assert_eq!(answer["shows"], Value::Null);
  assert_eq!(answer["speakers"], false);
  assert_eq!(
    answer["numbering"],
    json!({ "rooms": "native", "people": "native", "messages": "native" })
  );
  assert_eq!(
    answer["dialect"],
    json!({
      "name": "telegram-html",
      "limits": { "text": 4096, "caption": 1024, "albumPhotos": 10, "fileBytes": 20000000 },
      "writing": WRITING,
      "foldable": true,
    })
  );

  assert_eq!(answer["modes"][0]["voice"], "bot-account");
  assert_eq!(answer["modes"][0]["reach"], "all-messages");
  assert_eq!(answer["modes"][0]["standing"], "active");
  assert_eq!(answer["modes"][0]["reason"], Value::Null);
  assert_eq!(answer["modes"][0]["canReplyTo"], true);
  assert_eq!(answer["modes"][1]["voice"], "human-reader");
  assert_eq!(answer["modes"][1]["reach"], "all-messages");
  assert_eq!(answer["modes"][1]["standing"], "active");
  assert_eq!(answer["modes"][1]["canReplyTo"], false);

  assert_eq!(
    answer["can"],
    json!([
      "text",
      "photo",
      "album",
      "document",
      "audio",
      "voice",
      "sticker",
      "reaction",
      "pin",
      "typing",
      "edit",
      "delete",
      "replyTo",
      "fetchMedia",
      "avatars",
      "history",
    ])
  );
}

/// The service answers ready with the reader behind it and without one: the owner's account is what stands
/// in the declaration's modes and capabilities, never in the health.
#[tokio::test]
async fn the_service_is_ready_with_and_without_the_reader_behind_it() {
  let with = stood(true).await;
  let without = stood(false).await;

  for served in [&with, &without] {
    let (status, answer) = asked_get(&served.base, "/health").await;

    assert_eq!(status, 200);
    assert_eq!(answer["ready"], json!(true));
    assert_eq!(answer["reason"], Value::Null);
  }

  with.provider.refresh_reader().await;
  without.provider.refresh_reader().await;

  let (_status, with_declaration) = asked_get(&with.base, "/declaration").await;
  let (_status, without_declaration) = asked_get(&without.base, "/declaration").await;

  assert_eq!(with_declaration["modes"][1]["standing"], "active");
  assert_eq!(without_declaration["modes"][1]["standing"], "inactive");
  assert!(without_declaration["modes"][1]["reason"].as_str().is_some());

  assert_eq!(
    without_declaration["can"],
    json!([
      "text", "photo", "album", "document", "audio", "voice", "sticker", "reaction", "pin", "typing",
      "edit", "delete", "replyTo",
    ])
  );
}

/// Every room question the protocol asks, answered through the served routes: the owner's one-to-one is the
/// provider's own, a one-to-one is numbered by its person, and the title comes from the Bot API.
#[tokio::test]
async fn every_room_route_answers_for_a_room_the_provider_reads() {
  let served = stood(true).await;

  let (_status, owns) = asked(&served.base, "/room/owns", json!({ "chatId": OWNER })).await;
  assert_eq!(owns["owns"], true);

  let (_status, owns_other) = asked(&served.base, "/room/owns", json!({ "chatId": ROOM })).await;
  assert_eq!(owns_other["owns"], false);

  let (_status, identity) = asked(&served.base, "/room/identity", json!({ "chatId": ROOM })).await;
  assert_eq!(identity["identity"], BOT);

  let (_status, kind) = asked(&served.base, "/room/is-owner", json!({ "chatId": OWNER })).await;
  assert_eq!(kind["kind"], "owner");

  let (_status, group) = asked(&served.base, "/room/is-owner", json!({ "chatId": ROOM })).await;
  assert_eq!(group["kind"], "group");

  let (_status, person) = asked(&served.base, "/room/private-of", json!({ "chatId": PERSON })).await;
  assert_eq!(person["person"], PERSON);

  let (_status, title) = asked(&served.base, "/room/title", json!({ "chatId": ROOM })).await;
  assert_eq!(title["title"], "Board room");

  let (status, owner_room) = asked_get(&served.base, "/owner-room").await;

  assert_eq!(status, 200);
  assert_eq!(owner_room["chatId"], json!(OWNER));
}

/// A message's picture fetched again through the reader seam: the bytes the owner's account hands over, under
/// the file's own name and the owner's account's id, or the id the bot account handed it over with. Where no
/// reader stands, the capability is not declared and the fetch is refused.
#[tokio::test]
async fn a_picture_is_fetched_again_through_the_reader_seam_under_the_id_it_crossed_with() {
  let served = stood(true).await;
  let request = json!({ "chatId": ROOM, "messageId": 6, "sentAt": SENT_AT });

  let (status, answer) = asked(&served.base, "/media", request.clone()).await;

  assert_eq!(status, 200);
  assert_eq!(
    answer,
    json!({ "outcome": "fetched", "id": "photo-9", "name": "picture.jpg", "mime": "image/jpeg", "bytes": "AQID" })
  );

  served.file_ids.keep(ROOM, 6, "photo-unique");

  let (_status, again) = asked(&served.base, "/media", request.clone()).await;

  assert_eq!(again["id"], "photo-unique");

  let down = stood(false).await;
  let (_status, refused) = asked(&down.base, "/media", request.clone()).await;

  assert_eq!(refused["outcome"], "refused");
  assert_eq!(refused["refusal"]["code"], "unsupported-capability");
  assert_eq!(refused["refusal"]["capability"], "fetchMedia");
}

/// A send in the owner's own voice goes through the seam to the reader: the words cross to the owner's account,
/// which answers the message it went out as.
#[tokio::test]
async fn a_send_as_the_owner_goes_through_the_reader() {
  let served = stood(true).await;
  let words = "words in the owner's own voice";

  let (status, answer) = asked(
    &served.base,
    "/send",
    json!({ "chatId": ROOM, "as": "human-reader", "verb": "text", "text": words }),
  )
  .await;

  assert_eq!(status, 200);
  assert_eq!(answer, json!({ "sent": true, "messageId": 900 }));

  let asked = served.asked.lock().unwrap().clone();

  assert_eq!(asked.last().unwrap().clone(), json!({ "chatId": ROOM, "text": words, "reply": null }));
}

/// The repository's own routes beside the protocol's: what stands of each part, the profile pictures the
/// reader keeps, and the token every route takes.
#[tokio::test]
async fn the_service_s_own_routes_report_the_parts_and_serve_the_pictures() {
  let served = stood(true).await;

  let (status, answer) = asked_get(&served.base, "/status").await;

  assert_eq!(status, 200);

  for part in ["bot", "reader", "history", "avatars"] {
    assert_eq!(answer[part]["standing"], "active");
    assert_eq!(answer[part]["reason"], Value::Null);
  }

  let base = served.base.as_str();
  let picture = reqwest::Client::new()
    .get(format!("{base}/avatars/person/{PERSON}"))
    .bearer_auth(TOKEN)
    .send()
    .await
    .expect("the service answers");
  let kind = picture.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok());

  if picture.status().as_u16() != 200 {
    let why = picture.text().await.expect("an answer").to_owned();

    panic!("the picture route answered {why}");
  }

  assert_eq!(picture.status().as_u16(), 200);
  assert_eq!(kind, Some("image/jpeg"));
  assert_eq!(picture.bytes().await.expect("the picture's bytes"), vec![0xff, 0xd8]);

  let (status, missing) = asked_get(&served.base, "/avatars/room/9").await;

  assert_eq!(status, 404);
  assert_eq!(missing["code"], "nothing-held");

  let (status, astray) = asked_get(&served.base, "/avatars/sticker/9").await;

  assert_eq!(status, 404);
  assert_eq!(astray["code"], "no-route");

  let stranger = Wire::new(served.base.as_str(), "other");

  assert_eq!(stranger.get("/status").await.expect("the service answers").status, 401);
}

/// The seam's routes stand beside the provider's on the same service: what the reader hands over goes on to
/// the receiver, and what the provider has seen is read back.
#[tokio::test]
async fn the_seam_stands_beside_the_provider_routes() {
  let served = stood(true).await;
  let handover = json!({
    "kind": "message",
    "message": {
      "chatId": ROOM, "messageId": 7, "senderId": PERSON, "senderName": "Other bot", "sentAt": SENT_AT,
      "text": "words", "replyToMessageId": null, "via": "telegram", "album": null, "mode": "human-reader"
    },
    "context": { "roomTitle": "Room", "carriesFile": false, "gathering": false },
    "room": { "kind": "group", "identity": BOT, "privateTo": null }
  });

  let (status, _answer) = asked(&served.base, "/reader/inbound", handover.clone()).await;

  assert_eq!(status, 200);

  let handed = served.kept.handed.lock().unwrap().clone();

  assert_eq!(handed.iter().map(|(route, _)| route.as_str()).collect::<Vec<_>>(), ["/message"]);

  let (_status, seen) = asked_get(&served.base, "/reader/seen").await;

  assert_eq!(seen, json!({ "rooms": [ROOM], "people": [PERSON] }));
}
