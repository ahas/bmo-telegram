//! The seam between the service and the reader (docs/boundary-api.md): the service's client of the reader's
//! routes against a fake reader, and the service's routes the reader calls, over an in-memory receiver.

use crate::support;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use bmo_chat::{ChatPresence, ChatPresenceState, FileItem, MessageFile, ReceiverRoom, TypingMark};
use bmo_telegram::avatar::AvatarLines;
use bmo_telegram::compose::Part;
use bmo_telegram::policy::POLICY;
use bmo_telegram::reader::{AsOwner, OwnerSent, ReaderClient, ReaderFetch, ReaderRoom};
use bmo_telegram::seam::{Presence, Seam, Standing, seam_router};
use bmo_telegram::seen::Seen;
use bmo_telegram::wire::Wire;
use serde_json::{Value, json};
use support::{KeptReceiver, OWNER};

const TOKEN: &str = "seam-token";
// The one room of the receiver's the bot is in.
const BOT_ROOM: i64 = -1001;
// A one-to-one of the owner's account the bot is not in, added, so the receiver reads it.
const ADDED_PERSON: i64 = 77;

// The bot account, in `BOT_ROOM` alone.
struct InBotRoom;

#[async_trait::async_trait]
impl Presence for InBotRoom {
  async fn bot_in(&self, chat_id: i64) -> bool {
    chat_id == BOT_ROOM
  }
}

// A router served on the loopback at a port of its own, for as long as the test runs.
async fn serve(router: Router) -> u16 {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
  let port = listener.local_addr().expect("a bound address").port();

  tokio::spawn(async move { axum::serve(listener, router).await.expect("the fake serves") });

  port
}

// A port nothing listens on.
async fn closed_port() -> u16 {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");

  listener.local_addr().expect("a bound address").port()
}

// A reader with its account connected, that answers each route as the reader does and keeps what it is asked.
fn fake_reader(asked: Arc<Mutex<Vec<Value>>>) -> Router {
  let keep = move |body: Value| asked.lock().unwrap().push(body);
  let say = keep.clone();
  let album = keep.clone();

  Router::new()
    .route("/health", get(|| async { Json(json!({ "connected": true, "reason": null })) }))
    .route(
      "/reader/avatars",
      get(|| async {
        Json(json!({ "avatars": [
          { "kind": "person", "id": 55, "takenAt": 1_700_000_000_000_i64 },
          { "kind": "room", "id": -1001, "takenAt": 1_700_000_001_000_i64 }
        ] }))
      }),
    )
    .route(
      "/reader/dialogs",
      get(|| async {
        Json(json!({ "rooms": [{ "chatId": -1001, "title": "Board" }, { "chatId": 55, "title": null }] }))
      }),
    )
    .route(
      "/reader/picture",
      post(|Json(body): Json<Value>| async move {
        if body["messageId"] == 6 {
          Json(json!({ "outcome": "fetched", "id": "photo-6", "name": "6.jpg", "mime": "image/jpeg", "bytes": "AQID" }))
        } else {
          Json(json!({ "outcome": "unavailable", "reason": "the message carries no picture" }))
        }
      }),
    )
    .route(
      "/reader/file",
      post(|| async {
        Json(json!({ "outcome": "no-reader", "reason": "the owner's account is not connected" }))
      }),
    )
    .route(
      "/owner/say",
      post(move |Json(body): Json<Value>| async move {
        let refused = body["text"] == "unreachable";

        say(body);

        if refused {
          Json(json!({ "refused": "unreachable", "error": "no message to reply to" }))
        } else {
          Json(json!({ "messageId": 900, "text": "words as held" }))
        }
      }),
    )
    .route(
      "/owner/album",
      post(move |Json(body): Json<Value>| async move {
        album(body);
        Json(json!({ "ids": [901, null], "texts": ["a caption"] }))
      }),
    )
}

