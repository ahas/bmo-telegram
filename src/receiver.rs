//! What the provider needs of the receiver beyond the chat protocol (docs/boundary-api.md, 「Needed from the
//! receiver」): whether a room is listened to, the rooms whose history may be walked, a past message kept
//! where the receiver lacks it, and word that profile pictures changed. The receiver's boundary states none of
//! these routes yet, so each call answers what came of it or why no answer came, and the caller applies the
//! fallback the boundary states.

use bmo_chat::ChatHandover;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::wire::Wire;

/// The provider's key, as every hand-over carries it.
pub const VIA: &str = "telegram";

/// A room whose history may be walked, with the oldest message the receiver holds of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRoom {
  pub chat_id: i64,
  pub oldest_message_id: Option<i64>,
  pub oldest_sent_at: Option<i64>,
}

/// The receiver's routes this provider needs, at its base address behind its token.
#[derive(Debug, Clone)]
pub struct ReceiverNeeds {
  wire: Wire,
}

impl ReceiverNeeds {
  pub fn new(wire: Wire) -> Self {
    Self { wire }
  }

  // The body a route answered with, or why it gave none: a route the receiver does not serve, a failure, or
  // no answer at all.
  async fn post(&self, route: &str, body: &Value) -> Result<Value, String> {
    let answer = self.wire.post(route, body).await?;

    if answer.ok() {
      Ok(answer.body)
    } else {
      Err(format!("the receiver's {route} {}", answer.reason()))
    }
  }

  /// Whether the receiver listens to a room, or why it did not say.
  pub async fn listened(&self, chat_id: i64) -> Result<bool, String> {
    let answer = self.post("/rooms/listened", &json!({ "chatId": chat_id, "via": VIA })).await?;

    answer
      .get("listened")
      .and_then(Value::as_bool)
      .ok_or_else(|| "the receiver's /rooms/listened answered no `listened`".to_owned())
  }

  /// The rooms whose history may be walked, or why the receiver did not say.
  pub async fn rooms(&self) -> Result<Vec<HistoryRoom>, String> {
    let answer = self.post("/rooms/of", &json!({ "via": VIA })).await?;

    serde_json::from_value(answer.get("rooms").cloned().unwrap_or(Value::Null))
      .map_err(|why| format!("the receiver's /rooms/of answered no list of rooms: {why}"))
  }

  /// A past message handed over to be kept where the receiver lacks it: whether it was kept, or why the
  /// receiver did not take it.
  pub async fn history(&self, handover: &ChatHandover) -> Result<bool, String> {
    let body = serde_json::to_value(handover).map_err(|why| why.to_string())?;
    let answer = self.post("/history", &body).await?;

    Ok(answer.get("kept").and_then(Value::as_bool).unwrap_or(false))
  }

  /// Word that the profile pictures of these rooms and people changed, or why it did not go.
  pub async fn avatars_updated(&self, rooms: &[i64], people: &[i64]) -> Result<(), String> {
    self
      .post("/avatars/updated", &json!({ "via": VIA, "rooms": rooms, "people": people }))
      .await
      .map(drop)
  }
}
