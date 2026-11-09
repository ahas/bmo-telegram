//! The supervisor's road to one chat's recent messages (docs/boundary-api.md, 「This repository's own routes」):
//! the owner's word alone, read through the service and the reader, never into the chat. The turn a call is made
//! in is read off the process's environment, or, for a resident program, off the run its place names.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::provider::ReaderCandidate;
use crate::reader::ReaderMessage;

/// How many messages a read answers where it asks for no number, and the most it answers at all.
pub const DEFAULT_MAX: usize = 50;
pub const LARGEST_MAX: usize = 200;

/// The turn variable the receiver marks every message of the owner's turn by.
pub const PRIVILEGED_VARIABLE: &str = "BMO_TURN_PRIVILEGED";

// The variable naming a resident program's place, whose run's variables a verb reads.
const PLACE_VARIABLE: &str = "BMO_AGENT_PLACE";
const TURN_PREFIX: &str = "BMO_TURN_";

/// Why a call made outside the owner's word is refused: no grant in force names reading a chat.
pub const NOT_OWNER: &str =
  "Reading a chat is the owner's word alone: this turn is not the owner's, and no grant in force names it.";

/// Whether the call is made on the owner's word alone: the receiver marked the turn's every message the
/// owner's. A turn no room opened and a turn the receiver marked not the owner's are both refused.
pub fn owner_admitted(variables: &BTreeMap<String, String>) -> Result<(), String> {
  if variables.get(PRIVILEGED_VARIABLE).is_some_and(|flag| flag.trim() == "1") {
    Ok(())
  } else {
    Err(NOT_OWNER.to_owned())
  }
}

/// The process's environment with a resident program's run's own variables in it, as a turn's owner mark is
/// read.
pub fn turn_variables() -> BTreeMap<String, String> {
  let environment: BTreeMap<String, String> = std::env::vars().collect();

  read_turn_variables(&environment)
}

fn read_turn_variables(environment: &BTreeMap<String, String>) -> BTreeMap<String, String> {
  let Some(place) = environment.get(PLACE_VARIABLE).filter(|place| !place.is_empty()) else {
    return environment.clone();
  };
  let run: Option<Value> = std::fs::read_to_string(Path::new(place).join("run.json"))
    .ok()
    .and_then(|text| serde_json::from_str(&text).ok());
  let mut own: BTreeMap<String, String> = environment
    .iter()
    .filter(|(name, _)| !name.starts_with(TURN_PREFIX))
    .map(|(name, value)| (name.clone(), value.clone()))
    .collect();

  if let Some(run) = run.filter(|run| run.get("person").and_then(Value::as_bool) != Some(true))
    && let Some(env) = run.get("env").and_then(Value::as_object)
  {
    for (name, value) in env {
      if let Some(value) = value.as_str() {
        own.insert(name.clone(), value.to_owned());
      }
    }
  }

  own
}

/// The messages a service's answer carries, newest first.
pub fn messages_of(body: &Value) -> Result<Vec<ReaderMessage>, String> {
  serde_json::from_value(body.get("messages").cloned().unwrap_or(Value::Null))
    .map_err(|why| format!("the service's messages did not read: {why}"))
}

/// The chats and contacts a service's answer names for the owner to pick among.
pub fn candidates_of(body: &Value) -> Result<Vec<ReaderCandidate>, String> {
  serde_json::from_value(body.get("candidates").cloned().unwrap_or(Value::Null))
    .map_err(|why| format!("the service's candidates did not read: {why}"))
}

/// One message as a person reads it: the moment in epoch milliseconds, the sender's name and number, and the
/// words or the placeholder its kind stands under, as they are.
pub fn message_line(message: &ReaderMessage) -> String {
  let sender = match message.sender_id {
    Some(id) => format!("{} ({id})", message.sender_name),
    None => message.sender_name.clone(),
  };

  format!("{} {sender}: {}", message.sent_at, message.text)
}

/// One candidate as a person reads it: its number, the name it is shown by, and what it is.
pub fn candidate_line(candidate: &ReaderCandidate) -> String {
  format!("{} {} ({})", candidate.chat_id, candidate.name, candidate.kind)
}
