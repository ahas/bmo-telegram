//! One JSON call to a service at a base address behind its bearer token: the status and body it answered,
//! or why no answer came. The reader's seam and the receiver's needed routes both stand on it.

use reqwest::Method;
use reqwest::header::CONTENT_TYPE;
use serde_json::Value;

/// A service reached over HTTP: its base address and the token its routes take.
#[derive(Debug, Clone)]
pub struct Wire {
  base: String,
  token: String,
  client: reqwest::Client,
}

/// What a call came to: the status and the JSON body, `Null` where the body was none or no JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
  pub status: u16,
  pub body: Value,
}

impl Answered {
  pub fn ok(&self) -> bool {
    (200..300).contains(&self.status)
  }

  /// The English sentence a failure answered with, or the status where it gave none.
  pub fn reason(&self) -> String {
    self
      .body
      .get("error")
      .and_then(Value::as_str)
      .map_or_else(|| format!("it answered {}", self.status), str::to_owned)
  }
}

impl Wire {
  /// A service at `base`. It reads no environment of its own, so no proxy is taken from it.
  pub fn new(base: impl Into<String>, token: impl Into<String>) -> Self {
    let client = reqwest::Client::builder()
      .no_proxy()
      .build()
      .expect("a client with no TLS and no proxy always builds");

    Self {
      base: base.into().trim_end_matches('/').to_owned(),
      token: token.into(),
      client,
    }
  }

  /// A service listening on the loopback at `port`.
  pub fn loopback(port: u16, token: impl Into<String>) -> Self {
    Self::new(format!("http://127.0.0.1:{port}"), token)
  }

  /// `GET` of a route.
  pub async fn get(&self, route: &str) -> Result<Answered, String> {
    self.call(Method::GET, route, None).await
  }

  /// `POST` of a JSON body to a route.
  pub async fn post(&self, route: &str, body: &Value) -> Result<Answered, String> {
    self.call(Method::POST, route, Some(body)).await
  }

  async fn call(&self, method: Method, route: &str, body: Option<&Value>) -> Result<Answered, String> {
    let url = format!("{}{route}", self.base);
    let mut request = self.client.request(method, &url).bearer_auth(&self.token);

    if let Some(body) = body {
      let bytes = serde_json::to_vec(body).map_err(|why| why.to_string())?;

      request = request.header(CONTENT_TYPE, "application/json").body(bytes);
    }

    let answer = request.send().await.map_err(|why| format!("{url} did not answer: {why}"))?;
    let status = answer.status().as_u16();
    let bytes = answer.bytes().await.map_err(|why| format!("{url} answered unreadably: {why}"))?;

    Ok(Answered {
      status,
      body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    })
  }
}
