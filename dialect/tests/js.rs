//! JavaScript's character classes, as the dialect counts and trims by them.

use telegram_dialect::js::{chars, find, is_space, rfind, trim, units, within};

#[test]
fn white_space_is_javascript_s() {
  assert!(is_space('\u{feff}'));
  assert!(!is_space('\u{85}'));
  assert_eq!(trim("\u{feff} a \u{3000}"), "a");
}

#[test]
fn a_length_is_counted_in_utf16_units() {
  assert_eq!(units("a\u{1f600}"), 3);
  assert_eq!(within(&chars("a\u{1f600}b"), 2), 1);
  assert_eq!(within(&chars("a\u{1f600}b"), 3), 2);
}

#[test]
fn a_run_is_found_from_either_end() {
  let text = chars("a\n\nb\n\n");

  assert_eq!(find(&text, &chars("\n\n"), 0), Some(1));
  assert_eq!(rfind(&text, &chars("\n\n"), 9), Some(4));
  assert_eq!(rfind(&text, &chars("\n\n"), 3), Some(1));
  assert_eq!(find(&text, &chars("x"), 0), None);
}
