//! The service as one router (docs/boundary-api.md): bmo-chat's provider routes, the seam's routes the reader
//! calls and the repository's own, served together and asked over HTTP as the receiving side and the reader ask
//! them.

mod support;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use bmo_chat::{ChatProvider, ChatReceiver, ChatReceiverLink};
use bmo_telegram::file_ids::FileIds;
use bmo_telegram::provider::TelegramProvider;
use bmo_telegram::reader::ReaderClient;
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
// A contact of the owner's account the account has no chat with yet, and the bot neither.
const CONTACT: i64 = 66;
// The group a start makes.
const MADE_GROUP: i64 = -4_000_001;
const OTHER_ROOM: i64 = -200;
// A room nobody lists and the bot is not in.
const UNKNOWN_ROOM: i64 = -300;
// A room the bot is in that the owner's account does not list, seen by the provider.
const BOT_ROOM: i64 = -400;
// A room the provider has seen that the bot is no longer in and the owner's account does not list.
const LEFT_ROOM: i64 = -500;
const OWNER: i64 = 42;
const BOT: i64 = 7_000_000_001_i64;
const SENT_AT: i64 = 1_700_000_000_000;

/// A port nothing listens on: the reader that does not stand behind the service.
async fn closed_port() -> u16 {
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");

  listener.local_addr().expect("a bound address").port()
}

// The Bot API: who the bot is, the rooms it is in and what it calls them, and no updates at all. The bot is a
// member of `ROOM` and `BOT_ROOM`, has no chat with `PERSON`, and is in no other room.
async fn bot_api() -> String {
  let router = Router::new().fallback(move |request: Request| {
    async move {
      let path = request.uri().path().to_owned();
      let method = path.rsplit('/').next().unwrap_or_default().to_owned();
      let asked = axum::body::to_bytes(request.into_body(), usize::MAX).await.unwrap_or_default();
      let asked: Value = serde_json::from_slice(&asked).unwrap_or(Value::Null);
      let chat = asked["chat_id"].as_i64();
      let not_found = json!({ "ok": false, "error_code": 400, "description": "Bad Request: chat not found" });

      if method == "getUpdates" {
        return std::future::pending::<Response>().await;
      }

      let answer = match method.as_str() {
        "getMe" => json!({ "ok": true, "result": { "id": BOT, "is_bot": true, "first_name": "Helper", "username": "helper_bot" } }),
        "getChat" if matches!(chat, Some(PERSON | CONTACT)) => not_found,
        "getChat" => json!({ "ok": true, "result": { "title": "Board room" } }),
        "getChatMember" if asked["user_id"] == json!(BOT) && matches!(chat, Some(ROOM | BOT_ROOM)) => {
          json!({ "ok": true, "result": { "status": "member", "user": { "id": BOT } } })
        }
        "getChatMember" if chat == Some(LEFT_ROOM) => {
          json!({ "ok": true, "result": { "status": "left", "user": { "id": BOT } } })
        }
        "getChatMember" => not_found,
        _ => json!({ "ok": true, "result": true }),
      };

      Json(answer).into_response()
    }
  });

  format!("http://127.0.0.1:{}", serve(router).await)
}

// The receiving side: every hand-over and every call kept; a hand-over answered as held.
#[derive(Default)]
struct Kept {
  handed: Mutex<Vec<(String, Value)>>,
}

