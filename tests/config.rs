//! The configuration the deployment's environment file supplies (docs/boundary-api.md, 「Configuration」): the
//! service starts on a configuration whole, and the check names only the required values the environment leaves
//! out — a value that is given is never reported as missing.

use std::path::PathBuf;

use bmo_telegram::config::Config;

/// A configuration with every required value given.
fn whole() -> Config {
  Config {
    bot_token: "a-bot-token".to_owned(),
    owner_id: 7_000_000_002,
    api: "https://api.telegram.org".to_owned(),
    port: 3220,
    token: "a-service-token".to_owned(),
    receiver_url: "http://localhost:3210".to_owned(),
    receiver_token: "a-receiver-token".to_owned(),
    reader_port: 3221,
    data: PathBuf::from("/tmp/bmo-telegram-check"),
    picture_root: None,
    sticker_set: Some("a-sticker-set".to_owned()),
    avatar_prefix: None,
  }
}

#[test]
fn a_configuration_whole_is_started_without_complaint() {
  assert!(whole().missing().is_empty());
}

#[test]
fn only_the_required_values_the_environment_leaves_out_are_named() {
  let mut left_out = whole();
  left_out.owner_id = 0;
  left_out.token = "".to_owned();
  left_out.data = PathBuf::from("");

  assert_eq!(left_out.missing(), ["BMO_OWNER_ID", "BMO_TELEGRAM_TOKEN", "BMO_TELEGRAM_DATA"],);
}
