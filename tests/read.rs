//! The supervisor's road to one chat's recent messages (docs/boundary-api.md, 「This repository's own
//! routes」): the owner's word alone, the lines as the service reads them, and the candidates a name leaves to
//! pick among.

use std::collections::BTreeMap;

use bmo_telegram::read::{
  NOT_OWNER, candidate_line, candidates_of, message_line, messages_of, owner_admitted,
};
use serde_json::json;

#[test]
fn a_turn_that_is_not_the_owner_is_refused_since_no_grant_names_reading_a_chat() {
  assert_eq!(owner_admitted(&BTreeMap::new()), Err(NOT_OWNER.to_owned()));

  let member = BTreeMap::from([("BMO_TURN_PRIVILEGED".to_owned(), "0".to_owned())]);

  assert_eq!(owner_admitted(&member), Err(NOT_OWNER.to_owned()));
  assert!(NOT_OWNER.contains("no grant in force names it"));

  let owner = BTreeMap::from([("BMO_TURN_PRIVILEGED".to_owned(), "1".to_owned())]);

  assert_eq!(owner_admitted(&owner), Ok(()));
}

#[test]
fn the_messages_of_an_answer_read_in_the_order_the_service_answered_them() {
  let body = json!({ "messages": [
    { "messageId": 2, "senderId": 55, "senderName": "Bong", "sentAt": 2_000, "text": "newer" },
    { "messageId": 1, "senderId": null, "senderName": "unknown", "sentAt": 1_000, "text": "[Photo]" }
  ] });
  let messages = messages_of(&body).expect("the answer reads as messages");

  assert_eq!(messages.iter().map(|one| one.message_id).collect::<Vec<_>>(), [2, 1]);
  assert_eq!(message_line(&messages[0]), "2000 Bong (55): newer");
  assert_eq!(message_line(&messages[1]), "1000 unknown: [Photo]");
}

#[test]
fn the_candidates_of_an_answer_read_by_number_and_name() {
  let body = json!({ "candidates": [
    { "chatId": 55, "name": "Bong", "kind": "room" },
    { "chatId": 66, "name": "Bong's bot", "kind": "contact" }
  ] });
  let candidates = candidates_of(&body).expect("the answer reads as candidates");

  assert_eq!(candidate_line(&candidates[0]), "55 Bong (room)");
  assert_eq!(candidate_line(&candidates[1]), "66 Bong's bot (contact)");
}