#[tokio::test]
async fn the_reader_is_asked_for_its_health_its_fetches_its_pictures_and_the_owner_s_sends() {
  let asked = Arc::new(Mutex::new(Vec::new()));
  let port = serve(fake_reader(asked.clone())).await;
  let reader = ReaderClient::new(Wire::loopback(port, TOKEN));
  let file = FileItem {
    name: "a.pdf".to_owned(),
    mime: "application/pdf".to_owned(),
    bytes: vec![1, 2],
  };

  assert!(reader.health().await.connected);
  assert_eq!(
    reader.dialogs().await,
    Ok(vec![
      ReaderRoom {
        chat_id: -1001,
        title: Some("Board".to_owned())
      },
      ReaderRoom {
        chat_id: 55,
        title: None
      },
    ])
  );
  assert_eq!(
    reader.picture(5, 6, Some(1)).await,
    ReaderFetch::Fetched(MessageFile {
      id: "photo-6".to_owned(),
      name: "6.jpg".to_owned(),
      mime: "image/jpeg".to_owned(),
      bytes: vec![1, 2, 3],
      kind: None,
    })
  );
  assert_eq!(
    reader.picture(5, 7, None).await,
    ReaderFetch::Unavailable("the message carries no picture".to_owned())
  );
  assert_eq!(
    reader.file(5, 7, 1).await,
    ReaderFetch::NoReader("the owner's account is not connected".to_owned())
  );
  let part = |plain: &str, markup: Option<&str>| Part {
    plain: plain.to_owned(),
    markup: markup.map(str::to_owned),
  };
  let sent = |message_id: Option<i64>, words: &str| OwnerSent {
    message_id,
    words: words.to_owned(),
  };

  assert_eq!(
    reader.owner_say(5, &part("words", Some("<b>words</b>")), true, Some(3)).await,
    Ok(AsOwner::Sent(sent(Some(900), "words as held")))
  );
  assert_eq!(
    reader.owner_say(5, &part("unreachable", None), false, Some(3)).await,
    Ok(AsOwner::Refused {
      name: "unreachable".to_owned(),
      error: "no message to reply to".to_owned()
    })
  );
  assert_eq!(
    reader.owner_album(5, &[file.clone(), file], &part("a caption", None), None).await,
    Ok(AsOwner::Sent(vec![sent(Some(901), "a caption"), sent(None, "")]))
  );

  let asked = asked.lock().unwrap().clone();

  assert_eq!(
    asked[0],
    json!({ "chatId": 5, "text": "words", "markup": "<b>words</b>", "silent": true, "replyToMessageId": 3 })
  );
  assert_eq!(asked[2]["caption"], "a caption");
  assert_eq!(asked[2]["captionMarkup"], Value::Null);
  assert_eq!(
    asked[2]["files"][0],
    json!({ "name": "a.pdf", "mime": "application/pdf", "bytes": "AQI=" })
  );
  assert_eq!(
    reader.avatars().await.unwrap(),
    vec![
      bmo_chat::ChatAvatar {
        kind: bmo_chat::ChatAvatarKind::Person,
        id: 55,
        taken_at: 1_700_000_000_000
      },
      bmo_chat::ChatAvatar {
        kind: bmo_chat::ChatAvatarKind::Room,
        id: -1001,
        taken_at: 1_700_000_001_000
      },
    ]
  );
}

#[tokio::test]
async fn a_reader_that_does_not_answer_has_no_account_to_ask_and_carries_no_send() {
  let reader = ReaderClient::new(Wire::loopback(closed_port().await, TOKEN));

  assert!(!reader.health().await.connected);
  assert!(reader.dialogs().await.is_err());
  assert!(matches!(reader.picture(5, 6, None).await, ReaderFetch::Unavailable(_)));
  let words = Part {
    plain: "words".to_owned(),
    markup: None,
  };

  assert!(reader.owner_say(5, &words, false, None).await.is_err());
}

/// A reader whose account is not connected answers its dialogs 503 `not-ready`, and the reason is what the
/// service is told.
#[tokio::test]
async fn a_reader_whose_account_is_not_connected_lists_no_rooms() {
  let down = Router::new().route(
    "/reader/dialogs",
    get(|| async {
      (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({ "code": "not-ready", "error": "the account's values are not set" })),
      )
    }),
  );
  let reader = ReaderClient::new(Wire::loopback(serve(down).await, TOKEN));

  assert_eq!(reader.dialogs().await, Err("the account's values are not set".to_owned()));
}

struct Served {
  port: u16,
  receiver: Arc<KeptReceiver>,
  standing: Arc<Standing>,
}

