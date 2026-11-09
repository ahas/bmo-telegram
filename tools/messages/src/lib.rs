//! The hosted tool that reads one chat's recent messages through the running provider (docs/tools.md): the
//! owner's word alone, the provider's token and loopback port from the env file under the data root, and the
//! turn's owner mark from the session record. Nothing is sent into the chat and no room is made.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The protocol version this side speaks.
pub const PROTOCOL: u64 = 1;

/// The entry this executable answers, by its full name.
pub const ENTRY: &str = "bmo/telegram_messages";

/// The env file under the data root the deployment writes the provider's token and port into.
pub const ENV_FILE: &str = "env/telegram.env";

/// The variables the env file names.
pub const TOKEN_VARIABLE: &str = "BMO_TELEGRAM_TOKEN";
pub const PORT_VARIABLE: &str = "BMO_TELEGRAM_PORT";

/// The provider's loopback port where the env file names none.
pub const DEFAULT_PORT: u16 = 3220;

/// How many messages a read answers where it asks for no number, and the most it answers at all.
pub const DEFAULT_MAX: usize = 50;
pub const LARGEST_MAX: usize = 200;

/// Why a call made outside the owner's word is refused: no grant in force names reading a chat.
pub const NOT_OWNER: &str =
  "Reading a chat is the owner's word alone: this turn is not the owner's, and no grant in force names it.";

/// The data root the session record names.
pub const DATA_ROOT_VARIABLE: &str = "BMO_DATA_DIR";

/// The turn variable the receiver marks every message of the owner's turn by.
pub const PRIVILEGED_VARIABLE: &str = "BMO_TURN_PRIVILEGED";

/// A tool result saying plain text.
pub fn text(said: &str) -> Value {
  json!({ "content": [{ "type": "text", "text": said }], "isError": false })
}

/// A tool result refusing, which the model reads as any other answer.
pub fn refusal(said: &str) -> Value {
  json!({ "content": [{ "type": "text", "text": said }], "isError": true })
}

/// One chat read asked for by the call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Input {
  /// The chat: its number, or the name or handle the owner's account shows it by.
  pub chat: String,
  /// How many messages at most, newest first.
  #[serde(default)]
  pub max: Option<usize>,
  /// Only messages sent at or after this moment, in epoch milliseconds.
  #[serde(default)]
  pub since: Option<i64>,
}

/// The session record a call is made under: the parts of it the tool reads.
pub struct Session<'a> {
  record: &'a Value,
}

impl<'a> Session<'a> {
  pub fn of(record: &'a Value) -> Self {
    Self { record }
  }

  fn variables(&self) -> Option<&serde_json::Map<String, Value>> {
    self.record.get("variables").and_then(Value::as_object)
  }

  /// The data root the record names, whose env file holds the provider's token and port.
  pub fn data_root(&self) -> PathBuf {
    self
      .variables()
      .and_then(|variables| variables.get(DATA_ROOT_VARIABLE))
      .and_then(Value::as_str)
      .map(PathBuf::from)
      .unwrap_or_default()
  }

  /// One variable of the turn the call is made in: the run a resident program's place names, none for a
  /// person's turn, standing over the record's own variables.
  pub fn turn_variable(&self, name: &str) -> Option<String> {
    let from_place = self
      .record
      .get("labels")
      .and_then(Value::as_object)
      .and_then(|labels| labels.get("place"))
      .and_then(Value::as_str)
      .and_then(|place| {
        let run: Value =
          serde_json::from_str(&std::fs::read_to_string(Path::new(place).join("run.json")).ok()?).ok()?;

        if run.get("person").and_then(Value::as_bool) == Some(true) {
          return None;
        }

        run.get("env")?.get(name)?.as_str().map(str::to_owned)
      });

    from_place.or_else(|| {
      self
        .variables()
        .and_then(|variables| variables.get(name))
        .and_then(Value::as_str)
        .map(str::to_owned)
    })
  }

  /// Whether the call is made on the owner's word alone: the receiver marked the turn's every message the
  /// owner's. No grant names reading a chat.
  pub fn admitted(&self) -> Result<(), String> {
    if self.turn_variable(PRIVILEGED_VARIABLE).is_some_and(|flag| flag.trim() == "1") {
      Ok(())
    } else {
      Err(NOT_OWNER.to_owned())
    }
  }
}

/// The running provider as this process reaches it: its loopback port and the token its routes take.
pub struct Service {
  pub port: u16,
  pub token: String,
}

impl Service {
  /// The provider the data root's env file describes.
  pub fn of(data_root: &Path) -> Result<Self, String> {
    let file = data_root.join(ENV_FILE);
    let values = env_settings(&file);
    let named = |name: &str| {
      std::env::var(name)
        .ok()
        .or_else(|| values.get(name).cloned())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    };
    let Some(token) = named(TOKEN_VARIABLE) else {
      return Err(format!("the provider's token is not named in {TOKEN_VARIABLE} in {}", file.display()));
    };
    let port = named(PORT_VARIABLE).and_then(|port| port.parse().ok()).unwrap_or(DEFAULT_PORT);

    Ok(Self { port, token })
  }
}

// Every variable an env file under the data root names; none where there is no such file.
fn env_settings(file: &Path) -> HashMap<String, String> {
  dotenvy::from_path_iter(file)
    .map(|pairs| pairs.filter_map(Result::ok).collect())
    .unwrap_or_default()
}

