//! What the bot account hands the receiver of what a message carries (docs/telegram.md, 「Reading a room」):
//! its expressions, its file and its picture, gathered from a stand-in of the Bot API, past its ceiling through
//! a stand-in of the reader, and the long poll read through to the hand-overs it makes.

mod support;

use std::io::Cursor;
use std::sync::Arc;

use axum::Router;
use axum::response::Json;
use axum::routing::post;
use bmo_chat::{ChatReceiver, MessageFile};
use bmo_telegram::file_ids::FileIds;
use bmo_telegram::policy::POLICY;
use bmo_telegram::reader::ReaderClient;
use bmo_telegram::seen::Seen;
use bmo_telegram::telegram::Telegram;
use bmo_telegram::wire::Wire;
use image::{DynamicImage, ImageFormat, RgbImage, RgbaImage};
use serde_json::{Value, json};
use support::{BotApiStandIn, KeptReceiver, OWNER, TOKEN, bot_api, file_of, next, serve};

const ROOM: i64 = -100;
const SENT_AT: i64 = 1_700_000_000;

fn message(carries: Value) -> Value {
  let mut message = json!({ "message_id": 40, "date": SENT_AT, "chat": { "id": ROOM, "type": "supergroup", "title": "Room" },
    "from": { "id": 55, "is_bot": false, "first_name": "Test", "last_name": "Er" } });

  message.as_object_mut().unwrap().extend(carries.as_object().unwrap().clone());

  message
}

fn png(picture: &DynamicImage) -> Vec<u8> {
  let mut out = Cursor::new(Vec::new());

  picture.write_to(&mut out, ImageFormat::Png).unwrap();
  out.into_inner()
}

fn webp(picture: &DynamicImage) -> Vec<u8> {
  let mut out = Cursor::new(Vec::new());

  picture.write_to(&mut out, ImageFormat::WebP).unwrap();
  out.into_inner()
}

fn half_clear(width: u32, height: u32) -> DynamicImage {
  DynamicImage::ImageRgba8(RgbaImage::from_pixel(width, height, image::Rgba([30, 140, 60, 128])))
}

struct Account {
  telegram: Telegram,
  receiver: Arc<KeptReceiver>,
  heard: tokio::sync::mpsc::UnboundedReceiver<(String, Value)>,
  api: Arc<BotApiStandIn>,
}

impl Account {
  fn receiver(&self) -> Arc<dyn ChatReceiver> {
    self.receiver.clone()
  }
}

// The bot account over a stand-in of the Bot API, with a reader where one is given.
async fn account(reader: Option<ReaderClient>) -> Account {
  let api = Arc::new(BotApiStandIn::default());
  let address = bot_api(api.clone()).await;
  let (receiver, heard) = KeptReceiver::new();

  Account {
    telegram: Telegram::new(
      TOKEN,
      &address,
      OWNER,
      reader,
      Arc::new(Seen::in_memory()),
      Arc::new(FileIds::in_memory()),
    ),
    receiver,
    heard,
    api,
  }
}

// A reader whose owner's account hands over the bytes it is asked for.
async fn reader_handing(bytes: &'static [u8]) -> ReaderClient {
  let fetched = |name: &str| {
    let mut answer = serde_json::to_value(MessageFile {
      id: "document-9".to_owned(),
      name: name.to_owned(),
      mime: "application/octet-stream".to_owned(),
      bytes: bytes.to_vec(),
      kind: None,
    })
    .unwrap();

    answer["outcome"] = json!("fetched");
    Json(answer)
  };
  let router = Router::new()
    .route("/reader/file", post(move || async move { fetched("big.zip") }))
    .route("/reader/picture", post(move || async move { fetched("photo") }));

  ReaderClient::new(Wire::loopback(serve(router).await, "seam-token"))
}

