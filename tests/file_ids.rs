//! The record of the id each picture and file the bot account handed over crossed with (docs/telegram.md,
//! 「Reading a room」): kept across a restart, a later id of a message over an earlier one, and a record a stop
//! cut short left out.

use std::io::Write;

use bmo_telegram::file_ids::FileIds;
use tempfile::tempdir;

#[test]
fn the_ids_files_crossed_with_are_read_back_after_a_restart_the_latest_of_a_message_standing() {
  let folder = tempdir().expect("a temporary folder");
  let file = folder.path().join("file-ids.jsonl");
  let kept = FileIds::kept_in(file.clone());

  kept.keep(-100, 40, "doc-unique");
  kept.keep(-100, 41, "photo-unique");
  kept.keep(-100, 40, "newer-unique");
  kept.keep(-100, 40, "newer-unique");
  drop(kept);

  std::fs::OpenOptions::new()
    .append(true)
    .open(&file)
    .and_then(|mut record| record.write_all(br#"{"chatId":-100,"messageId":42,"#))
    .expect("a record cut short");

  let again = FileIds::kept_in(file.clone());

  assert_eq!(again.of(-100, 40).as_deref(), Some("newer-unique"));
  assert_eq!(again.of(-100, 41).as_deref(), Some("photo-unique"));
  assert_eq!(again.of(-100, 42), None);
  assert_eq!(std::fs::read_to_string(&file).unwrap().lines().count(), 4);
}