async fn seam_over(receiver: Arc<KeptReceiver>) -> Served {
  let seen = Arc::new(Seen::in_memory());
  let standing = Arc::new(Standing::default());
  let seam = Seam {
    token: TOKEN.to_owned(),
    receiver: receiver.clone(),
    seen,
    standing: standing.clone(),
    presence: Arc::new(InBotRoom),
    avatar: AvatarLines::new(OWNER, None),
  };
  let port = serve(seam_router(Arc::new(seam))).await;

  Served {
    port,
    receiver,
    standing,
  }
}

fn message(origin: &str, room_id: i64, message_id: i64, sender: i64) -> Value {
  json!({
    "kind": "message",
    "roomId": room_id,
    "messageId": message_id,
    "message": {
      "origin": origin,
      "senderId": sender, "senderName": "Other bot", "sentAt": 1_700_000_000_000_i64,
      "text": "words", "replyToMessageId": null, "via": "telegram", "album": null,
      "mode": "human-reader",
      "context": { "roomTitle": "Room", "carriesFile": true },
      "room": { "kind": "group", "identity": 7_000_000_001_i64, "privateTo": null },
      "files": [], "gathering": true
    }
  })
}

/// A video the reader gathered crosses whole: a hand-over far past a web server's usual two megabytes, as a
/// twelve-second phone video is, reaches the receiver rather than being refused for its size.
#[tokio::test]
async fn a_gathered_video_of_many_megabytes_crosses_the_seam_whole() {
  let (receiver, _heard) = KeptReceiver::new();

  *receiver.rooms.lock().unwrap() = vec![ReceiverRoom {
    id: -1001,
    listened: true,
    read: false,
    oldest_message_id: Some(5),
    oldest_sent_at: Some(1_700_000_000_000),
  }];

  let served = seam_over(receiver).await;
  let seam = Wire::loopback(served.port, TOKEN);

  assert_eq!(seam.post("/reader/inbound", &message("live", -1001, 7, 55)).await.unwrap().status, 200);

  // Twelve million bytes, as base64.
  let bytes = "A".repeat(16_000_000);
  let gathered = json!({
    "kind": "gathered", "roomId": -1001, "messageId": 7,
    "patch": { "files": [{ "id": "f1", "name": "IMG_0001.MOV", "mime": "video/quicktime", "bytes": bytes, "kind": "video" }] }
  });

  assert_eq!(seam.post("/reader/inbound", &gathered).await.unwrap().status, 200);

  let handed = served.receiver.handed.lock().unwrap().clone();

  assert_eq!(
    handed.iter().map(|(route, _)| route.as_str()).collect::<Vec<_>>(),
    ["message", "gathered"]
  );
}

#[tokio::test]
async fn what_the_reader_hands_over_goes_on_to_the_receiver_and_its_room_and_sender_are_seen() {
  let (receiver, _heard) = KeptReceiver::new();

  *receiver.rooms.lock().unwrap() = vec![ReceiverRoom {
    id: -1001,
    listened: true,
    read: false,
    oldest_message_id: Some(5),
    oldest_sent_at: Some(1_700_000_000_000),
  }];

  let served = seam_over(receiver).await;
  let seam = Wire::loopback(served.port, TOKEN);

  assert_eq!(seam.post("/reader/inbound", &message("live", -1001, 7, 55)).await.unwrap().status, 200);
  assert_eq!(seam.post("/reader/inbound", &message("live", -1002, 7, 56)).await.unwrap().status, 200);

  let gathered = json!({
    "kind": "gathered", "roomId": -1001, "messageId": 7,
    "patch": { "files": [], "gatherFailure": "The file could not be kept." }
  });

  assert_eq!(seam.post("/reader/inbound", &gathered).await.unwrap().status, 200);

  let handed = served.receiver.handed.lock().unwrap().clone();

  assert_eq!(
    handed.iter().map(|(route, _)| route.as_str()).collect::<Vec<_>>(),
    ["message", "message", "gathered"]
  );
  assert_eq!(handed[0].1["gathering"], true);
  // A room the bot is in keeps the bot's number as its identity; one it is not in carries none.
  assert_eq!(handed[0].1["room"]["identity"], 7_000_000_001_i64);
  assert_eq!(handed[1].1["room"], json!({ "kind": "group" }));
  assert_eq!(handed[2].1["gatherFailure"], "The file could not be kept.");
  assert_eq!(
    seam.get("/reader/seen").await.unwrap().body,
    json!({ "rooms": [-1002, -1001], "people": [55, 56] })
  );

  let history = seam.post("/reader/inbound", &message("history", -1003, 8, 55)).await.unwrap();

  assert_eq!((history.status, history.body), (200, json!({ "kept": true })));
  assert_eq!(
    seam.post("/reader/listened", &json!({ "chatId": -9 })).await.unwrap().body,
    json!({ "listened": false, "people": false, "tells": false })
  );
  assert_eq!(
    seam.post("/reader/listened", &json!({ "chatId": -1001 })).await.unwrap().body,
    json!({ "listened": true, "people": false, "tells": true })
  );
  assert_eq!(seam.get("/reader/rooms").await.unwrap().body["rooms"][0]["oldestMessageId"], 5);
  assert_eq!(
    seam
      .post("/reader/avatars", &json!({ "rooms": [-1001], "people": [] }))
      .await
      .unwrap()
      .status,
    200
  );
  assert_eq!(served.standing.history(), None);
  assert_eq!(served.standing.avatars(), None);
  assert_eq!(served.receiver.avatar_changes.lock().unwrap().clone()[0].room_ids, vec![-1001]);
}

