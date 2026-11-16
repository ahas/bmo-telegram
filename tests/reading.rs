//! The bot account's reading of updates of the Bot API's own shape: what each message becomes, the metadata it
//! carries, the album it went out in, and the names and types of what it carries.

use std::sync::Arc;

use bmo_chat::{ChatFileKind, ChatVoice, MessageOrigin, RoomFacts, RoomKind};
use bmo_telegram::albums::{AlbumNames, KEPT};
use bmo_telegram::file_ids::FileIds;
use bmo_telegram::files::audio_type;
use bmo_telegram::gather::{bot_file, media_file_name};
use bmo_telegram::kinds::Message;
use bmo_telegram::policy::{Kinds, POLICY, fill};
use bmo_telegram::reading::{Picture, Reading, metadata, read, read_edit};
use bmo_telegram::seen::Seen;
use bmo_telegram::telegram::{TOKEN_MISSING, Telegram};
use serde_json::{Value, json};

const ME: i64 = 7_000_000_001;
const OWNER: i64 = 42;

fn room(kind: RoomKind, identity: i64, private_to: Option<i64>) -> RoomFacts {
  RoomFacts {
    kind: Some(kind),
    identity: Some(identity),
    private_to,
  }
}

// The placeholders a message of each kind is handed over as, the same words the reading reads.
fn kinds() -> &'static Kinds {
  &POLICY.kinds
}

fn message(json: &str) -> Message {
  serde_json::from_str(json).unwrap()
}

#[test]
fn words_are_handed_over_with_who_said_them_what_they_answer_and_where_they_were_forwarded_from() {
  let json = r#"{
    "message_id": 31, "date": 1760000000,
    "chat": { "id": -1001, "type": "supergroup", "title": "Room" },
    "from": { "id": 55, "is_bot": false, "first_name": "Test", "last_name": "Er", "username": "tester" },
    "reply_to_message": { "message_id": 30, "date": 1759999990, "chat": { "id": -1001, "type": "supergroup" },
      "from": { "id": 66, "is_bot": false, "first_name": "Other" } },
    "forward_origin": { "type": "user", "date": 1, "sender_user": { "id": 77, "is_bot": false, "first_name": "Far" } },
    "text": "hello"
  }"#;

  let Reading::Said(handed) = read(&message(json), ME, OWNER, true) else {
    panic!("words are said");
  };

  assert_eq!(handed.origin, MessageOrigin::Live);
  assert_eq!(handed.sender_id, Some(55));
  assert_eq!(handed.sender_name, "Test Er");
  assert_eq!(handed.sent_at, 1_760_000_000_000);
  assert_eq!(handed.text, format!("{}hello", fill(&kinds().forwarded, &[("from", "Far")])));
  assert_eq!(handed.reply_to_message_id, Some(30));
  assert_eq!(handed.via, "telegram");
  assert_eq!(handed.album, None);
  assert_eq!(handed.mode, ChatVoice::BotAccount);
  assert_eq!(handed.room, room(RoomKind::Group, ME, None));
  assert!(handed.files.is_empty());
  assert!(!handed.gathering);
  assert_eq!(handed.context.room_title.as_deref(), Some("Room"));
  assert_eq!(handed.context.sender_username.as_deref(), Some("tester"));
  assert_eq!(handed.context.reply_to_sender_id, Some(66));
  assert_eq!(handed.context.spoken_text.as_deref(), Some("hello"));
  assert!(!handed.context.carries_file);
}

#[test]
fn words_are_handed_over_from_a_room_nobody_listens_to_and_the_receiver_decides() {
  let json = r#"{ "message_id": 1, "date": 1, "chat": { "id": 99999999999999999, "type": "group" }, "text": "words" }"#;

  assert!(matches!(read(&message(json), ME, OWNER, false), Reading::Said(..)));
}

