//! The bot account's acts on a message and its sticker (docs/telegram.md, 「Sends」), carried to a stand-in of
//! the Bot API: what went to it, and what each act answered — done, refused, or refused as gone. The last three
//! are posted to the provider's `/sends` route as the receiving side posts them.

mod support;

use bmo_chat::{ChatSend, ChatSendResult};
use bmo_telegram::policy::POLICY;
use bmo_telegram::send::{Sending, sent};
use serde_json::{Value, json};
use support::speaking::{Asked, BotSends, ROOM, post_send, provider_served, refusal, send, speaker};

const MESSAGE: i64 = 9;
const SET: &str = "AnimalSet";

fn with_set() -> Sending {
  Sending {
    sticker_set: Some(SET.to_owned()),
    ..Sending::default()
  }
}

// The set the stand-in holds: one sticker for each of two feelings, so the pick among them is no chance.
fn sticker_set(asked: &Asked) -> Option<Value> {
  (asked.method == "getStickerSet").then(|| {
    let sticker = |file_id: &str, emoji: &str| {
      json!({ "file_id": file_id, "file_unique_id": format!("u-{file_id}"), "emoji": emoji, "is_animated": false,
        "is_video": false, "width": 512, "height": 512, "type": "regular" })
    };

    json!({ "ok": true, "result": { "name": asked.body["name"], "title": "Animals", "sticker_type": "regular",
      "stickers": [sticker("sticker-1", "\u{1f600}"), sticker("sticker-2", "\u{1f622}")] } })
  })
}

#[tokio::test]
async fn a_reaction_a_pin_its_undoing_the_typing_mark_and_a_delete_go_as_asked_answering_the_message_acted_on()
 {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let reaction = |emoji: &str| ChatSend::Reaction {
    message_id: MESSAGE,
    emoji: emoji.to_owned(),
  };

  assert_eq!(send(&speaker, None, reaction("\u{1f44d}")).await, Ok(sent(Some(MESSAGE))));

  bot.answering(|_| None);
  send(&speaker, None, reaction("")).await.unwrap();
  assert_eq!(bot.bodies(), [json!({ "chat_id": ROOM, "message_id": MESSAGE, "reaction": [] })]);

  bot.answering(|_| None);
  send(
    &speaker,
    None,
    ChatSend::Pin {
      message_id: MESSAGE,
      on: true,
      silent: true,
    },
  )
  .await
  .unwrap();
  send(
    &speaker,
    None,
    ChatSend::Pin {
      message_id: MESSAGE,
      on: false,
      silent: false,
    },
  )
  .await
  .unwrap();
  assert_eq!(bot.methods(), ["pinChatMessage", "unpinChatMessage"]);
  assert_eq!(
    bot.bodies()[0],
    json!({ "chat_id": ROOM, "message_id": MESSAGE, "disable_notification": true })
  );

  bot.answering(|_| None);
  assert_eq!(send(&speaker, None, ChatSend::Typing { on: true }).await, Ok(sent(None)));
  assert_eq!(send(&speaker, None, ChatSend::Typing { on: false }).await, Ok(sent(None)));
  assert_eq!(bot.methods(), ["sendChatAction"]);

  bot.answering(|_| None);
  assert_eq!(
    send(
      &speaker,
      None,
      ChatSend::Delete {
        message_id: MESSAGE
      }
    )
    .await,
    Ok(sent(Some(MESSAGE)))
  );
  assert_eq!(bot.methods(), ["deleteMessage"]);
}

#[tokio::test]
async fn an_edit_is_done_done_where_the_words_are_the_same_refused_as_gone_and_refused_otherwise() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let edit = || ChatSend::Edit {
    message_id: MESSAGE,
    text: "new board".to_owned(),
  };
  let answering = |description: &'static str| {
    bot.answering(move |asked| (asked.method == "editMessageText").then(|| refusal(400, description)))
  };

  assert_eq!(send(&speaker, None, edit()).await, Ok(sent(Some(MESSAGE))));
  assert_eq!(bot.bodies(), [json!({ "chat_id": ROOM, "message_id": MESSAGE, "text": "new board" })]);

  answering("Bad Request: message is not modified: specified new message content is exactly the same");
  assert_eq!(send(&speaker, None, edit()).await, Ok(sent(Some(MESSAGE))));

  answering("Bad Request: message to edit not found");
  assert_eq!(
    send(&speaker, None, edit()).await,
    Ok(ChatSendResult::Refused(POLICY.message_gone("Bad Request: message to edit not found")))
  );

  answering("Bad Request: message text is empty");
  assert_eq!(
    send(&speaker, None, edit()).await,
    Ok(ChatSendResult::Refused(POLICY.refused("Bad Request: message text is empty")))
  );
}

