//! A send in the owner's own voice (docs/telegram.md, 「The owner's voice」): the words and files the bot's send
//! would carry, said through a fake reader and never by the bot, a send in no voice still the bot's, the echo
//! of the owner's send as the bot account reads it not handed over again, the refusal while the reader has no
//! account connected, and files gathered into the albums Telegram takes by their kinds, each file's size
//! judged and a caption's line breaks read as a form carries them.

mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::response::Json;
use axum::routing::{get, post};
use bmo_chat::{ChatProvider, ChatSend, ChatSendRequest, ChatSendResult, ChatSent, ChatVoice};
use bmo_telegram::compose::text_parts;
use bmo_telegram::echoes::{Echoes, numbered_for_everyone};
use bmo_telegram::file_ids::FileIds;
use bmo_telegram::owner::{albums, caption_of, kind_of, megabytes, refusal};
use bmo_telegram::policy::{POLICY, fill};
use bmo_telegram::provider::{NO_READER, TelegramProvider};
use bmo_telegram::reader::ReaderClient;
use bmo_telegram::seen::Seen;
use bmo_telegram::send::Sending;
use bmo_telegram::telegram::Telegram;
use bmo_telegram::wire::Wire;
use serde_json::{Value, json};
use support::speaking::{BotSends, bot_sends};
use support::{KeptReceiver, OWNER, TOKEN, serve};

// A supergroup, which numbers its messages for everyone, and a group, in which each account numbers its own.
const SUPERGROUP: i64 = -1_001_234_567;
const GROUP: i64 = -4_242;

// What the fake reader was asked, by its route.
type Asked = Arc<Mutex<Vec<(String, Value)>>>;

// A reader whose account is connected or not, keeping everything it is asked to send, numbering each message
// from 900 and holding each one's plain words as Telegram would.
fn fake_reader(connected: bool, asked: Asked) -> Router {
  let album_asked = asked.clone();

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
        let mut asked = asked.lock().unwrap();

        asked.push(("say".to_owned(), body.clone()));
        Json(json!({ "messageId": 899 + asked.len() as i64, "text": body["text"] }))
      }),
    )
    .route(
      "/owner/album",
      post(move |Json(body): Json<Value>| async move {
        let mut asked = album_asked.lock().unwrap();
        let count = body["files"].as_array().map_or(0, Vec::len);

        asked.push(("album".to_owned(), body.clone()));

        let first = 899 + asked.len() as i64 * 10;
        let ids: Vec<i64> = (0..count as i64).map(|at| first + at).collect();
        let texts: Vec<&str> =
          (0..count).map(|at| if at == 0 { body["caption"].as_str().unwrap_or_default() } else { "" }).collect();

        Json(json!({ "ids": ids, "texts": texts }))
      }),
    )
}

// The provider over a Bot API stand-in and, where one is asked for, a fake reader: the provider, the bot
// account it speaks and reads as, the stand-in, and what the reader was asked.
struct Standing {
  provider: TelegramProvider,
  telegram: Arc<Telegram>,
  bot: Arc<BotSends>,
  asked: Asked,
}

async fn standing(reader: Option<bool>) -> Standing {
  let asked: Asked = Arc::default();
  let bot = BotSends::new();
  let seen = Arc::new(Seen::in_memory());
  let telegram = Arc::new(Telegram::new(
    TOKEN,
    &bot_sends(bot.clone()).await,
    OWNER,
    None,
    seen.clone(),
    Arc::new(FileIds::in_memory()),
  ));
  let reader = match reader {
    Some(connected) => {
      let port = serve(fake_reader(connected, asked.clone())).await;

      Some(ReaderClient::new(Wire::loopback(port, "seam-token")))
    }
    None => None,
  };
  let provider = TelegramProvider::new(
    telegram.clone(),
    Sending::default(),
    reader,
    seen,
    Arc::default(),
    std::path::PathBuf::from("avatars"),
  );

  Standing {
    provider,
    telegram,
    bot,
    asked,
  }
}