#[test]
fn a_photo_asks_for_its_largest_size_and_goes_under_its_caption_or_the_placeholder() {
  let json = r#"{
    "message_id": 2, "date": 5, "chat": { "id": 42, "type": "private" },
    "from": { "id": 42, "is_bot": false, "first_name": "Owner" },
    "photo": [ { "file_id": "small", "file_unique_id": "s", "width": 90, "height": 90, "file_size": 100 },
               { "file_id": "large", "file_unique_id": "l", "width": 900, "height": 900, "file_size": 9000 } ]
  }"#;

  let Reading::Picture(handed, picture) = read(&message(json), ME, OWNER, true) else {
    panic!("a photo is a picture");
  };

  assert_eq!(handed.text, POLICY.pictures.mark);
  assert_eq!(
    picture,
    Some(Picture {
      file_id: "large".into(),
      file_unique_id: Some("l".into()),
      file_size: Some(9000)
    })
  );
  assert_eq!(handed.context.spoken_text, None);
  assert!(handed.context.carries_file);

  let captioned = json.replace(r#""photo""#, r#""caption": "look at this", "photo""#);
  let Reading::Picture(handed, _) = read(&message(&captioned), ME, OWNER, true) else {
    panic!("a photo is a picture");
  };

  assert_eq!(handed.text, "look at this");
  assert_eq!(handed.context.spoken_text.as_deref(), Some("look at this"));
}

#[test]
fn nothing_but_words_is_taken_from_a_room_nobody_listens_to() {
  let json = r#"{ "message_id": 2, "date": 5, "chat": { "id": 1, "type": "private" },
    "photo": [ { "file_id": "only", "file_unique_id": "o", "width": 1, "height": 1 } ] }"#;

  assert!(matches!(read(&message(json), ME, OWNER, false), Reading::Nothing));
}

#[test]
fn a_picture_sent_as_a_file_is_a_picture_and_an_svg_is_a_file() {
  let document = |mime: &str, name: &str| {
    format!(
      r#"{{ "message_id": 3, "date": 5, "chat": {{ "id": 42, "type": "private" }},
        "from": {{ "id": 42, "is_bot": false, "first_name": "Owner" }},
        "document": {{ "file_id": "doc", "file_unique_id": "d", "mime_type": "{mime}", "file_name": "{name}", "file_size": 12 }} }}"#
    )
  };

  assert!(matches!(
    read(&message(&document("image/png", "a.png")), ME, OWNER, true),
    Reading::Picture(_, Some(Picture { ref file_id, .. })) if file_id == "doc"
  ));
  assert!(matches!(
    read(&message(&document("application/octet-stream", "b.JPG")), ME, OWNER, true),
    Reading::Picture(..)
  ));

  let Reading::Attachment(handed) = read(&message(&document("image/svg+xml", "c.svg")), ME, OWNER, true)
  else {
    panic!("an svg is a file");
  };

  assert_eq!(handed.text, fill(&kinds().document, &[("name", " c.svg"), ("caption", "")]));
  assert!(handed.context.carries_file);
  assert_eq!(handed.context.spoken_text, None);
}

#[test]
fn a_file_the_bot_sent_itself_is_left_to_the_sender_that_sent_it() {
  let json = format!(
    r#"{{ "message_id": 4, "date": 5, "chat": {{ "id": 42, "type": "private" }},
      "from": {{ "id": {ME}, "is_bot": true, "first_name": "Helper" }},
      "voice": {{ "file_id": "v", "file_unique_id": "v", "duration": 75 }} }}"#
  );

  assert!(matches!(read(&message(&json), ME, OWNER, true), Reading::FileOnly));
}

#[test]
fn a_voice_is_described_with_its_length_and_its_caption() {
  let json = r#"{ "message_id": 4, "date": 5, "chat": { "id": 42, "type": "private" },
    "from": { "id": 9, "is_bot": false, "first_name": "Tester" }, "caption": "listen",
    "voice": { "file_id": "v", "file_unique_id": "v", "duration": 75 } }"#;

  let Reading::Attachment(handed) = read(&message(json), ME, OWNER, true) else {
    panic!("a voice is an attachment");
  };

  assert_eq!(handed.text, fill(&kinds().voice, &[("duration", " 1:15"), ("caption", " listen")]));
  assert_eq!(handed.context.spoken_text.as_deref(), Some("listen"));
}

