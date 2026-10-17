//! The Bot API as the bot account asks it: a method with its parameters as JSON, and a file's bytes by its
//! file id. The address is Telegram's own, or a stand-in a test points the provider at.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

/// Where the Bot API is asked unless the configuration names another address.
pub const TELEGRAM: &str = "https://api.telegram.org";

/// The Bot API behind one bot's token.
#[derive(Debug, Clone)]
pub struct BotApi {
  http: reqwest::Client,
  api: String,
  token: String,
}

/// The Bot API's answer to any method: whether it went, its result, and its description and parameters where
/// it did not.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Answered {
  pub ok: bool,
  pub result: Option<Value>,
  pub description: Option<String>,
  pub error_code: Option<u16>,
  pub parameters: Option<Parameters>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Parameters {
  pub retry_after: Option<u64>,
}

/// Why a call did not answer, and how long to wait before asking again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unanswered {
  pub why: String,
  pub wait: Duration,
}

impl std::fmt::Display for Unanswered {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.why)
  }
}

// How long to wait before asking again where the Bot API says nothing of it.
const AGAIN: Duration = Duration::from_secs(3);

// A long poll waits this long for an update; a call is let go a little after.
const CALL_TIMEOUT: Duration = Duration::from_secs(60);

impl BotApi {
  pub fn new(api: impl Into<String>, token: impl Into<String>) -> Self {
    let http = reqwest::Client::builder()
      .timeout(CALL_TIMEOUT)
      .build()
      .expect("a client with a timeout always builds");

    Self {
      http,
      api: api.into().trim_end_matches('/').to_owned(),
      token: token.into(),
    }
  }

  /// The token's own first part: the bot's number before the platform has been asked.
  pub fn token_identity(&self) -> Option<i64> {
    self.token.split(':').next().and_then(|id| id.parse().ok())
  }

  fn method(&self, method: &str) -> String {
    format!("{}/bot{}/{method}", self.api, self.token)
  }

  /// A method asked with its parameters, answered as the Bot API answered it.
  pub async fn answered(&self, method: &str, body: &Value) -> Result<Answered, String> {
    let bytes = serde_json::to_vec(body).map_err(|why| why.to_string())?;
    let answer = self
      .http
      .post(self.method(method))
      .header(reqwest::header::CONTENT_TYPE, "application/json")
      .body(bytes)
      .send()
      .await
      .map_err(|why| why.without_url().to_string())?;
    let bytes = answer.bytes().await.map_err(|why| why.without_url().to_string())?;

    serde_json::from_slice(&bytes).map_err(|why| why.to_string())
  }

  /// A method's result, or why it gave none and how long to wait before asking again.
  pub async fn call<T: DeserializeOwned>(&self, method: &str, body: &Value) -> Result<T, Unanswered> {
    let failed = |why: String| Unanswered { why, wait: AGAIN };
    let answered = self.answered(method, body).await.map_err(failed)?;

    match answered {
      Answered {
        ok: true,
        result: Some(result),
        ..
      } => serde_json::from_value(result).map_err(|why| failed(why.to_string())),
      Answered {
        description,
        error_code,
        parameters,
        ..
      } => Err(Unanswered {
        why: format!("{} ({})", description.unwrap_or_default(), error_code.unwrap_or_default()),
        wait: parameters
          .and_then(|parameters| parameters.retry_after)
          .map_or(AGAIN, Duration::from_secs),
      }),
    }
  }

  /// A file's bytes as the Bot API hands them over: its path asked for, then the file itself.
  pub async fn file_bytes(&self, file_id: &str) -> Result<Vec<u8>, String> {
    let answered = self.answered("getFile", &json!({ "file_id": file_id })).await?;
    let path = answered
      .result
      .as_ref()
      .filter(|_| answered.ok)
      .and_then(|result| result.get("file_path"))
      .and_then(Value::as_str);
    let Some(path) = path else {
      return Err(format!(
        "getFile failed: {}",
        answered.description.as_deref().unwrap_or("unknown error")
      ));
    };
    let answer = self
      .http
      .get(format!("{}/file/bot{}/{path}", self.api, self.token))
      .send()
      .await
      .map_err(|why| why.without_url().to_string())?;

    if !answer.status().is_success() {
      return Err(format!("file download failed: HTTP {}", answer.status().as_u16()));
    }

    answer
      .bytes()
      .await
      .map(|bytes| bytes.to_vec())
      .map_err(|why| why.without_url().to_string())
  }
}
