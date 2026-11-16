//! The bot account's files and voices (docs/telegram.md, 「Sends」), sent to a stand-in of the Bot API: what
//! went to it — a file by its name, its type and its bytes' digest — and what each send answered.

use crate::support;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bmo_chat::{ChatSend, ChatSendResult, FileBody};
use bmo_telegram::policy::POLICY;
use bmo_telegram::send::{Sending, sent};
use serde_json::json;
use support::speaking::{BotSends, ROOM, refusal, send, sha, speaker};
use tempfile::tempdir;

// Sounds and a file made for the check: a voice's OGG head, an MP3's, and a document's bytes.
const OGG: &[u8] = &[0x4f, 0x67, 0x67, 0x53, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4];
const MP3: &[u8] = &[0x49, 0x44, 0x33, 4, 0, 0, 0, 0, 0, 0, 9, 9, 9];
const DOCUMENT: &[u8] = b"#1337 inspection notes\n";

// A reading longer than a caption holds, so its first part is the caption and the rest a reply under it.
fn reading() -> String {
  format!("{} {}", "a".repeat(1000), "b".repeat(1500))
}

fn voice(caption: Option<&str>, transcript: Option<String>) -> ChatSend {
  ChatSend::Voice {
    audio: OGG.to_vec().into(),
    said: "today's news".to_owned(),
    caption: caption.map(str::to_owned),
    transcript,
  }
}

#[tokio::test]
async fn a_document_goes_as_a_reply_under_its_own_name() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let document = || ChatSend::Document {
    bytes: DOCUMENT.to_vec().into(),
    file_name: "inspection report.txt".to_owned(),
  };

  assert_eq!(send(&speaker, Some(41), document()).await, Ok(sent(Some(500))));
  assert_eq!(bot.methods(), ["sendDocument"]);
  assert_eq!(
    bot.bodies()[0],
    json!({ "chat_id": ROOM.to_string(), "reply_parameters": { "message_id": 41, "allow_sending_without_reply": true },
      "document": { "file": "inspection report.txt", "type": "application/octet-stream", "sha": sha(DOCUMENT) } })
  );

  bot.answering(|_| None);
  send(&speaker, None, document()).await.unwrap();
  assert!(bot.bodies()[0].get("reply_parameters").is_none());
}

#[tokio::test]
async fn an_audio_file_goes_under_the_type_its_bytes_read_as_and_a_refused_file_is_refused() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let audio = ChatSend::Audio {
    audio: MP3.to_vec().into(),
    file_name: "song.mp3".to_owned(),
  };

  assert_eq!(send(&speaker, None, audio).await, Ok(sent(Some(500))));
  assert_eq!(bot.bodies()[0]["audio"]["type"], "audio/mpeg");

  bot.answering(|asked| {
    (asked.method == "sendDocument").then(|| refusal(400, "Bad Request: chat not found"))
  });

  let rejected = ChatSend::Document {
    bytes: DOCUMENT.to_vec().into(),
    file_name: "a.txt".to_owned(),
  };

  assert_eq!(
    send(&speaker, None, rejected).await,
    Ok(ChatSendResult::Refused(POLICY.refused("Bad Request: chat not found")))
  );
}

// The same two sends with the file kept on this machine's disk, as a send that arrived as a multipart part
// carries it: what goes up is the file, read off the disk, and it is left where it stands.
#[tokio::test]
async fn a_document_kept_on_disk_goes_up_under_the_same_name_type_and_bytes() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let held = tempdir().expect("a temporary folder");
  let path = held.path().join("notes.bin");

  std::fs::write(&path, DOCUMENT).expect("the file is written");

  let document = ChatSend::Document {
    bytes: FileBody::stored(&path),
    file_name: "inspection report.txt".to_owned(),
  };

  assert_eq!(send(&speaker, None, document).await, Ok(sent(Some(500))));
  assert_eq!(
    bot.bodies()[0]["document"],
    json!({ "file": "inspection report.txt", "type": "application/octet-stream", "sha": sha(DOCUMENT) })
  );
  assert!(path.is_file(), "a file the send named is left where it lies");
}

