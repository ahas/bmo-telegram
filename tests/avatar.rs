//! The avatar's prefixes (docs/telegram.md, 「The avatar」): a prefix a send named is learnt and kept, read back
//! after a restart, and the longest a line begins with is taken off whole, the agent's name with its emoji.

use bmo_telegram::avatar::{AvatarLines, without_prefix};

const OWNER: i64 = 7_000_000_002;

#[test]
fn a_name_tag_a_send_named_is_read_back_whole_after_a_restart_too() {
  let dir = tempfile::tempdir().unwrap();
  let file = dir.path().join("avatar-prefixes.txt");
  let avatar = AvatarLines::new(OWNER, None).kept_in(file.clone());

  avatar.learn("\u{1F916} Agent B ");
  avatar.learn("\u{1F916} Agent B ");

  let words = "\u{1F916} Agent B hello there";
  let prefix = avatar.prefix_of(words).unwrap();

  assert_eq!(prefix, "\u{1F916} Agent B");
  assert_eq!(without_prefix(&prefix, words), "hello there");

  // Learnt once, kept once, and known again by an avatar started over the same file.
  assert_eq!(std::fs::read_to_string(&file).unwrap(), "\u{1F916} Agent B\n");
  let restarted = AvatarLines::new(OWNER, None).kept_in(file);
  assert_eq!(restarted.prefix_of(words).as_deref(), Some("\u{1F916} Agent B"));

  // A line behind the policy's own mark alone still reads as the avatar's, and one behind none does not.
  assert_eq!(restarted.prefix_of("\u{1F916} plain").as_deref(), Some("\u{1F916} "));
  assert_eq!(restarted.prefix_of("the owner's own words"), None);
}
