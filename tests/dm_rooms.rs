//! A supervisor's bot's one-to-ones (docs/telegram.md, 「A bot for each supervisor」): each is given a room of its
//! own below every number Telegram gives a chat, kept across a restart; a message of a supervisor's bot's private
//! chat is handed over under that room and one of a group under none; and a send to the room goes out through
//! that supervisor's bot to the person's private chat, whatever speaker it names.

use std::collections::BTreeMap;

use crate::support;
use bmo_telegram::dm_rooms::{DmRooms, FIRST_ROOM};
use bmo_telegram::telegram::private_in_room;
use serde_json::json;
use support::speaking::{BotSends, post_send, provider_served_with_bots};

#[test]
fn each_one_to_one_with_a_supervisors_bot_is_a_room_of_its_own_kept_across_a_restart() {
  let folder = tempfile::tempdir().unwrap();
  let file = folder.path().join("supervisor-dms.jsonl");
  let rooms = DmRooms::kept_in(file.clone());
  let first = rooms.room_of("agent-b", 501);
  let second = rooms.room_of("agent-c", 501);
  let third = rooms.room_of("agent-b", 502);

  assert_eq!(first, FIRST_ROOM);
  assert_eq!(
    [second, third],
    [FIRST_ROOM - 1, FIRST_ROOM - 2],
    "one person's chats with two bots are two rooms"
  );
  assert_eq!(rooms.room_of("agent-b", 501), first, "a one-to-one keeps its room");

  let again = DmRooms::kept_in(file);

  assert_eq!(again.room_of("agent-c", 501), second, "a restart gives it the room it had");
  assert_eq!(
    again.of_room(third).map(|one| (one.supervisor, one.person)),
    Some(("agent-b".to_owned(), 502))
  );
  assert_eq!(again.room_of("agent-d", 1), FIRST_ROOM - 3);
}

#[test]
fn a_private_chats_message_is_handed_over_under_its_room_and_a_groups_under_none() {
  let rooms = DmRooms::default();
  let private = json!({ "message_id": 1, "chat": { "id": 501, "type": "private" }, "text": "hello" });
  let group = json!({ "message_id": 2, "chat": { "id": -1001, "type": "supergroup" }, "text": "hello" });
  let handed = private_in_room(private, "agent-b", &rooms).unwrap();

  assert_eq!(handed["chat"]["id"], json!(rooms.room_of("agent-b", 501)));
  assert_eq!(handed["text"], json!("hello"));
  assert!(private_in_room(group, "agent-b", &rooms).is_none());
}

#[tokio::test]
async fn a_send_to_a_supervisors_one_to_one_goes_through_its_bot_to_the_persons_chat() {
  let bot = BotSends::new();
  let bots = BTreeMap::from([("agent-b".to_owned(), "222:own-bot".to_owned())]);
  let (base, provider) = provider_served_with_bots(bot.clone(), Default::default(), &bots).await;
  let room = provider.dm_rooms().room_of("agent-b", 501);

  let (status, _) =
    post_send(&base, json!({ "roomId": room, "kind": "text", "fields": { "text": "hi" } })).await;

  assert_eq!(status, 201);

  let sent: Vec<(String, serde_json::Value)> = bot
    .asked()
    .into_iter()
    .filter(|asked| asked.method == "sendMessage")
    .map(|asked| (asked.bot, asked.body["chat_id"].clone()))
    .collect();

  assert_eq!(sent, [("bot222:own-bot".to_owned(), json!(501))]);
}
