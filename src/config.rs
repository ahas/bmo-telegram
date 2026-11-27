//! The service's configuration, read from the environment the deployment's env file supplies
//! (docs/boundary-api.md, 「Configuration」). The reader in `reader/` reads the same names; the values that
//! name no person and no address have defaults, the credentials do not.

use std::path::PathBuf;

/// The loopback port the service listens on, and the one the reader listens on, where the environment names
/// neither.
const SERVICE_PORT: u16 = 3220;
const READER_PORT: u16 = 3221;

/// The Bot API's address, where the environment names no other.
const BOT_API: &str = "https://api.telegram.org";

/// The value a variable gives, trimmed; null where it is unset or empty.
fn given(name: &str) -> Option<String> {
  let value = std::env::var(name).ok();

  match value.as_ref().map(|value| value.trim()) {
    Some(trimmed) if !trimmed.is_empty() => Some(trimmed.to_owned()),
    _ => None,
  }
}

/// A loopback port a variable names, or the one the provider uses where it names none or an unusable one.
fn port(name: &str, otherwise: u16) -> u16 {
  given(name).map_or_else(|| otherwise, |value| value.parse().unwrap_or(otherwise))
}

/// What the service stands on.
pub struct Config {
  /// The bot account's token; the provider reports itself unconfigured without it and still runs.
  pub bot_token: String,
  /// The owner's Telegram number, which is also the number of the owner's one-to-one with the bot.
  pub owner_id: i64,
  /// The Bot API's base address.
  pub api: String,
  /// The loopback port the service listens on.
  pub port: u16,
  /// The bearer every route of the service and of the reader takes.
  pub token: String,
  /// The receiver's base address.
  pub receiver_url: String,
  /// The token the receiver's routes take.
  pub receiver_token: String,
  /// The loopback port the reader listens on.
  pub reader_port: u16,
  /// The provider's own data folder: the profile pictures, the record of what has been seen, the session lock.
  pub data: PathBuf,
  /// The folder a picture named by a file path may be read from.
  pub picture_root: Option<PathBuf>,
  /// The sticker set a `sticker` send picks from.
  pub sticker_set: Option<String>,
  /// The prefix the avatar's words carry where a send names none, kept as written, its trailing space with it.
  pub avatar_prefix: Option<String>,
  /// Each supervisor's own bot token, by the supervisor's name (docs/telegram.md, 「A bot for each supervisor」).
  pub supervisor_bots: std::collections::BTreeMap<String, String>,
}

/// The supervisors' bot tokens a file holds, one line a supervisor: its name before the first `=` and its token
/// verbatim after it; none where the file is not named or does not read.
pub fn supervisor_bots(file: Option<&str>) -> std::collections::BTreeMap<String, String> {
  let Some(text) = file.and_then(|file| std::fs::read_to_string(file).ok()) else {
    return Default::default();
  };

  text
    .lines()
    .filter_map(|line| line.split_once('='))
    .map(|(name, token)| (name.trim().to_owned(), token.to_owned()))
    .filter(|(name, token)| !name.is_empty() && !name.starts_with('#') && !token.trim().is_empty())
    .collect()
}

impl Config {
  /// The whole configuration as the environment gives it.
  pub fn from_environment() -> Self {
    Self {
      bot_token: given("BMO_BOT_TOKEN").unwrap_or_default(),
      owner_id: given("BMO_OWNER_ID").map_or(0, |value| value.parse().unwrap_or(0)),
      api: given("BMO_TELEGRAM_API").unwrap_or_else(|| BOT_API.to_owned()),
      port: port("BMO_TELEGRAM_PORT", SERVICE_PORT),
      token: given("BMO_TELEGRAM_TOKEN").unwrap_or_default(),
      receiver_url: given("BMO_TELEGRAM_RECEIVER_URL").unwrap_or_default(),
      receiver_token: given("BMO_TELEGRAM_RECEIVER_TOKEN").unwrap_or_default(),
      reader_port: port("BMO_TELEGRAM_READER_PORT", READER_PORT),
      data: given("BMO_TELEGRAM_DATA").map(PathBuf::from).unwrap_or_default(),
      picture_root: given("BMO_TELEGRAM_PICTURE_ROOT").map(PathBuf::from),
      sticker_set: given("BMO_TELEGRAM_STICKER_SET"),
      avatar_prefix: std::env::var("BMO_TELEGRAM_AVATAR_PREFIX")
        .ok()
        .filter(|prefix| !prefix.trim().is_empty()),
      supervisor_bots: supervisor_bots(given("BMO_TELEGRAM_SUPERVISOR_BOTS").as_deref()),
    }
  }

  /// The required values the environment does not give, each named by its variable.
  pub fn missing(&self) -> Vec<String> {
    let required = [
      ("BMO_OWNER_ID", self.owner_id != 0),
      ("BMO_TELEGRAM_TOKEN", !self.token.is_empty()),
      ("BMO_TELEGRAM_RECEIVER_URL", !self.receiver_url.is_empty()),
      ("BMO_TELEGRAM_RECEIVER_TOKEN", !self.receiver_token.is_empty()),
      ("BMO_TELEGRAM_DATA", !self.data.is_empty()),
    ];
    let mut missing = vec![];

    for (name, given) in required {
      if !given {
        missing.push(name.to_owned());
      }
    }

    missing
  }
}