#[test]
fn the_other_kinds_are_kept_as_placeholders_and_the_bots_own_service_messages_are_nobodys() {
  let cases = [
    (
      r#""location": { "latitude": 37.5, "longitude": 127, "live_period": 60 }"#,
      fill(
        &kinds().location,
        &[("latitude", "37.5"), ("longitude", "127"), ("live", kinds().location_live.as_str())],
      ),
    ),
    (
      r#""sticker": { "file_id": "s", "file_unique_id": "s", "type": "regular", "width": 1, "height": 1, "is_animated": false, "is_video": false, "emoji": "😀", "set_name": "Pack" }"#,
      fill(
        &kinds().sticker,
        &[("emoji", " 😀"), ("set", &fill(&kinds().sticker_set, &[("set", "Pack")]))],
      ),
    ),
    (
      r#""poll": { "id": "p", "question": "lunch?", "options": [ { "text": "rice", "voter_count": 0 }, { "text": "noodles", "voter_count": 0 } ], "total_voter_count": 0, "is_closed": false, "is_anonymous": true, "type": "regular", "allows_multiple_answers": false }"#,
      fill(&kinds().poll, &[("question", "lunch?"), ("options", "rice / noodles")]),
    ),
    (r#""dice": { "emoji": "🎲", "value": 4 }"#, "[🎲 4]".to_owned()),
    (
      r#""new_chat_members": [ { "id": 1, "is_bot": false, "first_name": "A" }, { "id": 2, "is_bot": false, "first_name": "", "username": "bee" } ]"#,
      fill(&kinds().joined, &[("members", "A, bee")]),
    ),
    (
      r#""pinned_message": { "message_id": 9, "date": 0, "chat": { "id": -5, "type": "group" } }"#,
      fill(&kinds().pinned, &[("text", &fill(&kinds().pinned_message, &[("id", "9")]))]),
    ),
    (
      r#""video_chat_ended": { "duration": 3600 }"#,
      fill(&kinds().call_ended, &[("duration", " 60:00")]),
    ),
  ];

  for (kind, expected) in cases {
    let json = format!(
      r#"{{ "message_id": 6, "date": 5, "chat": {{ "id": -5, "type": "group" }}, "from": {{ "id": 9, "is_bot": false, "first_name": "Tester" }}, {kind} }}"#
    );
    let Reading::Said(handed) = read(&message(&json), ME, OWNER, true) else {
      panic!("{kind} is said");
    };

    assert_eq!(handed.text, expected);
  }

  let pinned_by_bot = format!(
    r#"{{ "message_id": 7, "date": 5, "chat": {{ "id": -5, "type": "group" }}, "from": {{ "id": {ME}, "is_bot": true, "first_name": "Helper" }}, "pinned_message": {{ "message_id": 3, "date": 0, "chat": {{ "id": -5, "type": "group" }}, "text": "board" }} }}"#
  );

  assert!(matches!(read(&message(&pinned_by_bot), ME, OWNER, true), Reading::Nothing));
}

#[test]
fn an_edit_carries_the_platforms_own_words_formatting_and_file_in_order() {
  let json = r#"{ "message_id": 8, "date": 5, "edit_date": 6, "chat": { "id": -5, "type": "group" },
    "from": { "id": 9, "is_bot": false, "first_name": "Tester" }, "caption": "edited words",
    "caption_entities": [ { "type": "bold", "offset": 0, "length": 2 } ],
    "document": { "file_id": "d", "file_unique_id": "d", "file_name": "x.txt" } }"#;
  let raw: Value = serde_json::from_str(json).unwrap();
  let edit = read_edit(&message(json), &raw, ME, OWNER);
  let handed = edit.said.unwrap();

  assert_eq!(handed.origin, MessageOrigin::Edit);
  assert_eq!(
    handed.text,
    fill(&kinds().document, &[("name", " x.txt"), ("caption", " edited words")])
  );
  assert!(edit.carries_file);
  assert_eq!(
    serde_json::to_string(&handed.context.metadata.unwrap()).unwrap(),
    r#"{"text":"edited words","entities":[{"type":"bold","offset":0,"length":2}],"sticker":null,"attachment":{"file_id":"d","file_unique_id":"d","file_name":"x.txt"}}"#
  );
}

