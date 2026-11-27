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
    supervisor_bots: Default::default(),
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

#[test]
fn the_supervisors_bots_are_read_one_line_a_supervisor_with_the_token_verbatim() {
  let dir = tempfile::tempdir().unwrap();
  let file = dir.path().join("bots");

  std::fs::write(&file, "bmo=111:aa=bb\n# a note=ignored\nkakia-chat = 222:cc\nempty=\n=333:dd\n").unwrap();

  let bots = bmo_telegram::config::supervisor_bots(file.to_str());

  assert_eq!(bots.len(), 2);
  assert_eq!(bots["bmo"], "111:aa=bb");
  assert_eq!(bots["kakia-chat"], " 222:cc");
  assert!(bmo_telegram::config::supervisor_bots(None).is_empty());
  assert!(bmo_telegram::config::supervisor_bots(Some("/nowhere/at-all")).is_empty());
}

#[test]
fn a_supervisors_bot_may_be_named_by_the_file_its_token_is_saved_in() {
  let dir = tempfile::tempdir().unwrap();
  let file = dir.path().join("bots");

  std::fs::write(dir.path().join("kakia-token"), "333:from-a-file\n").unwrap();
  std::fs::write(&file, "kakia-chat=kakia-token\nnobody=no-such-file\n").unwrap();

  let bots = bmo_telegram::config::supervisor_bots(file.to_str());

  assert_eq!(bots.len(), 1);
  assert_eq!(bots["kakia-chat"], "333:from-a-file");
}
