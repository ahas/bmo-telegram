//! What a message that is not plain text is handed over as (docs/telegram.md, 「Reading a room」): a short
//! placeholder naming the kind and carrying what a reader could act on — coordinates, a name and number, a
//! poll's question and options, who joined or left — followed by the caption where there is one. The
//! placeholders are the policy's. Nothing here fetches anything; a file's content is gathered in
//! `crate::gather`.

use serde::Deserialize;
use serde_json::Value;

use crate::policy::{Kinds, POLICY, fill};

/// A person as the Bot API names one.
#[derive(Clone, Debug, Deserialize)]
pub struct User {
  pub id: i64,
  #[serde(default)]
  pub first_name: String,
  pub last_name: Option<String>,
  pub username: Option<String>,
}

impl User {
  /// The name a person goes by: the full name, or the handle, or the number.
  pub fn name(&self) -> String {
    let full = [Some(self.first_name.as_str()), self.last_name.as_deref()]
      .into_iter()
      .flatten()
      .collect::<Vec<_>>()
      .join(" ");

    if full.is_empty() {
      self.username.clone().unwrap_or_else(|| self.id.to_string())
    } else {
      full
    }
  }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Chat {
  pub id: i64,
  pub title: Option<String>,
}

/// Where a forwarded message came from.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Origin {
  User {
    sender_user: User,
  },
  HiddenUser {
    sender_user_name: String,
  },
  Chat {
    sender_chat: Chat,
  },
  Channel {
    chat: Chat,
  },
  #[serde(other)]
  Other,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PhotoSize {
  pub file_id: String,
  pub file_size: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Document {
  pub file_id: String,
  pub file_name: Option<String>,
  pub mime_type: Option<String>,
  pub file_size: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Timed {
  pub duration: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Audio {
  pub duration: Option<i64>,
  pub title: Option<String>,
  pub performer: Option<String>,
  pub file_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Location {
  pub latitude: f64,
  pub longitude: f64,
  pub live_period: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Venue {
  pub title: String,
  pub address: String,
  pub location: Location,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Contact {
  #[serde(default)]
  pub first_name: String,
  pub last_name: Option<String>,
  pub phone_number: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PollOption {
  pub text: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Poll {
  pub question: String,
  pub options: Vec<PollOption>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Dice {
  pub emoji: String,
  pub value: i64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Sticker {
  pub emoji: Option<String>,
  pub set_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pinned {
  pub message_id: i64,
  pub text: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Replied {
  pub message_id: i64,
  pub from: Option<User>,
}

/// One message as the Bot API delivers it, with what the provider reads of it.
#[derive(Clone, Debug, Deserialize)]
pub struct Message {
  pub message_id: i64,
  pub date: i64,
  pub chat: Chat,
  /// The album it went out in, as the platform names the messages of one.
  pub media_group_id: Option<String>,
  pub from: Option<User>,
  pub sender_chat: Option<Chat>,
  pub text: Option<String>,
  pub caption: Option<String>,
  pub reply_to_message: Option<Box<Replied>>,
  pub forward_origin: Option<Origin>,
  pub photo: Option<Vec<PhotoSize>>,
  pub document: Option<Document>,
  pub audio: Option<Audio>,
  pub voice: Option<Timed>,
  pub video: Option<Timed>,
  pub video_note: Option<Timed>,
  pub animation: Option<Timed>,
  pub location: Option<Location>,
  pub venue: Option<Venue>,
  pub contact: Option<Contact>,
  pub poll: Option<Poll>,
  pub dice: Option<Dice>,
  pub sticker: Option<Sticker>,
  pub story: Option<Value>,
  pub paid_media: Option<Value>,
  pub giveaway: Option<Value>,
  pub new_chat_members: Option<Vec<User>>,
  pub left_chat_member: Option<User>,
  pub pinned_message: Option<Pinned>,
  pub new_chat_title: Option<String>,
  pub new_chat_photo: Option<Value>,
  pub delete_chat_photo: Option<Value>,
  pub video_chat_started: Option<Value>,
  pub video_chat_ended: Option<Timed>,
}

/// A length of time as a placeholder shows it, after a space: minutes and two-digit seconds.
pub fn seconds(total: Option<i64>) -> String {
  total.map_or_else(String::new, |total| format!(" {}:{:02}", total / 60, total % 60))
}

fn kinds() -> &'static Kinds {
  &POLICY.kinds
}

/// Where a forwarded message came from, as the heading put in front of its words.
pub fn forwarded_from(origin: Option<&Origin>) -> String {
  match origin {
    None => String::new(),
    Some(Origin::User { sender_user }) => fill(&kinds().forwarded, &[("from", &sender_user.name())]),
    Some(Origin::HiddenUser { sender_user_name }) => {
      fill(&kinds().forwarded, &[("from", sender_user_name)])
    }
    Some(Origin::Chat { sender_chat }) => fill(
      &kinds().forwarded,
      &[("from", sender_chat.title.as_deref().unwrap_or(&kinds().forwarded_chat))],
    ),
    Some(Origin::Channel { chat }) => {
      fill(&kinds().forwarded, &[("from", chat.title.as_deref().unwrap_or_default())])
    }
    Some(Origin::Other) => kinds().forwarded_bare.clone(),
  }
}

/// The text a message of any kind is handed over as, or none for a kind that carries nothing worth keeping.
pub fn describe(message: &Message) -> Option<String> {
  let caption = message.caption.as_ref().map_or_else(String::new, |caption| format!(" {caption}"));

  if let Some(text) = &message.text {
    return Some(text.clone());
  }
  if message.photo.is_some() {
    return Some(fill(&kinds().photo, &[("caption", &caption)]));
  }
  if let Some(Location {
    latitude,
    longitude,
    live_period,
  }) = &message.location
  {
    let live = if live_period.is_some() { kinds().location_live.as_str() } else { "" };

    return Some(fill(
      &kinds().location,
      &[
        ("latitude", &latitude.to_string()),
        ("longitude", &longitude.to_string()),
        ("live", live),
      ],
    ));
  }
  if let Some(venue) = &message.venue {
    return Some(fill(
      &kinds().venue,
      &[
        ("title", &venue.title),
        ("address", &venue.address),
        ("latitude", &venue.location.latitude.to_string()),
        ("longitude", &venue.location.longitude.to_string()),
      ],
    ));
  }
  if let Some(contact) = &message.contact {
    let name = [Some(contact.first_name.as_str()), contact.last_name.as_deref()]
      .into_iter()
      .flatten()
      .collect::<Vec<_>>()
      .join(" ");

    return Some(fill(&kinds().contact, &[("name", &name), ("phone", &contact.phone_number)]));
  }
  if let Some(poll) = &message.poll {
    let options = poll.options.iter().map(|option| option.text.as_str()).collect::<Vec<_>>().join(" / ");

    return Some(fill(&kinds().poll, &[("question", &poll.question), ("options", &options)]));
  }
  if let Some(dice) = &message.dice {
    return Some(fill(&kinds().dice, &[("emoji", &dice.emoji), ("value", &dice.value.to_string())]));
  }
  if let Some(sticker) = &message.sticker {
    let emoji = sticker.emoji.as_ref().map_or_else(String::new, |emoji| format!(" {emoji}"));
    let set = sticker
      .set_name
      .as_ref()
      .map_or_else(String::new, |set| fill(&kinds().sticker_set, &[("set", set)]));

    return Some(fill(&kinds().sticker, &[("emoji", &emoji), ("set", &set)]));
  }
  if let Some(animation) = &message.animation {
    return Some(format!(
      "{}{caption}",
      fill(&kinds().gif, &[("duration", &seconds(animation.duration))])
    ));
  }
  if let Some(video) = &message.video {
    return Some(fill(&kinds().video, &[("duration", &seconds(video.duration)), ("caption", &caption)]));
  }
  if let Some(note) = &message.video_note {
    return Some(fill(&kinds().video_note, &[("duration", &seconds(note.duration))]));
  }
  if let Some(voice) = &message.voice {
    return Some(fill(&kinds().voice, &[("duration", &seconds(voice.duration)), ("caption", &caption)]));
  }
  if let Some(audio) = &message.audio {
    let named = [audio.performer.as_deref(), audio.title.as_deref()]
      .into_iter()
      .flatten()
      .collect::<Vec<_>>()
      .join(" - ");
    let name = if named.is_empty() { audio.file_name.clone().unwrap_or_default() } else { named };
    let name = if name.is_empty() { String::new() } else { format!(" {name}") };

    return Some(fill(
      &kinds().audio,
      &[("name", &name), ("duration", &seconds(audio.duration)), ("caption", &caption)],
    ));
  }
  if let Some(document) = &message.document {
    let name = document.file_name.as_ref().map_or_else(String::new, |name| format!(" {name}"));

    return Some(fill(&kinds().document, &[("name", &name), ("caption", &caption)]));
  }
  if message.story.is_some() {
    return Some(kinds().story.clone());
  }
  if message.paid_media.is_some() {
    return Some(fill(&kinds().paid_media, &[("caption", &caption)]));
  }
  if message.giveaway.is_some() {
    return Some(kinds().giveaway.clone());
  }
  if let Some(members) = &message.new_chat_members {
    return Some(fill(
      &kinds().joined,
      &[("members", &members.iter().map(User::name).collect::<Vec<_>>().join(", "))],
    ));
  }
  if let Some(member) = &message.left_chat_member {
    return Some(fill(&kinds().left, &[("member", &member.name())]));
  }
  if let Some(pinned) = &message.pinned_message {
    let text = pinned
      .text
      .clone()
      .unwrap_or_else(|| fill(&kinds().pinned_message, &[("id", &pinned.message_id.to_string())]));

    return Some(fill(&kinds().pinned, &[("text", &text)]));
  }
  if let Some(title) = &message.new_chat_title {
    return Some(fill(&kinds().renamed, &[("title", title)]));
  }
  if message.new_chat_photo.is_some() {
    return Some(kinds().photo_changed.clone());
  }
  if message.delete_chat_photo.is_some() {
    return Some(kinds().photo_deleted.clone());
  }
  if message.video_chat_started.is_some() {
    return Some(kinds().call_started.clone());
  }
  if let Some(ended) = &message.video_chat_ended {
    return Some(fill(&kinds().call_ended, &[("duration", &seconds(ended.duration))]));
  }

  None
}

const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "gif"];

/// Whether a file sent as a document is a picture: its type says so, or, where it names none or only a
/// generic one, the common picture endings of its name.
pub fn is_image_file(mime_type: Option<&str>, file_name: Option<&str>) -> bool {
  let mime = mime_type.map(str::to_lowercase);

  if let Some(mime) = mime.filter(|mime| !mime.is_empty() && mime != "application/octet-stream") {
    return mime.starts_with("image/");
  }

  file_name
    .and_then(|name| name.rfind('.').filter(|dot| *dot > 0).map(|dot| name[dot + 1..].to_lowercase()))
    .is_some_and(|extension| IMAGE_EXTENSIONS.contains(&extension.as_str()))
}