// A line of the owner's account the reader hands over, with the words typed and the room's identity and person as
// the reader names them.
fn owner_line(room_id: i64, message_id: i64, words: &str) -> Value {
  json!({
    "kind": "message",
    "roomId": room_id,
    "messageId": message_id,
    "message": {
      "origin": "history",
      "senderId": OWNER, "senderName": "Owner", "sentAt": 1_700_000_000_000_i64,
      "text": words, "replyToMessageId": null, "via": "telegram", "album": null,
      "mode": "human-reader",
      "context": { "spokenText": words },
      "room": { "kind": "group", "identity": 7_000_000_001_i64, "privateTo": (room_id > 0).then_some(room_id) },
      "files": []
    }
  })
}

/// A line the avatar sent, read back through the reader in a room the bot is in or one the receiver reads, goes on
/// as the agent's own under the room's identity with the prefix taken off, a read room naming no person; an
/// unprefixed line is the owner's, and so is a prefixed one in a room the bot is not in that the receiver does not
/// read, where no avatar send goes (docs/telegram.md, 「The rooms」, 「The avatar」).
#[tokio::test]
async fn a_line_the_avatar_sent_read_back_through_the_reader_goes_on_as_the_agent_s_own() {
  let (receiver, _heard) = KeptReceiver::new();

  *receiver.rooms.lock().unwrap() = vec![ReceiverRoom {
    id: ADDED_PERSON,
    listened: true,
    read: true,
    oldest_message_id: None,
    oldest_sent_at: None,
  }];

  let served = seam_over(receiver).await;
  let seam = Wire::loopback(served.port, TOKEN);
  let prefix = &POLICY.avatar.prefix;
  let lines = [
    owner_line(BOT_ROOM, 1, &format!("{prefix}on my way")),
    owner_line(BOT_ROOM, 2, "typed by hand"),
    owner_line(-1002, 3, &format!("{prefix}typed by hand")),
    owner_line(ADDED_PERSON, 4, &format!("{prefix}on my way")),
  ];

  for line in &lines {
    assert_eq!(seam.post("/reader/inbound", line).await.unwrap().status, 200);
  }

  let handed = served.receiver.handed.lock().unwrap().clone();
  let said: Vec<(Value, Value, Value, Value)> = handed
    .iter()
    .map(|(_, body)| {
      (
        body["senderId"].clone(),
        body["text"].clone(),
        body["context"]["spokenText"].clone(),
        body["mode"].clone(),
      )
    })
    .collect();
  let typed = format!("{prefix}typed by hand");

  assert_eq!(
    said,
    [
      (json!(7_000_000_001_i64), json!("on my way"), json!("on my way"), json!("human-avatar")),
      (json!(OWNER), json!("typed by hand"), json!("typed by hand"), json!("human-reader")),
      (json!(OWNER), json!(typed), json!(typed), json!("human-reader")),
      (json!(7_000_000_001_i64), json!("on my way"), json!("on my way"), json!("human-avatar")),
    ]
  );
  // A room the receiver reads keeps the agent's identity and names no person; one it does not read keeps none.
  assert_eq!(handed[2].1["room"], json!({ "kind": "group" }));
  assert_eq!(handed[3].1["room"], json!({ "kind": "group", "identity": 7_000_000_001_i64 }));
}

