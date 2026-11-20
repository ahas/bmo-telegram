//! A room's members as the bot account can tell them, and the lists of both accounts made one
//! (docs/telegram.md, 「The members of a room」). The Bot API lists no group's members, so the bot gives a group's
//! administrators and asks Telegram after each sender the provider has seen in the room, one by one, keeping
//! those still in it. Every person stands under their Telegram number, the sender number their messages carry.

use std::collections::{BTreeSet, HashSet};

use bmo_chat::ChatMember;
use serde_json::{Value, json};

use crate::bot_api::BotApi;
use crate::provider::asked_each;

/// Whether a `ChatMember` the Bot API answered is in the chat: its creator, an administrator or a member, or a
/// restricted member Telegram still counts as one.
pub fn still_member(answer: &Value) -> bool {
  match answer.get("status").and_then(Value::as_str) {
    Some("creator" | "administrator" | "member") => true,
    Some("restricted") => answer.get("is_member").and_then(Value::as_bool) == Some(true),
    _ => false,
  }
}

/// The name Telegram shows for a user: their first and last name, else their handle, else their number.
pub fn name_of(user: &Value) -> String {
  let part = |key: &str| user.get(key).and_then(Value::as_str).filter(|part| !part.is_empty());
  let name = [part("first_name"), part("last_name")]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");

  if !name.is_empty() {
    return name;
  }

  match (part("username"), user.get("id").and_then(Value::as_i64)) {
    (Some(handle), _) => format!("@{handle}"),
    (None, Some(id)) => id.to_string(),
    (None, None) => String::new(),
  }
}

/// A user as a member of a room, under their Telegram number; none where the user carries no number.
pub fn member_of(user: &Value) -> Option<ChatMember> {
  let person = user.get("id").and_then(Value::as_i64)?;

  Some(ChatMember {
    person: Some(person),
    name: name_of(user),
    status_message: None,
  })
}

/// The lists made one: each in turn, a person already standing in an earlier one left out of a later.
pub fn merged(lists: impl IntoIterator<Item = Vec<ChatMember>>) -> Vec<ChatMember> {
  let mut standing = HashSet::new();

  lists
    .into_iter()
    .flatten()
    .filter(|member| member.person.is_none_or(|person| standing.insert(person)))
    .collect()
}

/// A group's or a channel's members as the bot account tells them: its administrators, then each of `senders`
/// Telegram says is still in it, in their order. The administrators unread is a failure with Telegram's words;
/// a sender Telegram does not answer for is left out.
pub async fn group_members(
  api: &BotApi,
  chat_id: i64,
  senders: &BTreeSet<i64>,
) -> Result<Vec<ChatMember>, String> {
  let administrators: Vec<Value> = api
    .call("getChatAdministrators", &json!({ "chat_id": chat_id }))
    .await
    .map_err(|why| format!("Telegram gave no administrators: {why}"))?;
  let administrators: Vec<ChatMember> =
    administrators.iter().filter_map(|one| one.get("user").and_then(member_of)).collect();
  let listed: HashSet<i64> = administrators.iter().filter_map(|member| member.person).collect();
  let others: Vec<i64> = senders.iter().copied().filter(|sender| !listed.contains(sender)).collect();
  let api = api.clone();
  let mut checked = asked_each(&others, move |person| {
    let api = api.clone();

    async move { still_in(&api, chat_id, person).await }
  })
  .await;
  let still = others.iter().filter_map(|person| checked.remove(person).flatten());

  Ok(administrators.into_iter().chain(still).collect())
}

// A person as a member of a chat where Telegram says they are still in it; none where they left, were removed,
// or Telegram does not know them there.
async fn still_in(api: &BotApi, chat_id: i64, person: i64) -> Option<ChatMember> {
  let answer: Value = api
    .call("getChatMember", &json!({ "chat_id": chat_id, "user_id": person }))
    .await
    .ok()?;

  still_member(&answer).then(|| answer.get("user").and_then(member_of)).flatten()
}

/// A one-to-one of the bot's: its person, named as Telegram tells the bot, and the bot itself, `me`. A name
/// Telegram does not tell stands as the number.
pub async fn private_members(api: &BotApi, person: i64, me: i64) -> Vec<ChatMember> {
  let named = |id: i64, user: Option<Value>| ChatMember {
    person: Some(id),
    name: user
      .map(|user| name_of(&user))
      .filter(|name| !name.is_empty())
      .unwrap_or_else(|| id.to_string()),
    status_message: None,
  };
  let person_user = api.call::<Value>("getChat", &json!({ "chat_id": person })).await.ok();
  let bot_user = api.call::<Value>("getMe", &json!({})).await.ok();

  vec![named(person, person_user), named(me, bot_user)]
}
