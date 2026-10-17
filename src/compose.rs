//! The form a text or a caption goes out in on Telegram (docs/telegram.md, 「Sends」): the markdown divided into
//! the parts one message holds, each read into its HTML and its plain text by the `telegram-html` dialect, and
//! folded where the send asks for a fold. Whether to fold is decided upstream; the dialect only carries it out.

use telegram_dialect::format::markup_fault;
use telegram_dialect::markdown::{Converted, convert_markdown, folded_markup, telegram_parts};

/// One message's worth of words: the HTML it is sent as, where it carries marks the plain text does not and
/// they are sound, and the plain text it is sent as otherwise or where Telegram refuses the markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
  pub markup: Option<String>,
  pub plain: String,
}

// The markup a part goes out as: the fold where one is asked for and there is one worth making, the
// conversion where it says more than the plain text, and none where neither is sound.
fn markup_of(converted: &Converted, fold: bool) -> Option<String> {
  let folded = fold.then(|| folded_markup(converted)).flatten();
  let chosen = folded.or_else(|| converted.formatted.then(|| converted.markup.clone()));

  chosen.filter(|chosen| markup_fault(chosen).is_none())
}

/// A text's markdown in the parts it goes out in, folded where `fold` asks for it.
pub fn text_parts(markdown: &str, fold: bool) -> Vec<Part> {
  telegram_parts(markdown)
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