#[test]
fn the_bots_number_is_read_off_its_token_until_the_platform_says_it() {
  let seen = Arc::new(Seen::in_memory());
  let file_ids = Arc::new(FileIds::in_memory());

  let spool = std::path::PathBuf::from("spool");

  assert_eq!(
    Telegram::new("123456:secret", "", 42, None, seen.clone(), file_ids.clone(), spool.clone()).me(),
    123_456
  );
  assert_eq!(
    Telegram::new("", "", 42, None, seen, file_ids, spool).configured(),
    Some(TOKEN_MISSING.to_owned())
  );
}

#[test]
fn a_message_s_file_is_read_with_the_video_kinds_first() {
  let gif =
    json!({ "animation": { "file_id": "a", "mime_type": "video/mp4" }, "document": { "file_id": "d" } });
  let voice = json!({ "voice": { "file_id": "v", "mime_type": "audio/ogg" } });
  let read = bot_file(&gif).unwrap();

  assert_eq!(
    (read.file_id.as_str(), read.kind, read.file_name.as_deref()),
    ("a", ChatFileKind::Animation, Some("gif.mp4"))
  );
  assert_eq!(media_file_name(Some("audio/ogg; codecs=opus"), true), "voice.ogg");
  assert!(bot_file(&voice).unwrap().voice);
  assert!(bot_file(&json!({ "text": "words" })).is_none());
}

#[test]
fn the_metadata_keeps_the_order_telegram_wrote() {
  let message: Value = serde_json::from_str(
    r#"{"caption":"a\/b","caption_entities":[{"type":"bold","offset":0,"length":1}],"document":{"z":1,"a":2}}"#,
  )
  .unwrap();

  assert_eq!(
    serde_json::to_string(&metadata(&message).unwrap()).unwrap(),
    r#"{"text":"a/b","entities":[{"type":"bold","offset":0,"length":1}],"sticker":null,"attachment":{"z":1,"a":2}}"#
  );
}

#[test]
fn every_message_of_a_group_is_named_by_the_first_read() {
  let mut albums = AlbumNames::default();

  assert_eq!(albums.name(-1, "g", 10), 10);
  assert_eq!(albums.name(-1, "g", 11), 10);
  assert_eq!(albums.name(-2, "g", 12), 12);
}

#[test]
fn a_group_read_long_ago_is_forgotten() {
  let mut albums = AlbumNames::default();

  albums.name(-1, "old", 1);

  for at in 0..KEPT as i64 {
    albums.name(-1, &at.to_string(), 100 + at);
  }

  assert_eq!(albums.name(-1, "old", 500), 500);
}

#[test]
fn a_sound_is_read_by_its_first_bytes() {
  assert_eq!(audio_type(b"RIFF\x24\x08\x00\x00WAVEfmt "), ("wav", "audio/wav"));
  assert_eq!(audio_type(b"ID3\x04"), ("mp3", "audio/mpeg"));
  assert_eq!(audio_type(&[0xff, 0xfb, 0x90]), ("mp3", "audio/mpeg"));
  assert_eq!(audio_type(b"\x00\x00\x00\x20ftypM4A "), ("m4a", "audio/mp4"));
  assert_eq!(audio_type(b"fLaC"), ("flac", "audio/flac"));
  assert_eq!(audio_type(b"OggS"), ("ogg", "audio/ogg"));
}
