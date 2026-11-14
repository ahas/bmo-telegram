//! The form a text or a caption goes out in on Telegram (docs/telegram.md, 「Sends」): the markdown divided into
//! the parts one message holds, each read into its HTML and its plain text by the `telegram-html` dialect, and
//! folded where the send asks for a fold and the text is long enough for one (docs/telegram.md, 「The fold」).
//! Whether a fold may be made is decided upstream; the dialect measures the text and carries the fold out. The
//! avatar's words go out behind its prefix, each part short enough to hold it (docs/telegram.md, 「The avatar」).

use telegram_dialect::format::{escape_html, markup_fault};

use crate::avatar::without_prefix;
use telegram_dialect::js::units;
use telegram_dialect::markdown::{Converted, convert_markdown, folded_markup, markdown_parts};
use telegram_dialect::telegram::TEXT_LIMIT;

/// One message's worth of words: the HTML it is sent as, where it carries marks the plain text does not and
/// they are sound, and the plain text it is sent as otherwise or where Telegram refuses the markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
  pub markup: Option<String>,
  pub plain: String,
}

// The markup a part goes out as: the fold where one is asked for and the text is long enough for one, the
// conversion where it says more than the plain text, and none where neither is sound.
fn markup_of(converted: &Converted, fold: bool) -> Option<String> {
  let folded = fold.then(|| folded_markup(converted)).flatten();
  let chosen = folded.or_else(|| converted.formatted.then(|| converted.markup.clone()));

  chosen.filter(|chosen| markup_fault(chosen).is_none())
}

/// A text's markdown in the parts it goes out in, folded where `fold` asks for it.
pub fn text_parts(markdown: &str, fold: bool) -> Vec<Part> {
  parts_within(markdown, fold, TEXT_LIMIT)
}

// A text's markdown in parts of at most `limit` units each.
fn parts_within(markdown: &str, fold: bool, limit: usize) -> Vec<Part> {
  markdown_parts(markdown, limit)
    .iter()
    .map(|source| {
      let converted = convert_markdown(source);

      Part {
        markup: markup_of(&converted, fold),
        plain: converted.plain,
      }
    })
    .collect()
}

/// A caption's markdown as it goes under a picture: never folded and never divided.
pub fn caption(markdown: &str) -> Part {
  let converted = convert_markdown(markdown);

  Part {
    markup: markup_of(&converted, false),
    plain: converted.plain,
  }
}

/// A part behind the prefix: before the plain words as it stands, and before the markup escaped.
pub fn behind(prefix: &str, part: Part) -> Part {
  Part {
    markup: part.markup.map(|markup| format!("{}{markup}", escape_html(prefix))),
    plain: format!("{prefix}{}", part.plain),
  }
}

/// The avatar's markdown in the parts it goes out in, each behind the prefix and short enough to hold it; words
/// already behind the prefix are not put behind it twice.
pub fn avatar_text_parts(prefix: &str, markdown: &str, fold: bool) -> Vec<Part> {
  let limit = TEXT_LIMIT.saturating_sub(units(prefix)).max(1);

  parts_within(without_prefix(prefix, markdown), fold, limit)
    .into_iter()
    .map(|part| behind(prefix, part))
    .collect()
}

/// The avatar's caption: the caption's markdown behind the prefix, and the prefix alone where there is none.
pub fn avatar_caption(prefix: &str, markdown: &str) -> Part {
  behind(prefix, caption(without_prefix(prefix, markdown)))
}