/// A person's words are the reader's to hand over only in a room the bot is not in whose `read` the receiving
/// side answers true; being listened to is no such request (docs/telegram.md, 「The rooms」). Their typing and
/// presence are the reader's to tell in a listened room the bot is in as well (「Typing and presence」).
#[tokio::test]
async fn a_person_s_words_are_handed_over_only_where_the_receiver_asked_for_a_room_the_bot_is_not_in() {
  let (receiver, _heard) = KeptReceiver::new();
  let held = |id, read| ReceiverRoom {
    id,
    listened: true,
    read,
    oldest_message_id: None,
    oldest_sent_at: None,
  };

  *receiver.rooms.lock().unwrap() = vec![held(BOT_ROOM, true), held(-1004, true), held(-1005, false)];

  let served = seam_over(receiver).await;
  let seam = Wire::loopback(served.port, TOKEN);
  let people = async |chat_id: i64| {
    let answer = seam.post("/reader/listened", &json!({ "chatId": chat_id })).await.unwrap().body;
    (answer["listened"].clone(), answer["people"].clone(), answer["tells"].clone())
  };

  assert_eq!(
    people(BOT_ROOM).await,
    (json!(true), json!(false), json!(true)),
    "the bot's own room is read by the bot, and its people's typing told"
  );
  assert_eq!(
    people(-1004).await,
    (json!(true), json!(true), json!(true)),
    "a room the receiver asked to be read"
  );
  assert_eq!(
    people(-1005).await,
    (json!(true), json!(false), json!(false)),
    "a room listened to but not asked for"
  );
  assert_eq!(
    people(-1006).await,
    (json!(false), json!(false), json!(false)),
    "a room the receiver holds nothing of"
  );
}

#[tokio::test]
async fn where_the_receiver_does_not_answer_each_need_falls_back_as_the_boundary_states() {
  let (receiver, _heard) = KeptReceiver::new();

  *receiver.room_failure.lock().unwrap() = Some("the receiver failed".to_owned());
  *receiver.rooms_failure.lock().unwrap() = Some("the receiver failed".to_owned());
  *receiver.avatars_failure.lock().unwrap() = Some("the receiver failed".to_owned());
  *receiver.hand_over_failure.lock().unwrap() = Some("the receiver failed".to_owned());

  let served = seam_over(receiver.clone()).await;
  let seam = Wire::loopback(served.port, TOKEN);

  assert_eq!(
    seam.post("/reader/listened", &json!({ "chatId": -9 })).await.unwrap().body,
    json!({ "listened": true, "people": false, "tells": false })
  );

  let rooms = seam.get("/reader/rooms").await.unwrap();

  assert_eq!((rooms.status, rooms.body["code"].clone()), (503, json!("not-ready")));
  assert!(
    served
      .standing
      .history()
      .is_some_and(|why| why.starts_with("the history walk stands down"))
  );
  assert_eq!(
    seam.post("/reader/inbound", &message("history", -1001, 7, 55)).await.unwrap().status,
    500
  );
  assert_eq!(
    seam
      .post("/reader/avatars", &json!({ "rooms": [1], "people": [2] }))
      .await
      .unwrap()
      .status,
    200
  );
  assert!(served.standing.avatars().is_some());
}

#[tokio::test]
async fn the_seam_takes_its_own_token_alone_and_refuses_a_body_it_cannot_read() {
  let (receiver, _heard) = KeptReceiver::new();
  let served = seam_over(receiver.clone()).await;
  let stranger = Wire::loopback(served.port, "other");
  let seam = Wire::loopback(served.port, TOKEN);

  assert_eq!(stranger.get("/reader/seen").await.unwrap().status, 401);
  assert_eq!(seam.post("/reader/inbound", &json!({ "kind": "shout" })).await.unwrap().status, 400);
  assert!(receiver.handed.lock().unwrap().is_empty());
}