async fn receiver_served(kept: Arc<Kept>) -> String {
  let router = Router::new().fallback(move |request: Request| {
    let kept = kept.clone();

    async move {
      let method = request.method().clone();
      let path = request.uri().path().to_owned();
      let bytes = axum::body::to_bytes(request.into_body(), usize::MAX).await.unwrap_or_default();
      let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

      kept.handed.lock().unwrap().push((path, body.clone()));

      if method == axum::http::Method::PUT {
        (StatusCode::CREATED, Json(body)).into_response()
      } else {
        Json(json!({})).into_response()
      }
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
      "/reader/dialogs",
      get(|| async {
        Json(json!({ "rooms": [
          { "chatId": PERSON, "title": "Someone" },
          { "chatId": ROOM, "title": "Board" },
          { "chatId": OWNER, "title": "Helper" },
          { "chatId": OTHER_ROOM, "title": null }
        ] }))
      }),
    )
    .route(
      "/reader/contacts",
      get(|| async {
        Json(json!({ "contacts": [
          { "id": PERSON, "name": "Someone", "bot": false, "chatId": PERSON },
          { "id": CONTACT, "name": "A contact", "bot": false, "chatId": null },
          { "id": BOT, "name": "Helper", "bot": true, "chatId": OWNER }
        ] }))
      }),
    )
    .route(
      "/reader/start",
      post(|Json(body): Json<Value>| async move {
        Json(match (body["contactId"].as_i64(), body["title"].as_str()) {
          (Some(PERSON), _) => json!({ "outcome": "started", "chatId": PERSON, "title": "Someone" }),
          (Some(CONTACT), _) => json!({ "outcome": "started", "chatId": CONTACT, "title": "A contact" }),
          (Some(other), _) => json!({ "outcome": "no-contact", "contactId": other }),
          (None, Some("Refused")) => json!({ "outcome": "refused", "reason": "USER_PRIVACY_RESTRICTED" }),
          (None, title) => json!({ "outcome": "started", "chatId": MADE_GROUP, "title": title }),
        })
      }),
    )
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
  seen: Arc<Seen>,
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
    seen: seen.clone(),
    standing: standing.clone(),
    presence: telegram.clone(),
  });

  let pictures = tempdir().expect("a temporary folder");
  let avatars = pictures.path().join("avatars");

  let person = avatars.join("person");

  std::fs::create_dir_all(&person).expect("the pictures folder is made");
  std::fs::write(person.join(format!("{PERSON}.jpg")), [0xff, 0xd8]).expect("a picture is kept");

  let provider = Arc::new(TelegramProvider::new(
    telegram,
    Sending::default(),
    Some(reader),
    seen.clone(),
    standing,
    avatars,
  ));

  provider.start(receiver).await.expect("the provider starts");

  // The bot account is ready once Telegram has said who it is, which the stand-in answers at once.
  for _ in 0..500 {
    if provider.health().ready {
      break;
    }

    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
  }

  assert!(provider.health().ready, "the provider is ready");

  let app = service(provider.clone(), seam);

  Served {
    base: format!("http://127.0.0.1:{}", serve(app).await),
    provider,
    seen,
    file_ids,
    kept: handed,
    asked,
    _pictures: pictures,
  }
}

/// One route of the served service asked as the receiving side asks it: the status and the JSON answer.
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
    json!({
      "rooms": "native", "people": "native", "messages": "native",
      "ranges": { "rooms": null, "people": null }
    })
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
  assert_eq!(answer["modes"][1]["canReplyTo"], true);

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
      "roomList",
      "contactList",
      "startChat",
      "startGroup",
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
    assert_eq!(answer["version"], env!("CARGO_PKG_VERSION"));
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

/// Every room question the protocol asks, answered through the served routes: the owner's one-to-one and a
/// room the bot is in are the provider's own under the bot's number, titled by the Bot API; a chat of the owner's
/// account the bot is not in is the provider's own with no number of its own, titled by the account; a one-to-one
/// is numbered by its person; a room nobody lists and the bot is not in is not found.
#[tokio::test]
async fn every_room_route_answers_for_a_room_the_provider_reads() {
  let served = stood(true).await;

  let (status, owner) = asked_get(&served.base, &format!("/rooms/{OWNER}")).await;

  assert_eq!(status, 200);
  assert_eq!(
    owner,
    json!({ "id": OWNER, "owns": true, "identity": BOT, "kind": "owner", "privateTo": OWNER,
      "title": "Board room", "modes": ["bot-account", "human-reader"] })
  );

  let (status, bot_room) = asked_get(&served.base, &format!("/rooms/{ROOM}")).await;

  assert_eq!(status, 200);
  assert_eq!(
    bot_room,
    json!({ "id": ROOM, "owns": true, "identity": BOT, "kind": "group", "privateTo": null, "title": "Board room",
      "modes": ["bot-account", "human-reader"] })
  );

  let (status, person) = asked_get(&served.base, &format!("/rooms/{PERSON}")).await;

  assert_eq!(status, 200);
  assert_eq!(
    person,
    json!({ "id": PERSON, "owns": true, "identity": null, "kind": "group", "privateTo": PERSON,
      "title": "Someone", "modes": ["human-reader"] })
  );

  let (status, unknown) = asked_get(&served.base, &format!("/rooms/{UNKNOWN_ROOM}")).await;

  assert_eq!(status, 404);
  assert_eq!(unknown["code"], "not-found");

  let (status, owner_room) = asked_get(&served.base, "/owner").await;

  assert_eq!(status, 200);
  assert_eq!(owner_room, json!({ "roomId": OWNER }));
}

