//! The seam between the service and the reader (docs/boundary-api.md): the service's client of the reader's
//! routes against a fake reader, and the service's routes the reader calls, over an in-memory receiver and a
//! fake of the receiver's needed routes.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use bmo_chat::{ChatFailure, ChatGathered, ChatHandover, ChatReceiver, FileItem};
use bmo_telegram::reader::{AsOwner, OwnerReply, ReaderClient, ReaderFetch};
use bmo_telegram::receiver::ReceiverNeeds;
use bmo_telegram::seam::{Seam, Standing, seam_router};
use bmo_telegram::seen::Seen;
use bmo_telegram::wire::Wire;
use serde_json::{Value, json};

const TOKEN: &str = "seam-token";

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
      "/reader/picture",
      post(|Json(body): Json<Value>| async move {
        if body["messageId"] == 6 {
          Json(json!({ "outcome": "fetched", "name": "6.jpg", "mime": "image/jpeg", "bytes": "AQID" }))
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
          Json(json!({ "messageId": 900 }))
        }
      }),
    )
    .route(
      "/owner/album",
      post(move |Json(body): Json<Value>| async move {
        album(body);
        Json(json!({ "ids": [901, null] }))
      }),
    )
}

#[tokio::test]
async fn the_reader_is_asked_for_its_health_its_fetches_and_the_owner_s_sends() {
  let asked = Arc::new(Mutex::new(Vec::new()));
  let port = serve(fake_reader(asked.clone())).await;
  let reader = ReaderClient::new(Wire::loopback(port, TOKEN));
  let reply = OwnerReply {
    message_id: 3,
    sent_at: 1_700_000_000,
    text: "earlier words".to_owned(),
  };
  let file = FileItem {
    name: "a.pdf".to_owned(),
    mime: "application/pdf".to_owned(),
    bytes: vec![1, 2],
  };

  assert!(reader.health().await.connected);
  assert_eq!(
    reader.picture(5, 6, Some(1)).await,
    ReaderFetch::Fetched(FileItem {
      name: "6.jpg".to_owned(),
      mime: "image/jpeg".to_owned(),
      bytes: vec![1, 2, 3]
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
  assert_eq!(reader.owner_say(5, "words", Some(&reply)).await, Ok(AsOwner::Sent(Some(900))));
  assert_eq!(
    reader.owner_say(5, "unreachable", None).await,
    Ok(AsOwner::Refused {
      name: "unreachable".to_owned(),
      error: "no message to reply to".to_owned()
    })
  );
  assert_eq!(
    reader.owner_album(5, &[file.clone(), file], "a caption", None).await,
    Ok(AsOwner::Sent(vec![Some(901), None]))
  );

  let asked = asked.lock().unwrap().clone();

  assert_eq!(
    asked[0],
    json!({ "chatId": 5, "text": "words", "reply": { "messageId": 3, "sentAt": 1_700_000_000, "text": "earlier words" } })
  );
  assert_eq!(
    asked[2]["files"][0],
    json!({ "name": "a.pdf", "mime": "application/pdf", "bytes": "AQI=" })
  );
}

#[tokio::test]
async fn a_reader_that_does_not_answer_has_no_account_to_ask_and_carries_no_send() {
  let reader = ReaderClient::new(Wire::loopback(closed_port().await, TOKEN));

  assert!(!reader.health().await.connected);
  assert!(matches!(reader.picture(5, 6, None).await, ReaderFetch::Unavailable(_)));
  assert!(reader.owner_say(5, "words", None).await.is_err());
}

// A receiver that keeps every hand-over, by the route it came on.
#[derive(Default)]
struct KeptReceiver {
  handed: Mutex<Vec<(String, Value)>>,
}

impl KeptReceiver {
  fn keep(&self, route: &str, body: &impl serde::Serialize) -> Result<(), ChatFailure> {
    self.handed.lock().unwrap().push((route.to_owned(), serde_json::to_value(body)?));
    Ok(())
  }
}

#[async_trait]
impl ChatReceiver for KeptReceiver {
  async fn message(&self, handover: ChatHandover) -> Result<(), ChatFailure> {
    self.keep("message", &handover)
  }

  async fn attachment(&self, handover: ChatHandover) -> Result<(), ChatFailure> {
    self.keep("attachment", &handover)
  }

  async fn edited(&self, handover: ChatHandover) -> Result<(), ChatFailure> {
    self.keep("edited", &handover)
  }

  async fn gathered(&self, gathered: ChatGathered) -> Result<(), ChatFailure> {
    self.keep("gathered", &gathered)
  }
}

// The receiver's needed routes, served: a room listened to but one, two rooms to walk, history kept, and word
// of pictures taken.
fn needs_served() -> Router {
  Router::new()
    .route(
      "/rooms/listened",
      post(|Json(body): Json<Value>| async move { Json(json!({ "listened": body["chatId"] != -9 })) }),
    )
    .route(
      "/rooms/of",
      post(|| async {
        Json(json!({ "rooms": [{ "chatId": -1001, "oldestMessageId": 5, "oldestSentAt": 1_700_000_000 }] }))
      }),
    )
    .route("/history", post(|| async { Json(json!({ "kept": true })) }))
    .route("/avatars/updated", post(|| async { Json(json!({})) }))
}

// A receiver that serves none of the needed routes.
fn needs_unserved() -> Router {
  Router::new().fallback(|| async {
    (StatusCode::NOT_FOUND, Json(json!({ "code": "no-route", "error": "no route" }))).into_response()
  })
}

struct Served {
  port: u16,
  receiver: Arc<KeptReceiver>,
  seen: Arc<Seen>,
  standing: Arc<Standing>,
}

async fn seam_over(needs: Router) -> Served {
  let receiver = Arc::new(KeptReceiver::default());
  let seen = Arc::new(Seen::in_memory());
  let standing = Arc::new(Standing::default());
  let needs_port = serve(needs).await;
  let seam = Seam {
    token: TOKEN.to_owned(),
    receiver: receiver.clone(),
    needs: ReceiverNeeds::new(Wire::loopback(needs_port, "receiver-token")),
    seen: seen.clone(),
    standing: standing.clone(),
  };
  let port = serve(seam_router(Arc::new(seam))).await;

  Served {
    port,
    receiver,
    seen,
    standing,
  }
}

fn message(kind: &str, chat_id: i64, sender: i64) -> Value {
  json!({
    "kind": kind,
    "message": {
      "chatId": chat_id, "messageId": 7, "senderId": sender, "senderName": "Other bot", "sentAt": 1_700_000_000,
      "text": "words", "replyToMessageId": null, "via": "telegram", "album": null, "mode": "human-reader"
    },
    "context": { "roomTitle": "Room", "carriesFile": true, "gathering": true },
    "room": { "kind": "group", "identity": 7_000_000_001_i64, "privateTo": null }
  })
}

#[tokio::test]
async fn what_the_reader_hands_over_goes_on_to_the_receiver_and_its_room_and_sender_are_seen() {
  let served = seam_over(needs_served()).await;
  let seam = Wire::loopback(served.port, TOKEN);

  assert_eq!(
    seam.post("/reader/inbound", &message("attachment", -1001, 55)).await.unwrap().status,
    200
  );
  assert_eq!(seam.post("/reader/inbound", &message("message", -1002, 56)).await.unwrap().status, 200);

  let gathered = json!({
    "kind": "gathered", "chatId": -1001, "messageId": 7, "via": "telegram", "outcome": "not-kept",
    "reason": "The file could not be kept."
  });

  assert_eq!(seam.post("/reader/inbound", &gathered).await.unwrap().status, 200);

  let handed = served.receiver.handed.lock().unwrap().clone();

  assert_eq!(
    handed.iter().map(|(route, _)| route.as_str()).collect::<Vec<_>>(),
    ["attachment", "message", "gathered"]
  );
  assert_eq!(handed[0].1["context"]["gathering"], true);
  assert_eq!(handed[2].1["reason"], "The file could not be kept.");
  assert_eq!(
    seam.get("/reader/seen").await.unwrap().body,
    json!({ "rooms": [-1002, -1001], "people": [55, 56] })
  );
  assert!(served.seen.has_room(-1001));

  let history = seam.post("/reader/inbound", &message("history", -1001, 55)).await.unwrap();

  assert_eq!((history.status, history.body), (200, json!({ "kept": true })));
  assert_eq!(
    seam.post("/reader/listened", &json!({ "chatId": -9 })).await.unwrap().body,
    json!({ "listened": false })
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
  assert_eq!((served.standing.history(), served.standing.avatars()), (None, None));
}

#[tokio::test]
async fn where_the_receiver_does_not_answer_each_need_falls_back_as_the_boundary_states() {
  let served = seam_over(needs_unserved()).await;
  let seam = Wire::loopback(served.port, TOKEN);

  assert_eq!(
    seam.post("/reader/listened", &json!({ "chatId": -9 })).await.unwrap().body,
    json!({ "listened": true })
  );

  let rooms = seam.get("/reader/rooms").await.unwrap();

  assert_eq!((rooms.status, rooms.body["code"].clone()), (409, json!("not-ready")));
  assert!(
    served
      .standing
      .history()
      .is_some_and(|why| why.starts_with("the history walk stands down"))
  );
  assert_eq!(seam.post("/reader/inbound", &message("history", -1001, 55)).await.unwrap().status, 409);
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
  let served = seam_over(needs_served()).await;
  let stranger = Wire::loopback(served.port, "other");
  let seam = Wire::loopback(served.port, TOKEN);

  assert_eq!(stranger.get("/reader/seen").await.unwrap().status, 401);
  assert_eq!(seam.post("/reader/inbound", &json!({ "kind": "shout" })).await.unwrap().status, 400);
  assert!(served.receiver.handed.lock().unwrap().is_empty());
}
