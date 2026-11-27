//! A bot for each supervisor (docs/telegram.md, 「A bot for each supervisor」): the declaration says speakers where
//! a supervisor has a bot of its own; a send naming that supervisor goes through its bot, one its bot cannot send
//! goes through the service's own, one naming a supervisor with no bot goes through the service's own, and a speaker
//! that names no supervisor is refused.

use std::collections::BTreeMap;

use crate::support;
use bmo_chat::ChatProvider;
use serde_json::json;
use support::TOKEN;
use support::speaking::{BotSends, ROOM, post_send, provider_served, provider_served_with_bots, refusal};

fn bots() -> BTreeMap<String, String> {
  BTreeMap::from([("kakia-chat".to_owned(), "222:own-bot".to_owned())])
}

fn words(speaker: &str) -> serde_json::Value {
  json!({ "roomId": ROOM, "kind": "text", "fields": { "text": "hello" }, "speaker": speaker })
}

#[tokio::test]
async fn speakers_are_declared_only_where_a_supervisor_has_a_bot_of_its_own() {
  let (_, plain) = provider_served(BotSends::new(), Default::default()).await;
  let (_, with_bots) = provider_served_with_bots(BotSends::new(), Default::default(), &bots()).await;

  assert!(!plain.declaration().speakers);
  assert!(with_bots.declaration().speakers);
}

#[tokio::test]
async fn a_send_naming_a_supervisor_goes_through_its_own_bot_and_one_with_none_through_the_services() {
  let bot = BotSends::new();
  let (base, _) = provider_served_with_bots(bot.clone(), Default::default(), &bots()).await;

  let (status, _) = post_send(&base, words("kakia-chat")).await;
  assert_eq!(status, 201);
  let (status, _) = post_send(&base, words("bmo")).await;
  assert_eq!(status, 201);

  let sends: Vec<String> = bot
    .asked()
    .into_iter()
    .filter(|asked| asked.method == "sendMessage")
    .map(|asked| asked.bot)
    .collect();
  assert_eq!(sends, ["bot222:own-bot".to_owned(), format!("bot{TOKEN}")]);
}

#[tokio::test]
async fn a_supervisors_bot_turned_down_gives_way_to_the_services_bot() {
  let bot = BotSends::new();
  bot.answering(|asked| {
    (asked.bot == "bot222:own-bot")
      .then(|| refusal(403, "Forbidden: bot is not a member of the group chat"))
  });
  let (base, _) = provider_served_with_bots(bot.clone(), Default::default(), &bots()).await;

  let (status, _) = post_send(&base, words("kakia-chat")).await;

  assert_eq!(status, 201);
  let sends: Vec<String> = bot
    .asked()
    .into_iter()
    .filter(|asked| asked.method == "sendMessage")
    .map(|asked| asked.bot)
    .collect();
  assert_eq!(sends, ["bot222:own-bot".to_owned(), format!("bot{TOKEN}")]);
}

#[tokio::test]
async fn a_speaker_naming_no_supervisor_is_refused() {
  let bot = BotSends::new();
  let (base, _) = provider_served_with_bots(bot.clone(), Default::default(), &bots()).await;

  let (status, answer) = post_send(&base, words("account:21")).await;

  assert_eq!(status, 403, "{answer}");
  assert_eq!(answer["code"], "speaker-not-allowed");
  assert!(!bot.methods().contains(&"sendMessage".to_owned()));
}