/// The bot's rooms and the owner's account's, listed through the reader seam and the Bot API and paged by
/// bmo-chat's provider server in ascending order of room, each once: a chat the bot is also in is the bot's,
/// under the account's title; a chat the bot is not in carries no number of the provider's own; a room of the
/// bot's the account does not list is titled by the Bot API, and a room seen that the bot left is not listed.
/// Where no reader stands, `roomList` is not declared and the list is refused.
#[tokio::test]
async fn the_bot_s_rooms_and_the_owner_s_account_s_are_listed_once_each() {
  let served = stood(true).await;
  served.provider.refresh_reader().await;
  served.seen.room(BOT_ROOM);
  served.seen.room(LEFT_ROOM);
  served.seen.room(ROOM);

  let (status, first) = asked_get(&served.base, "/rooms?limit=3").await;

  assert_eq!(status, 200);
  assert_eq!(
    first,
    json!({ "items": [
      { "id": BOT_ROOM, "owns": true, "identity": BOT, "kind": "group", "privateTo": null, "title": "Board room",
        "modes": ["bot-account", "human-reader"] },
      { "id": OTHER_ROOM, "owns": true, "identity": null, "kind": "group", "privateTo": null, "title": null,
        "modes": ["human-reader"] },
      { "id": ROOM, "owns": true, "identity": BOT, "kind": "group", "privateTo": null, "title": "Board",
        "modes": ["bot-account", "human-reader"] }
    ], "more": true })
  );

  let (status, next) = asked_get(&served.base, &format!("/rooms?after={ROOM}&limit=3")).await;

  assert_eq!(status, 200);
  assert_eq!(
    next,
    json!({ "items": [
      { "id": OWNER, "owns": true, "identity": BOT, "kind": "owner", "privateTo": OWNER, "title": "Helper",
        "modes": ["bot-account", "human-reader"] },
      { "id": PERSON, "owns": true, "identity": null, "kind": "group", "privateTo": PERSON, "title": "Someone",
        "modes": ["human-reader"] }
    ], "more": false })
  );

  let down = stood(false).await;
  down.provider.refresh_reader().await;

  let (status, refused) = asked_get(&down.base, "/rooms?limit=3").await;

  assert_eq!(status, 422);
  assert_eq!(refused["code"], "unsupported-capability");
}

/// The owner's account's contacts, listed through the reader seam and paged by bmo-chat's provider server in
/// ascending order of number, each with its kind and the one-to-one the account has with it. Where no reader
/// stands, `contactList` is not declared and the list is refused.
#[tokio::test]
async fn the_owner_s_account_s_contacts_are_listed_with_their_one_to_one() {
  let served = stood(true).await;
  served.provider.refresh_reader().await;

  let (status, listed) = asked_get(&served.base, "/contacts?limit=2").await;

  assert_eq!(status, 200);
  assert_eq!(
    listed,
    json!({ "items": [
      { "id": PERSON, "name": "Someone", "kind": "person", "roomId": PERSON },
      { "id": CONTACT, "name": "A contact", "kind": "person", "roomId": null }
    ], "more": true })
  );

  let (_, rest) = asked_get(&served.base, &format!("/contacts?after={CONTACT}")).await;

  assert_eq!(rest["items"], json!([{ "id": BOT, "name": "Helper", "kind": "bot", "roomId": OWNER }]));

  let down = stood(false).await;
  down.provider.refresh_reader().await;

  let (status, refused) = asked_get(&down.base, "/contacts").await;

  assert_eq!((status, &refused["code"]), (422, &json!("unsupported-capability")));
}

