//! The dialect as bmo-chat's contract states it: its declaration and each of its readings wired to the code.

use bmo_chat::ChatDialect;
use telegram_dialect::TelegramHtml;

#[test]
fn the_dialect_declares_telegram_s_limits_and_folds() {
  let declared = TelegramHtml.declaration();

  assert_eq!(declared.name, "telegram-html");
  assert_eq!(declared.limits.text, Some(4096));
  assert_eq!(declared.limits.caption, Some(1024));
  assert_eq!(declared.limits.album_photos, Some(10));
  assert_eq!(declared.limits.file_bytes, Some(20_000_000));
  assert!(declared.foldable);
  assert!(declared.writing.starts_with("This room shows formatting."));
  assert!(
    declared
      .writing
      .contains("folds into an expandable quote only where its send asks for a fold"),
    "the writer is told a fold is asked for, and made only where the text is long"
  );
}

#[test]
fn each_reading_answers_as_the_code_reads_it() {
  let converted = TelegramHtml.convert("**bold** words");

  assert_eq!(converted.markup, "<b>bold</b> words");
  assert_eq!(converted.plain, "bold words");
  assert!(converted.formatted);
  assert_eq!(TelegramHtml.parts("short"), vec!["short"]);
  assert_eq!(TelegramHtml.visible("<b>a &amp; b</b>"), "a & b");
  assert_eq!(TelegramHtml.fault("<b>sound</b>"), None);
  assert_eq!(
    TelegramHtml.fault("<b>open"),
    Some("Structure check: the tag <b> is never closed.".to_owned())
  );
  assert_eq!(TelegramHtml.fold("too little to hide"), None);
}