fn text(room: i64, mode: Option<ChatVoice>, words: &str) -> ChatSendRequest {
  ChatSendRequest {
    mode,
    reply_to_message_id: Some(5),
    ..ChatSendRequest::new(
      room,
      ChatSend::Text {
        text: words.to_owned(),
        silent: false,
        fold: false,
      },
    )
  }
}

fn as_owner(room: i64, words: &str) -> ChatSendRequest {
  text(room, Some(ChatVoice::HumanReader), words)
}

// A message the bot account reads in a room, from the owner's number.
fn from_owner(room: i64, message_id: i64, words: &str) -> Value {
  json!({ "message_id": message_id, "date": 1_700_000_000, "chat": { "id": room, "type": "group" },
    "from": { "id": OWNER, "is_bot": false, "first_name": "Owner" }, "text": words })
}

fn handed_ids(receiver: &KeptReceiver) -> Vec<i64> {
  receiver
    .handed
    .lock()
    .unwrap()
    .iter()
    .filter_map(|(_, body)| body["messageId"].as_i64())
    .collect()
}

#[tokio::test]
async fn the_owner_s_words_go_through_the_reader_in_the_parts_and_markup_the_bot_s_send_takes_never_by_the_bot()
 {
  let up = standing(Some(true)).await;
  let words = format!("**Bold** words\n\n{}", "a ".repeat(2100));
  let parts = text_parts(&words, false);

  assert!(parts.len() > 1, "the words take several parts");
  assert_eq!(
    up.provider.send(as_owner(SUPERGROUP, &words)).await.unwrap(),
    ChatSendResult::Sent(ChatSent {
      message_id: Some(900),
      missed: Vec::new()
    })
  );
  assert!(up.bot.asked().is_empty(), "the bot sent nothing: {:?}", up.bot.methods());

  let asked = up.asked.lock().unwrap().clone();

  assert_eq!(asked.len(), parts.len());

  for (at, ((route, body), part)) in asked.iter().zip(&parts).enumerate() {
    assert_eq!(route, "say");
    assert_eq!(body["chatId"], SUPERGROUP);
    assert_eq!(body["text"], part.plain);
    assert_eq!(body["markup"], json!(part.markup));
    assert_eq!(body["silent"], false);
    assert_eq!(body["replyToMessageId"], if at == 0 { json!(5) } else { Value::Null });
  }

  assert_eq!(
    asked[0].1["markup"].as_str().map(|markup| markup.starts_with("<b>Bold</b>")),
    Some(true)
  );
}

#[tokio::test]
async fn the_owner_s_files_go_through_the_reader_a_document_kept_whole_whatever_its_name() {
  let up = standing(Some(true)).await;
  let document = ChatSendRequest {
    mode: Some(ChatVoice::HumanReader),
    ..ChatSendRequest::new(
      GROUP,
      ChatSend::Document {
        bytes: vec![1, 2, 3],
        file_name: "scan.jpg".to_owned(),
      },
    )
  };

  assert_eq!(
    up.provider.send(document).await.unwrap(),
    ChatSendResult::Sent(ChatSent {
      message_id: Some(909),
      missed: Vec::new()
    })
  );
  assert!(up.bot.asked().is_empty());

  let asked = up.asked.lock().unwrap().clone();

  assert_eq!(asked[0].0, "album");
  assert_eq!(
    asked[0].1["files"],
    json!([{ "name": "scan.jpg", "mime": "application/octet-stream", "bytes": "AQID" }])
  );
  assert_eq!(asked[0].1["caption"], "");
  assert_eq!(asked[0].1["captionMarkup"], Value::Null);

  let sticker = ChatSendRequest {
    mode: Some(ChatVoice::HumanReader),
    ..ChatSendRequest::new(
      GROUP,
      ChatSend::Sticker {
        emoji: "x".to_owned(),
      },
    )
  };

  assert_eq!(
    up.provider.send(sticker).await.unwrap(),
    ChatSendResult::Refused(POLICY.owner_verb("sticker"))
  );
}