#[tokio::test]
async fn expressions_are_handed_over_as_telegram_wrote_them_with_their_assets_and_an_edit_keeps_its_assets_alone()
 {
  let mut account = account(None).await;
  let said = message(json!({
    "text": "well done 🎉",
    "entities": [{ "type": "custom_emoji", "offset": 10, "length": 2, "custom_emoji_id": "e-1" }, { "type": "bold", "offset": 0, "length": 1 }],
    "sticker": { "file_id": "sticker-file", "file_unique_id": "sticker-unique", "type": "regular", "width": 512, "height": 512,
      "is_animated": false, "is_video": true, "file_size": 10, "emoji": "😀" }
  }));

  account.telegram.message(account.receiver(), said.clone()).await.unwrap();

  let (route, handed) = next(&mut account.heard).await;

  assert_eq!(route, "message");
  assert_eq!(handed["gathering"], true);
  assert_eq!(handed["context"]["metadata"]["text"], "well done 🎉");
  assert_eq!(handed["context"]["metadata"]["sticker"]["file_id"], "sticker-file");
  assert_eq!(handed["context"]["metadata"]["attachment"], Value::Null);

  let (route, gathered) = next(&mut account.heard).await;
  let mut names: Vec<(String, String)> = gathered["files"]
    .as_array()
    .unwrap()
    .iter()
    .map(|file| {
      let file = file_of(file);
      (file.name, file.id)
    })
    .collect();

  names.sort();
  assert_eq!(route, "gathered");
  assert_eq!(account.api.methods(), ["getFile", "getCustomEmojiStickers", "getFile"]);
  assert_eq!(names[0], ("emoji-e-1.webp".to_owned(), "emoji-unique-e-1".to_owned()));
  assert!(names[1].0.starts_with("sticker-") && names[1].0.ends_with(".webm"), "{names:?}");
  assert_eq!(names[1].1, "sticker-unique");

  account.api.forget();
  account.telegram.edited(account.receiver(), said).await.unwrap();

  let (route, edited) = next(&mut account.heard).await;

  assert_eq!((route.as_str(), &edited["files"]), ("message", &json!([])));
  assert_eq!(edited["origin"], "edit");
  assert_eq!(account.api.methods(), ["getCustomEmojiStickers"]);
}

#[tokio::test]
async fn a_document_is_fetched_through_the_bot_api_and_handed_over_under_its_own_name() {
  let mut account = account(None).await;
  let document = message(
    json!({ "document": { "file_id": "doc-1", "file_unique_id": "doc-unique", "file_name": "report v1.pdf",
    "mime_type": "application/pdf", "file_size": 30 } }),
  );

  account.telegram.message(account.receiver(), document).await.unwrap();

  let (route, attachment) = next(&mut account.heard).await;

  assert_eq!(route, "message");
  assert_eq!(attachment["senderName"], "Test Er");
  assert_eq!(attachment["gathering"], true);

  let (_, gathered) = next(&mut account.heard).await;
  let file = file_of(&gathered["files"][0]);

  assert_eq!(account.api.methods(), ["getFile"]);
  assert!(gathered.get("gatherFailure").is_none_or(Value::is_null));
  assert_eq!((file.name.as_str(), file.mime.as_str()), ("report v1.pdf", "application/pdf"));
  assert_eq!(file.id, "doc-unique");
  assert_eq!(account.telegram.file_id_of(ROOM, 40).as_deref(), Some("doc-unique"));
  assert_eq!(file.bytes, b"bytes of files/doc-1");
}

#[tokio::test]
async fn a_long_name_crosses_whole_and_the_receiver_names_what_it_keeps() {
  let mut account = account(None).await;
  let name = format!("{}.txt", "a long name ".repeat(20));
  let document = message(
    json!({ "document": { "file_id": "long-1", "file_unique_id": "long-unique", "file_name": name, "file_size": 30 } }),
  );

  account.telegram.message(account.receiver(), document).await.unwrap();
  next(&mut account.heard).await;

  let (_, gathered) = next(&mut account.heard).await;

  assert_eq!(file_of(&gathered["files"][0]).name, name);
}

#[tokio::test]
async fn a_spoken_note_and_a_gif_are_named_for_what_they_are_and_one_telegram_names_no_unique_id_by_its_place()
 {
  let mut account = account(None).await;
  let voice = message(json!({ "voice": { "file_id": "voice-1", "mime_type": "audio/ogg",
    "duration": 2, "file_size": 20 } }));
  let gif = message(
    json!({ "message_id": 41, "animation": { "file_id": "gif-1", "file_unique_id": "gif-unique", "mime_type": "video/mp4",
    "width": 1, "height": 1, "duration": 1 }, "document": { "file_id": "gif-1", "file_unique_id": "gif-unique", "mime_type": "video/mp4" } }),
  );
  let mut named = Vec::new();

  for carried in [voice, gif] {
    account.telegram.message(account.receiver(), carried).await.unwrap();
    next(&mut account.heard).await;

    let (_, gathered) = next(&mut account.heard).await;
    let file = file_of(&gathered["files"][0]);

    named.push((file.name, file.mime, file.id));
  }

  assert_eq!(
    named,
    [
      ("voice.ogg".to_owned(), "audio/ogg".to_owned(), "file".to_owned()),
      ("gif.mp4".to_owned(), "video/mp4".to_owned(), "gif-unique".to_owned())
    ]
  );
}