/// What the reader sees happen to a message afterwards goes on as the protocol's reaction change and deletion,
/// a message named by its number or by its stamp (docs/boundary-api.md, 「The service's routes, called by the
/// reader」).
#[tokio::test]
async fn a_reaction_and_a_deletion_the_reader_sees_go_on_to_the_receiver() {
  let (receiver, _heard) = KeptReceiver::new();
  let served = seam_over(receiver.clone()).await;
  let seam = Wire::loopback(served.port, TOKEN);
  let reacted = json!({
    "roomId": -5_000_000_001_i64,
    "stamp": { "senderId": 55, "sentAt": 1_700_000_000_000_i64 },
    "reactions": [{ "personId": 56, "emojis": ["\u{1F44D}"] }],
    "whole": true
  });

  assert_eq!(seam.post("/reader/reactions", &reacted).await.unwrap().status, 200);
  assert_eq!(
    seam
      .post("/reader/deleted", &json!({ "roomId": -1_001_234_567_890_i64, "messageIds": [7, 8] }))
      .await
      .unwrap()
      .status,
    200
  );

  let change = receiver.reaction_changes.lock().unwrap()[0].clone();

  assert_eq!((change.via.as_str(), change.room_id, change.whole), ("telegram", -5_000_000_001, true));
  assert_eq!(
    change.stamp.map(|stamp| (stamp.sender_id, stamp.sent_at)),
    Some((55, 1_700_000_000_000))
  );
  assert_eq!(change.reactions[0].emojis, ["\u{1F44D}"]);
  assert_eq!(receiver.deletions.lock().unwrap()[0].message_ids, [7, 8]);
}

/// Who is writing and who is reachable, as the reader hears them, go on as the protocol's typing mark, under the
/// policy's life, and presence change (docs/boundary-api.md, 「The service's routes, called by the reader」).
#[tokio::test]
async fn typing_and_presence_the_reader_hears_go_on_to_the_receiver() {
  let (receiver, _heard) = KeptReceiver::new();
  let served = seam_over(receiver.clone()).await;
  let seam = Wire::loopback(served.port, TOKEN);
  let typing = json!({ "roomId": BOT_ROOM, "personId": 55, "on": true });
  let presence = json!({
    "people": [
      { "personId": 55, "state": "online", "lastSeenAt": null },
      { "personId": 56, "state": "offline", "lastSeenAt": 1_700_000_000_000_i64 }
    ],
    "whole": true
  });

  assert_eq!(seam.post("/reader/typing", &typing).await.unwrap().status, 200);
  assert_eq!(seam.post("/reader/presence", &presence).await.unwrap().status, 200);
  assert_eq!(seam.post("/reader/typing", &json!({ "roomId": 1 })).await.unwrap().status, 400);

  assert_eq!(
    receiver.typing_marks.lock().unwrap().clone(),
    [TypingMark {
      via: "telegram".to_owned(),
      room_id: BOT_ROOM,
      person_id: 55,
      on: true,
      lasts_ms: Some(6_000),
    }]
  );

  let change = receiver.presence_changes.lock().unwrap()[0].clone();

  assert_eq!((change.via.as_str(), change.whole), ("telegram", true));
  assert_eq!(
    change.people.iter().map(|one| (one.person_id, one.presence)).collect::<Vec<_>>(),
    [
      (
        55,
        ChatPresence {
          state: ChatPresenceState::Online,
          last_seen_at: None
        }
      ),
      (
        56,
        ChatPresence {
          state: ChatPresenceState::Offline,
          last_seen_at: Some(1_700_000_000_000)
        }
      ),
    ]
  );
}

/// A reaction update of the bot account names the message by the bot's number and the person by theirs, with
/// every emoji they have now and no custom emoji (docs/telegram.md, 「Reading a room」).
#[test]
fn a_reaction_update_is_the_persons_reactions_now() {
  let update = json!({
    "chat": { "id": -1_001_234_567_890_i64 },
    "message_id": 42,
    "user": { "id": 56 },
    "new_reaction": [
      { "type": "emoji", "emoji": "\u{2764}" },
      { "type": "custom_emoji", "custom_emoji_id": "5" }
    ]
  });
  let change = bmo_telegram::telegram::reaction_change(&update).unwrap();

  assert_eq!((change.room_id, change.message_id), (-1_001_234_567_890, 42));
  assert_eq!(change.reactions[0].person_id, Some(56));
  assert_eq!(change.reactions[0].emojis, ["\u{2764}"]);
  assert!(
    bmo_telegram::telegram::reaction_change(&json!({ "chat": { "id": 1 }, "message_id": 1 })).is_none()
  );
}