/// One call answered: a refusal the model reads, or the lines read.
pub async fn call(session: &Session<'_>, arguments: Value) -> Value {
  if let Err(why) = session.admitted() {
    return refusal(&why);
  }

  let input: Input = match serde_json::from_value(arguments) {
    Ok(input) => input,
    Err(why) => return refusal(&format!("the arguments of {ENTRY} do not read: {why}")),
  };

  if input.chat.trim().is_empty() {
    return refusal("chat is required");
  }

  let service = match Service::of(&session.data_root()) {
    Ok(service) => service,
    Err(why) => return refusal(&why),
  };

  match asked(&service, &input).await {
    Ok(said) => text(&said),
    Err(why) => refusal(&why),
  }
}

/// One read of the provider's `/messages` route, answered as the lines a person reads.
async fn asked(service: &Service, input: &Input) -> Result<String, String> {
  let mut url = reqwest::Url::parse(&format!("http://127.0.0.1:{}/messages", service.port))
    .map_err(|why| format!("the provider's address does not read: {why}"))?;
  let max = input.max.unwrap_or(DEFAULT_MAX).clamp(1, LARGEST_MAX);

  {
    let mut query = url.query_pairs_mut();

    query.append_pair("chat", input.chat.trim());
    query.append_pair("max", &max.to_string());

    if let Some(since) = input.since {
      query.append_pair("since", &since.to_string());
    }
  }

  let answer = reqwest::Client::builder()
    .no_proxy()
    .build()
    .map_err(|why| format!("the provider could not be called: {why}"))?
    .get(url)
    .bearer_auth(&service.token)
    .send()
    .await
    .map_err(|why| format!("the provider did not answer: {why}"))?;
  let status = answer.status().as_u16();
  let bytes = answer.bytes().await.map_err(|why| format!("the provider answered unreadably: {why}"))?;
  let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);

  if !(200..300).contains(&status) {
    let why = body.get("error").and_then(Value::as_str).unwrap_or("no reason given");

    return Err(format!("the provider refused the read ({status}): {why}"));
  }

  if body.get("candidates").is_some() {
    return candidates_text(&body);
  }

  messages_text(&body)
}

/// The messages of an answer, newest first, each with its moment, sender and words or placeholder; a count
/// first.
fn messages_text(body: &Value) -> Result<String, String> {
  let messages = body
    .get("messages")
    .and_then(Value::as_array)
    .ok_or_else(|| "the provider answered no messages".to_owned())?;
  let mut lines = vec![format!(
    "{} message{}, newest first:",
    messages.len(),
    if messages.len() == 1 { "" } else { "s" }
  )];

  lines.extend(messages.iter().map(message_line));

  Ok(lines.join("\n"))
}

/// One message as a person reads it: the moment in epoch milliseconds, the sender's name and number, and the
/// words or the placeholder its kind stands under, as they are.
pub fn message_line(message: &Value) -> String {
  let sent_at = message.get("sentAt").and_then(Value::as_i64).unwrap_or_default();
  let name = message.get("senderName").and_then(Value::as_str).unwrap_or("unknown");
  let sender = match message.get("senderId").and_then(Value::as_i64) {
    Some(id) => format!("{name} ({id})"),
    None => name.to_owned(),
  };
  let text = message.get("text").and_then(Value::as_str).unwrap_or_default();

  format!("{sent_at} {sender}: {text}")
}

/// The chats and contacts a name could mean, each by its number and name, so the owner picks.
fn candidates_text(body: &Value) -> Result<String, String> {
  let candidates = body
    .get("candidates")
    .and_then(Value::as_array)
    .ok_or_else(|| "the provider answered no candidates".to_owned())?;

  if candidates.is_empty() {
    return Ok("no chat or contact is named".to_owned());
  }

  let mut lines = vec!["no chat is read from that name alone; pick one by its number:".to_owned()];

  for candidate in candidates {
    let chat_id = candidate.get("chatId").and_then(Value::as_i64).unwrap_or_default();
    let name = candidate.get("name").and_then(Value::as_str).unwrap_or_default();
    let kind = candidate.get("kind").and_then(Value::as_str).unwrap_or_default();

    lines.push(format!("{chat_id} {name} ({kind})"));
  }

  Ok(lines.join("\n"))
}

/// One request answered: a call of this tool's entry, or the protocol's other kinds.
pub async fn answer(handed: &Value) -> Result<Value, String> {
  if handed.get("protocol").and_then(Value::as_u64) != Some(PROTOCOL) {
    return Err(format!("the call does not read: it speaks no protocol {PROTOCOL}"));
  }

  match handed.get("kind").and_then(Value::as_str) {
    Some("call") => {}
    Some("claim") => return Ok(json!({ "arguments": null })),
    Some("resources_list") => return Ok(json!({ "resources": [] })),
    Some("resources_read") => return Ok(json!({ "error": "this executable serves no resources" })),
    _ => return Err("the call does not read: it names no kind this executable answers".to_owned()),
  }

  let entry = handed.get("entry").and_then(Value::as_str).unwrap_or_default();

  if entry != ENTRY {
    return Ok(refusal(&format!("this executable holds no entry {entry}")));
  }

  let arguments = handed.get("arguments").cloned().unwrap_or_else(|| json!({}));
  let record = handed.get("session").cloned().unwrap_or(Value::Null);
  let session = Session::of(&record);

  Ok(call(&session, arguments).await)
}