#[tokio::test]
async fn a_sound_kept_on_disk_is_typed_off_its_first_bytes_and_goes_up_as_a_part() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let held = tempdir().expect("a temporary folder");
  let path = held.path().join("song.mp3");

  std::fs::write(&path, MP3).expect("the sound is written");

  let audio = ChatSend::Audio {
    audio: FileBody::stored(&path),
    file_name: "song.mp3".to_owned(),
  };

  assert_eq!(send(&speaker, None, audio).await, Ok(sent(Some(500))));
  assert_eq!(bot.bodies()[0]["audio"]["type"], "audio/mpeg");
  assert_eq!(bot.bodies()[0]["audio"]["sha"], sha(MP3));
}

// A send may name a file whose bytes its sender holds already; the provider has none of them and says so.
#[tokio::test]
async fn a_send_naming_a_file_without_its_bytes_is_refused_naming_the_file() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let document = ChatSend::Document {
    bytes: FileBody::withheld(),
    file_name: "a.txt".to_owned(),
  };

  let refused = send(&speaker, None, document).await.expect_err("a file with no bytes does not go");

  assert!(refused.contains("a.txt"), "the refusal names the file: {refused}");
  assert!(refused.contains("without its bytes"), "the refusal says why: {refused}");
  assert_eq!(bot.methods(), Vec::<String>::new());
}

#[tokio::test]
async fn a_voice_goes_with_the_first_part_of_its_reading_as_its_caption_and_the_rest_quietly_under_it() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;

  assert_eq!(send(&speaker, Some(41), voice(None, Some(reading()))).await, Ok(sent(Some(500))));
  assert_eq!(bot.methods(), ["sendVoice", "sendMessage"]);

  let bodies = bot.bodies();

  assert_eq!(bodies[0]["parse_mode"], "HTML");
  assert_eq!(bodies[0]["voice"]["file"], "voice.ogg");
  assert_eq!(bodies[0]["reply_parameters"]["message_id"], 41);
  assert_eq!(bodies[1]["reply_parameters"], json!({ "message_id": 500 }));
  assert_eq!(bodies[1]["disable_notification"], true);
}

#[tokio::test]
async fn a_caption_telegram_will_not_read_goes_again_bare_and_every_flood_limit_is_waited_out() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let once = Arc::new(AtomicBool::new(false));
  let refused_once = once.clone();

  bot.answering(move |asked| {
    (asked.method == "sendVoice" && !refused_once.swap(true, Ordering::SeqCst))
      .then(|| refusal(400, "Bad Request: can't parse entities: Unsupported start tag"))
  });
  send(&speaker, None, voice(None, Some(reading()))).await.unwrap();
  assert_eq!(bot.methods(), ["sendVoice", "sendVoice", "sendMessage", "sendMessage"]);
  assert!(bot.bodies()[1].get("caption").is_none());

  let flooded = Arc::new(AtomicUsize::new(0));

  bot.answering(move |asked| {
    (asked.method == "sendMessage" && flooded.fetch_add(1, Ordering::SeqCst) < 3).then(|| {
      json!({ "ok": false, "error_code": 429, "description": "Too Many Requests: retry after 0",
        "parameters": { "retry_after": 0 } })
    })
  });
  send(&speaker, None, voice(None, Some(reading()))).await.unwrap();
  assert_eq!(bot.methods(), ["sendVoice", "sendMessage", "sendMessage", "sendMessage", "sendMessage"]);
}

#[tokio::test]
async fn a_caption_of_its_own_is_read_in_the_room_and_a_reading_that_did_not_go_is_the_voice_s_failure() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;

  send(&speaker, None, voice(Some("greeting sample"), None)).await.unwrap();
  assert_eq!(bot.methods(), ["sendVoice"]);
  assert_eq!(bot.bodies()[0]["caption"], "greeting sample");
  assert!(bot.bodies()[0].get("parse_mode").is_none());

  bot.answering(|asked| {
    (asked.method == "sendMessage").then(|| refusal(400, "Bad Request: message text is empty"))
  });

  assert_eq!(
    send(&speaker, None, voice(None, Some(reading()))).await,
    Err(
      "voice 500 was sent; transcript continuation failed: Error: Bad Request: message text is empty"
        .to_owned()
    )
  );
}
