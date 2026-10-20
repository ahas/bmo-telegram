//! The bot account's acts on a message and its sticker (docs/telegram.md, 「Sends」): a reaction, a pin and its
//! undoing, the typing mark, an edit and a delete — a message Telegram no longer has answered with the refusal
//! marked gone — and a sticker picked out of the configured set by the emoji Telegram attaches to it.

use rand::seq::IndexedRandom;
use regex::RegexBuilder;
use serde_json::{Value, json};

use crate::bot_api::Answered;
use crate::policy::POLICY;
use crate::send::{Sent, Speaker, message_id, refused, sent, turned_down};

/// Whether Telegram's description of a refused act says the message is not there to act on.
pub fn is_gone(description: &str) -> bool {
  RegexBuilder::new(&POLICY.telegram.gone_descriptions.join("|"))
    .case_insensitive(true)
    .build()
    .is_ok_and(|gone| gone.is_match(description))
}

impl Speaker {
  // An act on a message already sent: done, gone, or turned down. An edit to the words the message already
  // says is done.
  async fn on_message(&self, method: &str, body: &Value, message_id: i64) -> Sent {
    let answered = self.call(method, body).await?;
    let description = answered.description.as_deref().unwrap_or_default();

    if answered.ok
      || (method == "editMessageText" && description.contains(&POLICY.telegram.unchanged_description))
    {
      return Ok(sent(Some(message_id)));
    }

    if is_gone(description) {
      return Ok(refused(POLICY.message_gone(description)));
    }

    turned_down(method, &answered)
  }

  /// The emoji on a message; an empty emoji takes the bot's own reaction off.
  pub async fn reaction(&self, chat_id: i64, message_id: i64, emoji: &str) -> Sent {
    let reaction = if emoji.is_empty() { json!([]) } else { json!([{ "type": "emoji", "emoji": emoji }]) };
    let body = json!({ "chat_id": chat_id, "message_id": message_id, "reaction": reaction });

    self.on_message("setMessageReaction", &body, message_id).await
  }

  /// A message pinned, quietly where `silent`, or unpinned.
  pub async fn pin(&self, chat_id: i64, message_id: i64, on: bool, silent: bool) -> Sent {
    if on {
      let body = json!({ "chat_id": chat_id, "message_id": message_id, "disable_notification": silent });

      return self.on_message("pinChatMessage", &body, message_id).await;
    }

    let body = json!({ "chat_id": chat_id, "message_id": message_id });

    self.on_message("unpinChatMessage", &body, message_id).await
  }

  /// The typing mark put up; taking it off asks nothing, since Telegram takes it down by itself.
  pub async fn typing(&self, chat_id: i64, on: bool) -> Sent {
    if !on {
      return Ok(sent(None));
    }

    let answered = self.call("sendChatAction", &json!({ "chat_id": chat_id, "action": "typing" })).await?;

    if answered.ok { Ok(sent(None)) } else { turned_down("sendChatAction", &answered) }
  }

  /// A message's words changed.
  pub async fn edit(&self, chat_id: i64, message_id: i64, text: &str) -> Sent {
    let body = json!({ "chat_id": chat_id, "message_id": message_id, "text": text });

    self.on_message("editMessageText", &body, message_id).await
  }

  /// A message taken down.
  pub async fn delete(&self, chat_id: i64, message_id: i64) -> Sent {
    let body = json!({ "chat_id": chat_id, "message_id": message_id });

    self.on_message("deleteMessage", &body, message_id).await
  }

  /// The sticker for a feeling out of the configured set; a feeling the set does not carry is refused, which
  /// is a different answer from the provider not doing stickers at all.
  pub async fn sticker(&self, chat_id: i64, emoji: &str) -> Sent {
    let Some(set) = self.sending.sticker_set.as_deref() else {
      return Ok(refused(POLICY.sticker_unset()));
    };
    let answered = self.call("getStickerSet", &json!({ "name": set })).await?;
    let Some(stickers) = answered.result.as_ref().filter(|_| answered.ok) else {
      return turned_down("getStickerSet", &answered);
    };
    let matching: Vec<&Value> = stickers
      .get("stickers")
      .and_then(Value::as_array)
      .map(|stickers| {
        stickers
          .iter()
          .filter(|one| one.get("emoji").and_then(Value::as_str) == Some(emoji))
          .collect()
      })
      .unwrap_or_default();
    let Some(file_id) = matching
      .choose(&mut rand::rng())
      .and_then(|one| one.get("file_id"))
      .and_then(Value::as_str)
    else {
      return Ok(refused(POLICY.sticker_missing(set, emoji)));
    };
    let answered: Answered =
      self.call("sendSticker", &json!({ "chat_id": chat_id, "sticker": file_id })).await?;
    let Some(went) = message_id(&answered) else {
      return turned_down("sendSticker", &answered);
    };

    self.spoke_in(chat_id);

    Ok(sent(Some(went)))
  }
}
