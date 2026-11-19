//! The ledger of whose numbering each line of a room numbered per account stands under (docs/telegram.md, 「The
//! owner's voice」): a number of the bot account's kept with its line's stamp, one of the owner's account's as it
//! stands, both read back after a restart; a number under both, or unlearned in a room the bot account reads,
//! naming no line.

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

#[test]
fn a_number_of_the_bot_account_s_is_kept_with_its_stamp_and_one_of_the_owner_s_as_it_stands_across_a_restart()
 {
  let folder = tempdir().expect("a temporary folder");
  let file = folder.path().join("lines.jsonl");
  let kept = Lines::kept_in(file.clone());

  kept.bots(&read(GROUP, 7, 55, "héllo there"));
  kept.owners(GROUP, 900);
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
  let lines = Lines::in_memory();

  lines.bots(&read(GROUP, 7, 55, "hello"));
  lines.owners(GROUP, 7);

  assert_eq!(lines.line(GROUP, 7), Line::Unknown);
  assert_eq!(lines.line(GROUP, 8), Line::Unknown);
  // A room the bot account never read, a room numbered for everyone and a number the history walk gave stand.
  assert_eq!(lines.line(OTHER_GROUP, 8), Line::Owners);
  assert_eq!(lines.line(SUPERGROUP, 8), Line::Owners);
  assert_eq!(lines.line(GROUP, -999_999_000), Line::Owners);
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