#[tokio::test]
async fn a_send_in_no_voice_or_the_bot_s_goes_by_the_bot_and_never_through_the_reader() {
  let up = standing(Some(true)).await;

  for mode in [None, Some(ChatVoice::BotAccount)] {
    up.bot.answering(|_| None);

    assert_eq!(
      up.provider.send(text(SUPERGROUP, mode, "hello")).await.unwrap(),
      ChatSendResult::Sent(ChatSent {
        message_id: Some(500),
        missed: Vec::new()
      })
    );
    assert_eq!(up.bot.methods(), ["sendMessage"]);
  }

  assert!(up.asked.lock().unwrap().is_empty(), "the reader was asked nothing");
}

#[tokio::test]
async fn the_owner_s_send_is_refused_while_the_reader_has_no_account_and_never_goes_by_the_bot() {
  let down = standing(Some(false)).await;

  assert_eq!(
    down.provider.send(as_owner(SUPERGROUP, "hello")).await.unwrap(),
    ChatSendResult::Refused(POLICY.mode_down("human-reader", Some("the account is not connected")))
  );
  assert!(down.bot.asked().is_empty());
  assert!(down.asked.lock().unwrap().is_empty());

  let none = standing(None).await;

  assert_eq!(
    none.provider.send(as_owner(SUPERGROUP, "hello")).await.unwrap(),
    ChatSendResult::Refused(POLICY.mode_down("human-reader", Some(NO_READER)))
  );
  assert!(none.bot.asked().is_empty());
}

#[tokio::test]
async fn the_echo_of_the_owner_s_send_is_not_handed_over_again_by_number_or_by_words_where_numbers_differ()
{
  let up = standing(Some(true)).await;
  let (receiver, _heard) = KeptReceiver::new();

  assert!(numbered_for_everyone(SUPERGROUP));
  assert!(!numbered_for_everyone(GROUP));
  assert!(!numbered_for_everyone(OWNER));

  // A supergroup: the bot reads the message under the number the owner's account gave it.
  up.provider.send(as_owner(SUPERGROUP, "hello")).await.unwrap();
  up.telegram
    .message(receiver.clone(), from_owner(SUPERGROUP, 900, "hello"))
    .await
    .unwrap();
  // The same words typed again on the phone are the owner's own, and go.
  up.telegram
    .message(receiver.clone(), from_owner(SUPERGROUP, 950, "hello"))
    .await
    .unwrap();

  // A group: the bot numbers the message its own way, and the words find it.
  up.provider.send(as_owner(GROUP, "see you")).await.unwrap();
  up.telegram
    .message(receiver.clone(), from_owner(GROUP, 7_000, "something else"))
    .await
    .unwrap();
  up.telegram
    .message(receiver.clone(), from_owner(GROUP, 7_001, "see you"))
    .await
    .unwrap();
  up.telegram
    .message(receiver.clone(), from_owner(GROUP, 7_002, "see you"))
    .await
    .unwrap();

  assert_eq!(handed_ids(&receiver), [950, 7_000, 7_002]);
}

#[tokio::test]
async fn a_message_from_the_owner_waits_for_a_send_in_the_owner_s_voice_still_going_into_its_room() {
  let echoes = Arc::new(Echoes::default());
  let going = echoes.going(GROUP);
  let waiting = {
    let echoes = echoes.clone();

    tokio::spawn(async move { echoes.echo(GROUP, 7_001, "see you").await })
  };

  tokio::time::sleep(Duration::from_millis(50)).await;
  assert!(!waiting.is_finished(), "the echo waits for the send");

  echoes.sent(GROUP, Some(31), "see you");
  drop(going);

  assert!(waiting.await.unwrap());
  // With no send going, a message from the owner nobody noted is the owner's own at once.
  assert!(!echoes.echo(GROUP, 7_002, "see you").await);
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
