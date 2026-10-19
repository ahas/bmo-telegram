//! A send in the owner's own voice (docs/telegram.md, 「The owner's voice」): the owner's words cut where a client
//! cuts a long paste and said through a fake reader, and files gathered into the albums Telegram takes by their
//! kinds, each file's size judged and a caption's line breaks read as a form carries them.

mod support;

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::response::Json;
use axum::routing::{get, post};
use bmo_chat::{ChatProvider, ChatSend, ChatSendRequest, ChatSendResult, ChatVoice};
use bmo_telegram::file_ids::FileIds;
use bmo_telegram::owner::{albums, caption_of, kind_of, megabytes, refusal};
use bmo_telegram::policy::{POLICY, fill};
use bmo_telegram::provider::TelegramProvider;
use bmo_telegram::reader::ReaderClient;
use bmo_telegram::seen::Seen;
use bmo_telegram::send::Sending;
use bmo_telegram::telegram::Telegram;
use bmo_telegram::wire::Wire;
use serde_json::{Value, json};
use support::{OWNER, TOKEN, serve};

const ROOM: i64 = -100;

// A reader whose account is connected or not, keeping every piece of words it is asked to say and numbering
// each from 900.
fn fake_reader(connected: bool, said: Arc<Mutex<Vec<Value>>>) -> Router {
  Router::new()
    .route(
      "/health",
      get(move || async move {
        Json(json!({ "connected": connected, "reason": (!connected).then_some("the account is not connected") }))
      }),
    )
    .route(
      "/owner/say",
      post(move |Json(body): Json<Value>| async move {
        let mut said = said.lock().unwrap();

        said.push(body);
        Json(json!({ "messageId": 899 + said.len() as i64 }))
      }),
    )
}

async fn provider(connected: bool, said: Arc<Mutex<Vec<Value>>>) -> TelegramProvider {
  let port = serve(fake_reader(connected, said)).await;
  let seen = Arc::new(Seen::in_memory());
  let telegram = Arc::new(Telegram::new(
    TOKEN,
    "http://127.0.0.1:9",
    OWNER,
    None,
    seen.clone(),
    Arc::new(FileIds::in_memory()),
  ));
  let reader = ReaderClient::new(Wire::loopback(port, "seam-token"));

  TelegramProvider::new(telegram, Sending::default(), Some(reader), seen, Arc::default())
}

fn as_owner(text: &str) -> ChatSendRequest {
  ChatSendRequest {
    voice: Some(ChatVoice::HumanReader),
    ..ChatSendRequest::new(
      ROOM,
      ChatSend::Text {
        text: text.to_owned(),
        reply_to: Some(5),
        silent: false,
        fold: false,
      },
    )
  }
}

#[tokio::test]
async fn words_are_cut_after_the_last_pause_that_fits_and_a_run_with_no_pause_between_whole_characters() {
  let said = Arc::new(Mutex::default());
  let up = provider(true, said.clone()).await;
  let words = format!("{} {}", "a".repeat(4090), "b".repeat(20));
  let run = "\u{1f600}".repeat(2100);

  assert_eq!(
    up.send(as_owner(&words)).await.unwrap(),
    ChatSendResult::Sent {
      message_id: Some(900),
      missed: Vec::new()
    }
  );
  up.send(as_owner(&run)).await.unwrap();

  let pieces: Vec<Value> = said.lock().unwrap().clone();
  let texts: Vec<&str> = pieces.iter().map(|piece| piece["text"].as_str().unwrap()).collect();

  assert_eq!(
    texts,
    [
      format!("{} ", "a".repeat(4090)),
      "b".repeat(20),
      "\u{1f600}".repeat(2048),
      "\u{1f600}".repeat(52)
    ]
  );
  assert!(pieces.iter().all(|piece| piece["chatId"] == ROOM && piece["reply"].is_null()));

  let down = provider(false, Arc::default()).await;

  assert_eq!(
    down.send(as_owner("hello")).await.unwrap(),
    ChatSendResult::Refused {
      refusal: POLICY.mode_down("human-reader", Some("the account is not connected")),
      gone: false
    }
  );
}

#[test]
fn files_go_as_the_albums_telegram_takes_a_gif_alone_each_kind_up_to_the_ceiling_in_the_order_their_first_file_came()
 {
  let largest = POLICY.owner.largest_bytes;
  let files = [
    ("one.jpg", "image/jpeg", 10),
    ("paper.pdf", "application/pdf", 10),
    ("moving.gif", "image/gif", 10),
    ("clip.mp4", "video/mp4", 10),
    ("song.mp3", "", 10),
    ("three.png", "", 10),
    ("sheet.webp", "image/webp", 10),
    ("empty.png", "image/png", 0),
    ("huge.mov", "video/quicktime", largest + 1),
    ("big.zip", "application/zip", 12 * 1024 * 1024),
  ];
  let grouped: Vec<Vec<&str>> = albums(&files, |(name, mime, _)| kind_of(mime, name), 2)
    .into_iter()
    .map(|album| album.into_iter().map(|(name, _, _)| name).collect())
    .collect();
  let ceiling = megabytes(largest);

  assert_eq!(
    grouped,
    [
      vec!["one.jpg", "clip.mp4"],
      vec!["paper.pdf", "sheet.webp"],
      vec!["moving.gif"],
      vec!["song.mp3"],
      vec!["three.png", "empty.png"],
      vec!["huge.mov"],
      vec!["big.zip"]
    ]
  );
  assert_eq!(ceiling, "50 MB");
  assert_eq!(
    files.iter().map(|(name, _, size)| refusal(name, *size)).collect::<Vec<_>>(),
    [
      None,
      None,
      None,
      None,
      None,
      None,
      None,
      Some(fill(&POLICY.owner.empty_file, &[("name", "empty.png")])),
      Some(fill(
        &POLICY.owner.too_large,
        &[("name", "huge.mov"), ("size", &ceiling), ("largest", &ceiling)]
      )),
      None
    ]
  );
  assert_eq!(
    ["Two lines\r\nhere", "  old\rmac  ", ""].map(|caption| caption_of(Some(caption))),
    ["Two lines\nhere", "old\nmac", ""]
  );
}