/// A conversation started by the owner's account: a one-to-one with a contact is an owner-account room,
/// read-only, the bot not being in it, and is known as a room before its first message; a group made is
/// answered by its new number; a number that is no contact is not found, and what Telegram refuses is
/// refused `platform`. Where no reader stands, neither shape is declared and the start is refused.
#[tokio::test]
async fn a_conversation_is_started_by_the_owner_s_account_and_read_only_where_the_bot_is_not_in_it() {
  let served = stood(true).await;
  served.provider.refresh_reader().await;
  let read_only = |id: i64, title: &str, private_to: Value| {
    json!({ "id": id, "owns": true, "identity": null, "kind": "group", "privateTo": private_to, "title": title,
      "modes": ["human-reader"] })
  };

  let (status, room) = asked(&served.base, "/rooms", json!({ "contactId": CONTACT })).await;

  assert_eq!((status, room), (201, read_only(CONTACT, "A contact", json!(CONTACT))));
  assert_eq!(
    asked_get(&served.base, &format!("/rooms/{CONTACT}")).await,
    (200, read_only(CONTACT, "A contact", json!(CONTACT))),
    "a contact's one-to-one is known before its first message"
  );

  let (status, room) =
    asked(&served.base, "/rooms", json!({ "title": "Plans", "contactIds": [CONTACT, PERSON] })).await;

  assert_eq!((status, room), (201, read_only(MADE_GROUP, "Plans", Value::Null)));

  let (status, missing) = asked(&served.base, "/rooms", json!({ "contactId": 77 })).await;

  assert_eq!((status, &missing["code"]), (404, &json!("not-found")));

  let (status, refused) =
    asked(&served.base, "/rooms", json!({ "title": "Refused", "contactIds": [CONTACT] })).await;

  assert_eq!(
    (status, refused),
    (422, json!({ "code": "platform", "error": "Telegram: USER_PRIVACY_RESTRICTED" }))
  );

  let down = stood(false).await;
  down.provider.refresh_reader().await;

  let (status, refused) = asked(&down.base, "/rooms", json!({ "contactId": CONTACT })).await;

  assert_eq!((status, &refused["code"]), (422, &json!("unsupported-capability")));
}

/// A message's picture fetched again through the reader seam: the bytes the owner's account hands over, under
/// the file's own name and the owner's account's id, or the id the bot account handed it over with. Where no
/// reader stands, the capability is not declared and the fetch is refused.
#[tokio::test]
async fn a_picture_is_fetched_again_through_the_reader_seam_under_the_id_it_crossed_with() {
  let served = stood(true).await;
  let route = format!("/rooms/{ROOM}/messages/6/files?sentAt={SENT_AT}");

  let (status, answer) = asked_get(&served.base, &route).await;

  assert_eq!(status, 200);
  assert_eq!(
    answer,
    json!({ "items": [{ "id": "photo-9", "name": "picture.jpg", "mime": "image/jpeg", "bytes": "AQID" }],
      "more": false })
  );

  served.file_ids.keep(ROOM, 6, "photo-unique");

  let (_status, again) = asked_get(&served.base, &route).await;

  assert_eq!(again["items"][0]["id"], "photo-unique");

  let down = stood(false).await;
  let (status, refused) = asked_get(&down.base, &route).await;

  assert_eq!(status, 422);
  assert_eq!(refused["code"], "unsupported-capability");
}

/// A send in the owner's own voice goes through the seam to the reader: the words cross to the owner's account,
/// which answers the message it went out as.
#[tokio::test]
async fn a_send_as_the_owner_goes_through_the_reader() {
  let served = stood(true).await;
  let words = "words in the owner's own voice";

  let (status, answer) = asked(
    &served.base,
    "/sends",
    json!({ "roomId": ROOM, "kind": "text", "fields": { "text": words }, "mode": "human-reader" }),
  )
  .await;

  assert_eq!(status, 201);
  assert_eq!(answer, json!({ "messageId": 900, "missed": [] }));

  let asked = served.asked.lock().unwrap().clone();

  assert_eq!(
    asked.last().unwrap().clone(),
    json!({ "chatId": ROOM, "text": words, "markup": null, "silent": false, "replyToMessageId": null })
  );
}