#[tokio::test]
async fn a_sticker_is_picked_out_of_the_configured_set_by_its_feeling() {
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), with_set()).await;

  bot.answering(sticker_set);

  assert_eq!(
    send(
      &speaker,
      None,
      ChatSend::Sticker {
        emoji: "\u{1f600}".to_owned()
      }
    )
    .await,
    Ok(sent(Some(500)))
  );
  assert_eq!(bot.methods(), ["getStickerSet", "sendSticker"]);
  assert_eq!(bot.bodies()[0], json!({ "name": SET }));
  assert_eq!(bot.bodies()[1], json!({ "chat_id": ROOM, "sticker": "sticker-1" }));
}

#[tokio::test]
async fn a_feeling_the_set_does_not_carry_is_refused_and_so_is_a_sticker_with_no_set() {
  let bot = BotSends::new();
  let with = speaker(bot.clone(), with_set()).await;

  bot.answering(sticker_set);

  assert_eq!(
    send(
      &with,
      None,
      ChatSend::Sticker {
        emoji: "\u{1f984}".to_owned()
      }
    )
    .await,
    Ok(ChatSendResult::Refused(POLICY.sticker_missing(SET, "\u{1f984}")))
  );
  assert_eq!(bot.methods(), ["getStickerSet"]);

  let unset = speaker(bot.clone(), Sending::default()).await;

  bot.answering(sticker_set);
  assert_eq!(
    send(
      &unset,
      None,
      ChatSend::Sticker {
        emoji: "\u{1f600}".to_owned()
      }
    )
    .await,
    Ok(ChatSendResult::Refused(POLICY.sticker_unset()))
  );
  assert!(bot.methods().is_empty());
}

#[tokio::test]
async fn a_pin_is_quiet_and_an_unpin_undoes_it() {
  let bot = BotSends::new();
  let (base, _provider) = provider_served(bot.clone(), Sending::default()).await;

  bot.answering(|_| None);

  let pinned = post_send(
    &base,
    json!({ "roomId": ROOM, "kind": "pin", "fields": { "messageId": 5, "on": true, "silent": true } }),
  )
  .await;
  let unpinned = post_send(
    &base,
    json!({ "roomId": ROOM, "kind": "pin", "fields": { "messageId": 5, "on": false } }),
  )
  .await;

  assert_eq!(pinned, (201, json!({ "messageId": 5, "missed": [] })));
  assert_eq!(unpinned, (201, json!({ "messageId": 5, "missed": [] })));
  assert_eq!(bot.methods(), ["pinChatMessage", "unpinChatMessage"]);
  assert_eq!(bot.bodies()[0]["disable_notification"], true);
  assert_eq!(bot.bodies()[1], json!({ "chat_id": ROOM, "message_id": 5 }));
}

#[tokio::test]
async fn a_reaction_is_put_on_and_none_takes_it_off() {
  let bot = BotSends::new();
  let (base, _provider) = provider_served(bot.clone(), Sending::default()).await;

  bot.answering(|_| None);

  let put = post_send(
    &base,
    json!({ "roomId": ROOM, "kind": "reaction", "fields": { "messageId": 5, "emoji": "\u{1f44d}" } }),
  )
  .await;
  let taken = post_send(
    &base,
    json!({ "roomId": ROOM, "kind": "reaction", "fields": { "messageId": 5, "emoji": "" } }),
  )
  .await;

  assert_eq!([put.0, taken.0], [201, 201]);
  assert_eq!(bot.bodies()[0]["reaction"], json!([{ "type": "emoji", "emoji": "\u{1f44d}" }]));
  assert_eq!(bot.bodies()[1]["reaction"], json!([]));
}

#[tokio::test]
async fn a_sticker_the_set_does_not_carry_is_refused_with_where_to_look() {
  let bot = BotSends::new();
  let (base, _provider) = provider_served(bot.clone(), with_set()).await;

  bot.answering(sticker_set);

  let (status, answer) =
    post_send(&base, json!({ "roomId": ROOM, "kind": "sticker", "fields": { "emoji": "\u{1f642}" } }))
      .await;

  assert_eq!(status, 422);
  assert_eq!(
    answer,
    json!({ "code": "platform",
      "error": format!("Telegram: The sticker set {SET} has no sticker for \u{1f642}.") })
  );
  assert!(!bot.methods().contains(&"sendSticker".to_owned()));
}