#[tokio::test]
async fn a_file_past_the_bot_api_s_ceiling_is_asked_of_the_reader_and_with_none_standing_is_not_kept() {
  let big = |id: &str| {
    message(
      json!({ "document": { "file_id": id, "file_unique_id": id, "file_name": "big.zip", "file_size": 20_000_001 } }),
    )
  };
  let mut alone = account(None).await;

  alone.telegram.message(alone.receiver(), big("big-1")).await.unwrap();
  next(&mut alone.heard).await;

  let (_, gathered) = next(&mut alone.heard).await;

  assert_eq!(alone.api.methods(), Vec::<String>::new());
  assert_eq!(gathered["files"], json!([]));
  assert_eq!(gathered["gatherFailure"], POLICY.gathering.file_not_kept.as_str());

  let mut read = account(Some(reader_handing(b"read by the owner's account").await)).await;

  read.telegram.message(read.receiver(), big("big-2")).await.unwrap();
  next(&mut read.heard).await;

  let (_, gathered) = next(&mut read.heard).await;

  let file = file_of(&gathered["files"][0]);

  assert_eq!(read.api.methods(), Vec::<String>::new());
  assert_eq!(
    (file.id.as_str(), file.bytes.as_slice()),
    ("big-2", b"read by the owner's account".as_slice())
  );
}

#[tokio::test]
async fn a_file_gathered_already_is_left_as_it_stands_and_one_changed_under_its_name_is_fetched_again() {
  let mut account = account(None).await;
  let edit = |unique: &str| {
    message(
      json!({ "edit_date": SENT_AT + 1, "caption": "the file", "document": { "file_id": "doc-1", "file_unique_id": unique,
      "file_name": "a.txt", "file_size": 30 } }),
    )
  };

  account.telegram.edited(account.receiver(), edit("doc-unique")).await.unwrap();
  assert_eq!(file_of(&next(&mut account.heard).await.1["files"][0]).name, "a.txt");

  account.api.forget();
  account.telegram.edited(account.receiver(), edit("doc-unique")).await.unwrap();
  assert_eq!(next(&mut account.heard).await.1["files"], json!([]));
  assert_eq!(account.api.methods(), Vec::<String>::new());

  account.telegram.edited(account.receiver(), edit("newer-unique")).await.unwrap();
  next(&mut account.heard).await;
  assert_eq!(account.api.methods(), ["getFile"]);
}

#[tokio::test]
async fn a_picture_is_fetched_through_the_bot_api_and_handed_over_with_its_message() {
  let mut account = account(None).await;

  account.api.files.lock().unwrap().insert("photo-1".to_owned(), png(&half_clear(6, 4)));
  account
    .telegram
    .message(account.receiver(), message(json!({ "photo": [{ "file_id": "photo-1", "file_unique_id": "photo-unique", "width": 6, "height": 4, "file_size": 90 }] })))
    .await
    .unwrap();

  let (route, handed) = next(&mut account.heard).await;
  let picture = file_of(&handed["files"][0]);

  assert_eq!(account.api.methods(), ["getFile"]);
  assert_eq!(route, "message");
  assert_eq!(handed["context"]["carriesFile"], true);
  assert_eq!(picture.kind, Some(bmo_chat::ChatFileKind::Photo));
  assert_eq!((picture.name.as_str(), picture.mime.as_str()), ("40.jpg", "image/jpeg"));
  assert_eq!(picture.id, "photo-unique");
  assert_eq!(account.telegram.file_id_of(ROOM, 40).as_deref(), Some("photo-unique"));
}

#[tokio::test]
async fn every_kind_of_picture_is_kept_as_a_picture_is_kept_a_large_one_brought_inside_its_largest_side() {
  let mut account = account(None).await;
  let solid = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(6, 4, image::Rgb([30, 140, 60]))));
  let kinds = [
    ("photo-solid", solid.clone()),
    ("photo-webp", webp(&half_clear(6, 4))),
    ("photo-large", png(&half_clear(1700, 1700))),
  ];
  let mut kept = Vec::new();

  for (id, bytes) in &kinds {
    account.api.files.lock().unwrap().insert((*id).to_owned(), bytes.clone());
    account
      .telegram
      .message(account.receiver(), message(json!({ "photo": [{ "file_id": id, "file_unique_id": id, "width": 6, "height": 4, "file_size": bytes.len() }] })))
      .await
      .unwrap();

    let picture = file_of(&next(&mut account.heard).await.1["files"][0]);
    let read = image::load_from_memory(&picture.bytes).unwrap();

    kept.push((picture.mime, read.width(), read.height(), picture.bytes == *bytes));
  }

  assert_eq!(
    kept,
    [
      ("image/png".to_owned(), 6, 4, true),
      ("image/jpeg".to_owned(), 6, 4, false),
      (
        "image/jpeg".to_owned(),
        POLICY.pictures.kept_max_dimension,
        POLICY.pictures.kept_max_dimension,
        false
      ),
    ]
  );
}

