//! What the bot account makes of one message (docs/telegram.md, 「Reading a room」): the words handed over, a
//! picture to gather first, a file to gather after, or nothing. Nothing of what a message means to the receiver
//! is decided here: no allow list, no command, no routing. What is here is what only Telegram's reading can do.

use bmo_chat::{ChatVoice, HandedMessage, InboundChatContext, MessageOrigin, RoomFacts, RoomKind};
use serde_json::{Map, Value};

use crate::VIA;
use crate::kinds::{Message, describe, forwarded_from, is_image_file};
use crate::policy::POLICY;

/// The largest number a room is told apart by without losing a digit.
pub const SAFE_INTEGER: i64 = (1 << 53) - 1;

/// The picture a message carries, as the gathering is asked for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
  pub file_id: String,
  pub file_unique_id: Option<String>,
  pub file_size: Option<i64>,
}

/// What becomes of one new message, read off it alone.
#[derive(Debug)]
pub enum Reading {
  /// Words, or a kind handed over as a placeholder.
  Said(HandedMessage),
  /// A picture: its file gathered first, and the message handed over with it.
  Picture(HandedMessage, Option<Picture>),
  /// A message carrying a file that is not a picture: handed over at once, its file gathered after.
  Attachment(HandedMessage),
  /// A file nobody's words go with — the bot's own, or one of a kind handed over as nothing.
  FileOnly,
  /// Nothing to take.
  Nothing,
}

impl Reading {
  /// The message this reading hands over, where it hands one over.
  pub fn message_mut(&mut self) -> Option<&mut HandedMessage> {
    match self {
      Reading::Said(message) | Reading::Picture(message, _) | Reading::Attachment(message) => Some(message),
      Reading::FileOnly | Reading::Nothing => None,
    }
  }
}

/// What becomes of one edited message.
#[derive(Debug)]
pub struct Edit {
  /// The new words, where the kind keeps any.
  pub said: Option<HandedMessage>,
  /// Whether it now carries a file, to be gathered again.
  pub carries_file: bool,
}

/// The name the platform shows for who said a message.
pub fn sender_name(message: &Message) -> String {
  match &message.from {
    Some(from) => from.name(),
    None => message
      .sender_chat
      .as_ref()
      .and_then(|chat| chat.title.clone())
      .unwrap_or_else(|| "unknown".to_owned()),
  }
}

/// What the provider knows of the room a message came from: Telegram numbers a one-to-one by its person,
/// above zero, and a group, a supergroup or a channel below it.
pub fn room_facts(chat_id: i64, owner_id: i64, me: i64) -> RoomFacts {
  RoomFacts {
    kind: Some(if chat_id == owner_id { RoomKind::Owner } else { RoomKind::Group }),
    identity: Some(me),
    private_to: (chat_id > 0).then_some(chat_id),
  }
}

// What the platform says of a message beyond the message: the room's name as it stands now, the handle of
// whoever spoke, and the author of the message this one answers.
fn context(message: &Message) -> InboundChatContext {
  InboundChatContext {
    room_title: message.chat.title.clone(),
    sender_username: message.from.as_ref().and_then(|from| from.username.clone()),
    reply_to_sender_id: message
      .reply_to_message
      .as_ref()
      .and_then(|replied| replied.from.as_ref())
      .map(|from| from.id),
    ..InboundChatContext::default()
  }
}

fn handed(message: &Message, text: &str, origin: MessageOrigin, owner_id: i64, me: i64) -> HandedMessage {
  HandedMessage {
    origin,
    sender_id: message.from.as_ref().map(|from| from.id),
    sender_name: sender_name(message),
    sent_at: message.date.saturating_mul(1000),
    text: format!("{}{text}", forwarded_from(message.forward_origin.as_ref())),
    line: None,
    reply_to_message_id: message.reply_to_message.as_ref().map(|replied| replied.message_id),
    via: VIA.to_owned(),
    album: None,
    mode: ChatVoice::BotAccount,
    context: context(message),
    room: room_facts(message.chat.id, owner_id, me),
    files: Vec::new(),
    gathering: false,
    gather_failure: None,
  }
}

