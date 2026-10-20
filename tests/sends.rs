//! What the sends read off what they are handed and what Telegram answers, before anything goes: a send's verb
//! off the wire, a message Telegram no longer has, a caption it will not take, a flood limit's wait, how
//! pictures split into albums, and which picture may go at all.

use std::time::Duration;

use bmo_chat::{ChatSend, ChatSendRequest, MissedPictureReason};
use bmo_telegram::acts::is_gone;
use bmo_telegram::bot_api::Answered;
use bmo_telegram::files::caption_refused;
use bmo_telegram::pictures::{flood_wait, groups, is_kept, unsendable};
use serde_json::json;

#[test]
fn a_message_telegram_no_longer_has_is_read_as_gone() {
  assert!(is_gone("Bad Request: message to edit not found"));
  assert!(is_gone("Bad Request: MESSAGE_ID_INVALID"));
  assert!(is_gone("Bad Request: message can't be deleted for everyone"));
  assert!(!is_gone("Bad Request: chat not found"));
}

#[test]
fn each_verb_reads_as_the_chat_protocol_hands_it_over() {
  let pin: ChatSendRequest =
    serde_json::from_value(json!({ "roomId": 7, "kind": "pin", "fields": { "messageId": 9, "on": true } }))
      .unwrap();

  assert_eq!(pin.room_id, 7);
  assert_eq!(
    pin.send,
    ChatSend::Pin {
      message_id: 9,
      on: true,
      silent: false
    }
  );
  assert!(serde_json::from_value::<ChatSendRequest>(json!({ "roomId": 7, "kind": "poll" })).is_err());
}

#[test]
fn a_refused_caption_is_read_off_telegram_s_description() {
  assert!(caption_refused("Bad Request: can't parse entities: Unsupported start tag"));
  assert!(caption_refused("Bad Request: message caption is too long"));
  assert!(!caption_refused("Bad Request: chat not found"));
}

#[test]
fn an_album_the_flood_limit_refused_waits_as_asked_but_never_past_a_minute() {
  let answered = |text: &str| serde_json::from_str::<Answered>(text).expect("an answer");

  assert_eq!(
    flood_wait(&answered(r#"{"ok":false,"error_code":429,"parameters":{"retry_after":7}}"#)),
    Some(Duration::from_secs(7))
  );
  assert_eq!(
    flood_wait(&answered(r#"{"ok":false,"error_code":429,"parameters":{"retry_after":600}}"#)),
    None
  );
  assert_eq!(flood_wait(&answered(r#"{"ok":false,"error_code":400,"description":"bad"}"#)), None);
}

#[test]
fn pictures_split_evenly_into_albums_none_of_one() {
  let addresses: Vec<String> = (0..11).map(|at| at.to_string()).collect();
  let sizes: Vec<usize> = groups(&addresses).iter().map(|group| group.len()).collect();

  assert_eq!(sizes, vec![6, 5]);
  assert_eq!(groups(&addresses[..10]).len(), 1);
}

#[test]
fn only_a_web_address_or_a_file_under_the_picture_root_may_go() {
  let root = std::env::temp_dir().join(format!("telegram-pictures-{}", std::process::id()));
  let kept = root.join("picture.png");

  std::fs::create_dir_all(&root).unwrap();
  std::fs::write(&kept, b"x").unwrap();

  let kept_path = kept.to_str().unwrap();

  assert_eq!(unsendable(Some(&root), "https://example.com/a.png"), None);
  assert_eq!(unsendable(Some(&root), kept_path), None);
  assert_eq!(unsendable(Some(&root), "a.png"), Some(MissedPictureReason::NotAnAddress));
  assert_eq!(
    unsendable(Some(&root.join("sub")), kept_path),
    Some(MissedPictureReason::OutsideDataRoot)
  );
  assert_eq!(unsendable(None, kept_path), Some(MissedPictureReason::OutsideDataRoot));
  assert_eq!(unsendable(Some(&root), "/nowhere/at/all.png"), Some(MissedPictureReason::MissingFile));
  assert!(!is_kept(Some(&root.join("sub")), &format!("{}/sub/../picture.png", root.display())));

  std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_inside_the_picture_root_cannot_reach_a_file_outside_it() {
  use std::os::unix::fs::symlink;

  let root = std::env::temp_dir().join(format!("telegram-links-{}", std::process::id()));
  let outside = std::env::temp_dir().join(format!("telegram-outside-{}.png", std::process::id()));
  let link = root.join("escape.png");

  std::fs::create_dir_all(&root).unwrap();
  std::fs::write(&outside, b"x").unwrap();
  symlink(&outside, &link).unwrap();

  assert!(!is_kept(Some(&root), link.to_str().unwrap()));
  assert_eq!(
    unsendable(Some(&root), link.to_str().unwrap()),
    Some(MissedPictureReason::OutsideDataRoot)
  );

  std::fs::remove_dir_all(root).unwrap();
  std::fs::remove_file(outside).unwrap();
}
