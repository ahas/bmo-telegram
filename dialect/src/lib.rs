//! The `telegram-html` dialect (docs/telegram.md, 「The dialect」): the agent's markdown read line for line into
//! Telegram's HTML and plain text, the structure check every markup is read by, the fold, and the division into
//! messages. Every length here is counted as Telegram counts it, in UTF-16 units.

pub mod format;
pub mod js;
pub mod markdown;
pub mod telegram;
pub mod web_address;

use bmo_chat::{ChatDialect, ChatDialectLimits, ChatMarkup};

/// The most pictures Telegram puts in one album.
pub const ALBUM_LIMIT: usize = 10;

/// The largest file the Bot API hands a bot, in bytes.
pub const FILE_LIMIT: u64 = 20_000_000;

/// The sentence a writer in a room of this dialect is told.
pub const WRITING: &str = "This room shows formatting. Write markdown as the answer-shape rulebook describes: \
the send path turns it into this platform's own markup and escapes what has to be escaped, so never write a tag \
yourself. A long answer may be folded, which the formatting gate decides.";

/// Telegram's HTML parse mode, as bmo-chat's contract states a dialect.
#[derive(Debug, Clone, Copy, Default)]
pub struct TelegramHtml;

impl ChatDialect for TelegramHtml {
  fn name(&self) -> &str {
    "telegram-html"
  }

  fn limits(&self) -> ChatDialectLimits {
    ChatDialectLimits {
      text: Some(telegram::TEXT_LIMIT as u64),
      caption: Some(telegram::CAPTION_LIMIT as u64),
      album_photos: Some(ALBUM_LIMIT as u64),
      file_bytes: Some(FILE_LIMIT),
    }
  }

  fn writing(&self) -> &str {
    WRITING
  }

  fn foldable(&self) -> bool {
    true
  }

  fn convert(&self, markdown: &str) -> ChatMarkup {
    let converted = markdown::convert_markdown(markdown);

    ChatMarkup {
      markup: converted.markup,
      plain: converted.plain,
      formatted: converted.formatted,
    }
  }

  fn parts(&self, markdown: &str) -> Vec<String> {
    markdown::telegram_parts(markdown)
  }

  fn fault(&self, markup: &str) -> Option<String> {
    format::markup_fault(markup).map(|fault| format::fault_reason(&fault))
  }

  fn fold(&self, markdown: &str) -> Option<String> {
    markdown::folded_markup(&markdown::convert_markdown(markdown))
  }

  fn visible(&self, markup: &str) -> String {
    format::visible_text(markup)
  }
}
