//! The ledger of whose numbering each line of a room numbered per account stands under (docs/telegram.md, 「The
//! owner's voice」): a number of the bot account's kept with its line's stamp and read back after a restart;
//! records an earlier ledger left of the owner account's own numbers read back as they stand; a number under
//! both, or unlearned in a room the bot account reads, naming no line; and a provider number, which turns back
//! by its own arithmetic and needs no record at all (docs/reader.md, 「Numbers」).

use std::io::Write;

use bmo_telegram::kinds::Message;
use bmo_telegram::lines::{Line, Lines, Stamp, words_hash};
use serde_json::{Value, json};
use tempfile::tempdir;

// A group, in which each account numbers its own messages; another such group the bot account never read; and a
// supergroup, which numbers them for everyone.
const GROUP: i64 = -4_242;
const OTHER_GROUP: i64 = -4_343;
const SUPERGROUP: i64 = -1_001_234_567;

fn message(room: i64, id: i64, sender: i64, words: &str) -> Value {
  json!({ "message_id": id, "date": 1_700_000_000, "chat": { "id": room, "type": "group" },
    "from": { "id": sender, "is_bot": false, "first_name": "Someone" }, "text": words })
}

fn read(room: i64, id: i64, sender: i64, words: &str) -> Message {
  serde_json::from_value(message(room, id, sender, words)).expect("a message")
}

// A line as an earlier ledger wrote it, when the owner account's own numbers still crossed.
fn owner_record(file: &std::path::Path, room: i64, id: i64) {
  let mut line =
    serde_json::to_vec(&json!({ "chatId": room, "messageId": id, "by": "owner" })).expect("a record");
  line.push(b'\n');

  std::fs::OpenOptions::new()
    .create(true)
    .append(true)
    .open(file)
    .expect("the ledger")
    .write_all(&line)
    .expect("written");
}

#[test]
fn a_number_of_the_bot_account_s_is_kept_with_its_stamp_and_an_older_one_of_the_owner_s_reads_back_across_a_restart()
 {
  let folder = tempdir().expect("a temporary folder");
  let file = folder.path().join("lines.jsonl");
  let kept = Lines::kept_in(file.clone());

  kept.bots(&read(GROUP, 7, 55, "héllo there"));
  owner_record(&file, GROUP, 900);
  drop(kept);

  let again = Lines::kept_in(file);

  // The same hash the reader reckons for the same words.
  assert_eq!(
    words_hash("héllo there"),
    "1702af18d505d036fbde1cdf11961be738378371e39204f212dcbbc6f306ab19"
  );
  assert_eq!(
    again.line(GROUP, 7),
    Line::Bots(Stamp {
      sender_id: 55,
      sent_at: 1_700_000_000_000,
      words: words_hash("héllo there"),
    })
  );
  assert_eq!(again.line(GROUP, 900), Line::Owners);
}

#[test]
fn a_number_under_both_numberings_or_unlearned_in_a_room_the_bot_account_reads_names_no_line() {
  let folder = tempdir().expect("a temporary folder");
  let file = folder.path().join("lines.jsonl");

  {
    let kept = Lines::kept_in(file.clone());

    kept.bots(&read(GROUP, 7, 55, "hello"));
    owner_record(&file, GROUP, 7);
  }

  let lines = Lines::kept_in(file);

  // The line of 7 stands under both numberings since an older ledger met them: no act can tell which one the
  // receiving side means.
  assert_eq!(lines.line(GROUP, 7), Line::Unknown);
  assert_eq!(lines.line(GROUP, 8), Line::Unknown);
  // A room the bot account never read, a room numbered for everyone and a provider number stand.
  assert_eq!(lines.line(OTHER_GROUP, 8), Line::Owners);
  assert_eq!(lines.line(SUPERGROUP, 8), Line::Owners);
  assert_eq!(lines.line(GROUP, -999_999_000), Line::Owners);
}

#[test]
fn a_provider_number_of_a_room_needs_no_record_and_an_older_owner_number_stands_where_the_bot_reads_no_line_there()
 {
  let lines = Lines::in_memory();

  lines.bots(&read(GROUP, 7, 55, "hello"));

  // What the reader gave the line from below every bot number turns back by its own arithmetic.
  assert_eq!(lines.line(GROUP, -999_999_100), Line::Owners);
  // In a room the bot account never read, every number is the owner's account's.
  assert_eq!(lines.line(OTHER_GROUP, -999_999_100), Line::Owners);
  assert_eq!(lines.line(OTHER_GROUP, 10), Line::Owners);
}

#[test]
fn an_edit_the_bot_account_reads_moves_the_words_of_a_line_it_holds_under_its_number_and_adds_none() {
  let lines = Lines::in_memory();

  lines.bots(&read(GROUP, 7, 55, "old words"));
  lines.edited(&read(GROUP, 7, 55, "new words"));
  lines.edited(&read(GROUP, 9, 42, "a line the owner's account sent"));

  assert!(matches!(lines.line(GROUP, 7), Line::Bots(stamp) if stamp.words == words_hash("new words")));
  assert_eq!(lines.line(GROUP, 9), Line::Unknown);
}

#[test]
fn what_a_bot_api_call_answers_with_is_kept_where_it_is_a_message_or_several() {
  let lines = Lines::in_memory();

  lines.answered(&json!([message(GROUP, 500, 777, "first"), message(GROUP, 501, 777, "second")]));
  lines.answered(&json!(true));

  assert!(matches!(lines.line(GROUP, 500), Line::Bots(stamp) if stamp.sender_id == 777));
  assert!(matches!(lines.line(GROUP, 501), Line::Bots(stamp) if stamp.words == words_hash("second")));
}