/// A room of the owner's account the bot is not in is read-only: a send in any voice but the owner's own, or
/// naming none, is refused `platform` whatever its verb, and the owner's own words go through the reader. The
/// declaration carries no `human-avatar` mode, so no room is answered as the owner. A room the bot is in still
/// takes the bot's sends.
#[tokio::test]
async fn a_room_the_bot_is_not_in_takes_only_the_owner_s_own_words() {
  let served = stood(true).await;
  served.provider.refresh_reader().await;

  let (_status, declaration) = asked_get(&served.base, "/declaration").await;
  let voices: Vec<&Value> =
    declaration["modes"].as_array().unwrap().iter().map(|mode| &mode["voice"]).collect();

  assert_eq!(voices, [&json!("bot-account"), &json!("human-reader")]);

  for mode in [Value::Null, json!("bot-account"), json!("human-avatar")] {
    for (kind, fields) in [("text", json!({ "text": "an answer" })), ("typing", json!({ "on": true }))] {
      for room in [OTHER_ROOM, PERSON] {
        let (status, refused) = asked(
          &served.base,
          "/sends",
          json!({ "roomId": room, "kind": kind, "fields": fields, "mode": mode }),
        )
        .await;

        assert_eq!(status, 422, "{mode} {kind} {room}");
        assert_eq!(refused["code"], "platform", "{mode} {kind} {room}");
        assert_eq!(
          refused["error"],
          "Telegram: The bot is not in this room: it is the owner's account's alone, and takes only the owner's own words.",
        );
      }
    }
  }

  assert!(served.asked.lock().unwrap().is_empty(), "the reader was asked nothing");

  let (status, answer) = asked(
    &served.base,
    "/sends",
    json!({ "roomId": OTHER_ROOM, "kind": "text", "fields": { "text": "the owner's words" }, "mode": "human-reader" }),
  )
  .await;

  assert_eq!(status, 201);
  assert_eq!(answer, json!({ "messageId": 900, "missed": [] }));
  assert_eq!(served.asked.lock().unwrap().len(), 1, "the owner's words went through the reader");

  let (status, _answer) = asked(
    &served.base,
    "/sends",
    json!({ "roomId": ROOM, "kind": "typing", "fields": { "on": true } }),
  )
  .await;

  assert_eq!(status, 201, "a room the bot is in takes the bot's sends");
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
    .get(format!("{base}/people/{PERSON}/avatar"))
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

  let (status, missing) = asked_get(&served.base, "/rooms/9/avatar").await;

  assert_eq!(status, 404);
  assert_eq!(missing["code"], "not-found");

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
    "roomId": ROOM,
    "messageId": 7,
    "message": {
      "origin": "live",
      "senderId": PERSON, "senderName": "Other bot", "sentAt": SENT_AT,
      "text": "words", "replyToMessageId": null, "via": "telegram", "album": null,
      "mode": "human-reader",
      "context": { "roomTitle": "Room", "carriesFile": false },
      "room": { "kind": "group", "identity": BOT, "privateTo": null },
      "files": [], "gathering": false
    }
  });

  let (status, _answer) = asked(&served.base, "/reader/inbound", handover.clone()).await;

  assert_eq!(status, 200);

  let handed = served.kept.handed.lock().unwrap().clone();

  assert_eq!(
    handed.iter().map(|(route, _)| route.as_str()).collect::<Vec<_>>(),
    [format!("/rooms/{ROOM}/messages/7").as_str()]
  );

  let (_status, seen) = asked_get(&served.base, "/reader/seen").await;

  assert_eq!(seen, json!({ "rooms": [ROOM], "people": [PERSON] }));

  let (status, room) = asked_get(&served.base, &format!("/rooms/{ROOM}")).await;

  assert_eq!(status, 200);
  assert_eq!(room["owns"], true);
  assert_eq!(room["kind"], "group");
}