/// Whether the message carries a file of the kinds kept as files.
pub fn carries_file(message: &Message) -> bool {
  message.animation.is_some()
    || message.video.is_some()
    || message.video_note.is_some()
    || message.document.is_some()
    || message.audio.is_some()
    || message.voice.is_some()
}

fn picture_message(message: &Message, picture: Option<Picture>, owner_id: i64, me: i64) -> Reading {
  let caption = message.caption.clone();
  let mut handed = handed(
    message,
    caption.as_deref().unwrap_or(&POLICY.pictures.mark),
    MessageOrigin::Live,
    owner_id,
    me,
  );

  handed.context.spoken_text = caption;
  handed.context.carries_file = picture.is_some();

  Reading::Picture(handed, picture)
}

/// What becomes of a new message: `me` is the bot's own number, `owner_id` the owner's, and `listened`
/// whether the room is one the receiver listens to at all, which is asked only to keep a file from being
/// fetched for a room nobody listens to.
pub fn read(message: &Message, me: i64, owner_id: i64, listened: bool) -> Reading {
  if let Some(text) = &message.text {
    let mut handed = handed(message, text, MessageOrigin::Live, owner_id, me);

    handed.context.spoken_text = Some(text.clone());

    return Reading::Said(handed);
  }

  if !listened {
    return Reading::Nothing;
  }

  if let Some(photos) = &message.photo {
    let largest = photos.last().map(|photo| Picture {
      file_id: photo.file_id.clone(),
      file_unique_id: photo.file_unique_id.clone(),
      file_size: photo.file_size,
    });

    return picture_message(message, largest, owner_id, me);
  }

  if carries_file(message) {
    if let Some(document) = &message.document {
      let mime = document.mime_type.as_deref();
      let svg = mime.is_some_and(|mime| mime.eq_ignore_ascii_case("image/svg+xml"))
        || document.file_name.as_ref().is_some_and(|name| name.to_lowercase().ends_with(".svg"));

      if !svg && is_image_file(mime, document.file_name.as_deref()) {
        return picture_message(
          message,
          Some(Picture {
            file_id: document.file_id.clone(),
            file_unique_id: document.file_unique_id.clone(),
            file_size: document.file_size,
          }),
          owner_id,
          me,
        );
      }
    }

    let mine = message.from.as_ref().is_some_and(|from| from.id == me);

    return match describe(message) {
      Some(text) if !mine => {
        let mut handed = handed(message, &text, MessageOrigin::Live, owner_id, me);

        handed.context.spoken_text = message.caption.clone();
        handed.context.carries_file = true;

        Reading::Attachment(handed)
      }
      _ => Reading::FileOnly,
    };
  }

  match describe(message) {
    // A service message the bot itself caused — pinning its own board — is nobody's word.
    Some(text) if message.from.as_ref().is_none_or(|from| from.id != me) => {
      Reading::Said(handed(message, &text, MessageOrigin::Live, owner_id, me))
    }
    _ => Reading::Nothing,
  }
}

/// What the receiver keeps of a message as the platform sent it: its words, their formatting, its sticker and
/// the file it carries, in that order, each as Telegram wrote it.
pub fn metadata(raw: &Value) -> Option<Value> {
  let present =
    |keys: &[&str]| keys.iter().find_map(|key| raw.get(*key).filter(|value| !value.is_null())).cloned();
  let mut kept = Map::new();

  kept.insert("text".to_owned(), present(&["text", "caption"]).unwrap_or(Value::Null));
  kept.insert(
    "entities".to_owned(),
    present(&["entities", "caption_entities"]).unwrap_or(Value::Array(Vec::new())),
  );
  kept.insert("sticker".to_owned(), present(&["sticker"]).unwrap_or(Value::Null));
  kept.insert(
    "attachment".to_owned(),
    present(&["audio", "voice", "video", "video_note", "animation", "document"]).unwrap_or(Value::Null),
  );

  raw.is_object().then_some(Value::Object(kept))
}

/// What becomes of an edited message.
pub fn read_edit(message: &Message, raw: &Value, me: i64, owner_id: i64) -> Edit {
  let said = describe(message).map(|text| {
    let mut handed = handed(message, &text, MessageOrigin::Edit, owner_id, me);

    handed.context.metadata = metadata(raw);

    handed
  });

  Edit {
    said,
    carries_file: carries_file(message),
  }
}