#[tokio::test]
async fn a_picture_past_the_bot_api_s_ceiling_is_asked_of_the_reader_and_with_none_standing_goes_without_its_bytes()
 {
  let mut account = account(None).await;

  account
    .telegram
    .message(account.receiver(), message(json!({ "photo": [{ "file_id": "photo-big", "file_unique_id": "big", "width": 6, "height": 4, "file_size": 20_000_001 }] })))
    .await
    .unwrap();

  let (route, handed) = next(&mut account.heard).await;

  assert_eq!(account.api.methods(), Vec::<String>::new());
  assert_eq!((route.as_str(), &handed["files"]), ("message", &json!([])));
  assert_eq!(handed["context"].get("carriesFile"), None);
}

#[tokio::test]
async fn the_long_poll_reads_an_update_and_hands_its_words_and_its_picture_to_the_receiver() {
  let mut account = account(None).await;
  let from = json!({ "id": OWNER, "is_bot": false, "first_name": "Tester" });

  account.api.files.lock().unwrap().insert("p".to_owned(), png(&half_clear(1, 1)));
  *account.api.updates.lock().unwrap() = Some(json!([
    { "update_id": 500, "message": { "message_id": 3, "date": 1_760_000_000, "chat": { "id": -110, "type": "group", "title": "Room" },
      "from": from, "text": "hello there" } },
    { "update_id": 501, "message": { "message_id": 4, "date": 1_760_000_001, "chat": { "id": -110, "type": "group", "title": "Room" },
      "from": from, "photo": [ { "file_id": "p", "file_unique_id": "p", "width": 1, "height": 1, "file_size": 10 } ] } }
  ]));

  let telegram = Arc::new(account.telegram);
  let reading = tokio::spawn(telegram.clone().read(account.receiver.clone()));
  let (_, words) = next(&mut account.heard).await;
  let (_, picture) = next(&mut account.heard).await;

  reading.abort();

  assert_eq!((words["messageId"].clone(), words["via"].clone()), (json!(3), json!("telegram")));
  assert_eq!(picture["messageId"], 4);
  assert_eq!(file_of(&picture["files"][0]).name, "4.jpg");
  assert!(account.api.methods().contains(&"getFile".to_owned()));
  assert_eq!(telegram.me(), 7_000_000_001);
  assert_eq!(telegram.standing(), None);
}

#[tokio::test]
async fn a_group_the_bot_leaves_or_that_moves_hands_over_the_leaving_and_reads_on() {
  let mut account = account(None).await;
  let (room, moved, after) = (-150_i64, -1_000_000_150_i64, -151_i64);
  let from = json!({ "id": OWNER, "is_bot": false, "first_name": "Tester" });
  let group = json!({ "id": room, "type": "group", "title": "Group" });
  let bot =
    json!({ "id": 7_000_000_001_i64, "is_bot": true, "first_name": "Helper", "username": "helper_bot" });

  // The bot taken out of the group, the platform's own account of that, and the group moved to a supergroup
  // under a number of its own; then a word in another room.
  *account.api.updates.lock().unwrap() = Some(json!([
    { "update_id": 600, "message": { "message_id": 2, "date": 1_760_000_002, "chat": group, "from": from, "left_chat_member": bot } },
    { "update_id": 601, "my_chat_member": { "chat": group, "from": from, "date": 1_760_000_003,
      "old_chat_member": { "status": "member", "user": bot }, "new_chat_member": { "status": "kicked", "user": bot, "until_date": 0 } } },
    { "update_id": 602, "message": { "message_id": 3, "date": 1_760_000_004, "chat": group, "from": from, "migrate_to_chat_id": moved } },
    { "update_id": 603, "message": { "message_id": 1, "date": 1_760_000_005,
      "chat": { "id": moved, "type": "supergroup", "title": "Group" }, "from": from, "migrate_from_chat_id": room } },
    { "update_id": 604, "message": { "message_id": 1, "date": 1_760_000_006, "chat": { "id": after, "type": "group", "title": "Other" },
      "from": from, "text": "are you there" } }
  ]));

  let reading = tokio::spawn(Arc::new(account.telegram).read(account.receiver.clone()));
  let (_, left) = next(&mut account.heard).await;
  let (_, word) = next(&mut account.heard).await;

  reading.abort();

  assert_eq!((left["roomId"].clone(), left["text"].clone()), (json!(room), json!("[Left] Helper")));
  assert_eq!((word["roomId"].clone(), word["text"].clone()), (json!(after), json!("are you there")));
  assert_eq!(account.receiver.handed.lock().unwrap().len(), 2);
}
