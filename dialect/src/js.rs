//! A text read with JavaScript's character classes: its white space, its trim, its line terminators and its
//! lengths in UTF-16 units, which is how Telegram counts a message's length.

/// JavaScript's `\s`, as a regular expression's class body.
pub const SPACE: &str =
  r"\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";

/// JavaScript's `.`: any character but a line terminator, as a regular expression's class.
pub const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

/// Whether a character is JavaScript's white space.
pub fn is_space(character: char) -> bool {
  matches!(
    character,
    '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
      ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
  )
}

/// A text with JavaScript's white space taken off both ends.
pub fn trim(text: &str) -> &str {
  text.trim_matches(is_space)
}

/// A text with JavaScript's white space taken off its end.
pub fn trim_end(text: &str) -> &str {
  text.trim_end_matches(is_space)
}

/// A text's length in UTF-16 units.
pub fn units(text: &str) -> usize {
  text.encode_utf16().count()
}

/// Characters' length in UTF-16 units.
pub fn units_of(characters: &[char]) -> usize {
  characters.iter().map(|character| character.len_utf16()).sum()
}

/// The characters of a text, which every reading here walks by index.
pub fn chars(text: &str) -> Vec<char> {
  text.chars().collect()
}

/// Characters put back into a text.
pub fn text(characters: &[char]) -> String {
  characters.iter().collect()
}

/// How many characters of a text come within a number of UTF-16 units.
pub fn within(characters: &[char], limit: usize) -> usize {
  let mut spent = 0;

  characters
    .iter()
    .take_while(|character| {
      spent += character.len_utf16();
      spent <= limit
    })
    .count()
}

/// The first place a run of characters stands at or after `from`.
pub fn find(characters: &[char], needle: &[char], from: usize) -> Option<usize> {
  (from..=characters.len().saturating_sub(needle.len()))
    .find(|&at| characters.len() >= needle.len() && characters[at..at + needle.len()] == *needle)
}

/// The last place a run of characters starts at or before `before`.
pub fn rfind(characters: &[char], needle: &[char], before: usize) -> Option<usize> {
  if characters.len() < needle.len() {
    return None;
  }

  (0..=before.min(characters.len() - needle.len()))
    .rev()
    .find(|&at| characters[at..at + needle.len()] == *needle)
}
