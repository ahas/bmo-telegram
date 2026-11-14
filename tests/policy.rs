//! The policy file read whole: every key it holds is one the provider reads, every line is filled where it
//! is used, and each reason becomes its bmo-chat refusal code.

use bmo_chat::{ChatCapability, ChatRefusal, MissedPictureReason};
use bmo_telegram::policy::{Gathering, Kinds, Owner, POLICY, Pictures, Platform, Policy, Refusals, fill};
use strum::VariantArray;

#[test]
fn every_key_of_the_file_is_read_and_none_is_empty() {
  let Policy {
    telegram,
    refusals,
    pictures,
    owner,
    gathering,
    kinds,
  } = &*POLICY;
  let Platform {
    label,
    can,
    gone_descriptions,
    unchanged_description,
    caption_refused_descriptions,
  } = telegram;
  let Refusals {
    message_gone,
    sticker_missing,
    sticker_unset,
    mode_down,
    mode_missing,
    owner_verb,
    read_only,
  } = refusals;
  let Pictures {
    mark,
    refused,
    failed,
    left_out,
    upload_quality,
    kept_threshold_bytes,
    kept_max_dimension,
    kept_quality,
  } = pictures;
  let Owner {
    largest_bytes,
    empty_file,
    too_large,
    megabytes,
  } = owner;
  let Gathering {
    file_not_kept,
    seam_body_bytes,
    voice_name,
    sound_name,
    video_name,
    video_note_name,
    gif_name,
    video_fallback_extension,
    audio_extensions,
    video_extensions,
  } = gathering;
  let Kinds {
    forwarded,
    forwarded_bare,
    forwarded_chat,
    photo,
    location,
    location_live,
    venue,
    contact,
    poll,
    dice,
    sticker,
    sticker_set,
    video,
    video_note,
    gif,
    voice,
    audio,
    document,
    unnamed_file,
    story,
    paid_media,
    giveaway,
    joined,
    left,
    pinned,
    pinned_message,
    renamed,
    photo_changed,
    photo_deleted,
    call_started,
    call_ended,
  } = kinds;
  let lines = [
    label,
    unchanged_description,
    message_gone,
    sticker_missing,
    sticker_unset,
    mode_down,
    mode_missing,
    owner_verb,
    read_only,
    mark,
    refused,
    failed,
    left_out,
    empty_file,
    too_large,
    megabytes,
    file_not_kept,
    voice_name,
    sound_name,
    video_name,
    video_note_name,
    gif_name,
    video_fallback_extension,
    forwarded,
    forwarded_bare,
    forwarded_chat,
    photo,
    location,
    location_live,
    venue,
    contact,
    poll,
    dice,
    sticker,
    sticker_set,
    video,
    video_note,
    gif,
    voice,
    audio,
    document,
    unnamed_file,
    story,
    paid_media,
    giveaway,
    joined,
    left,
    pinned,
    pinned_message,
    renamed,
    photo_changed,
    photo_deleted,
    call_started,
    call_ended,
  ];

  assert!(lines.iter().all(|line| !line.trim().is_empty()), "a line of the policy is empty");
  assert_eq!(label, "Telegram");
  assert_eq!(can.len(), ChatCapability::VARIANTS.len(), "every capability is declared");
  assert_eq!(gone_descriptions.len(), 3);
  assert_eq!(caption_refused_descriptions.len(), 2);
  assert_eq!((*upload_quality, *kept_quality), (90, 80));
  assert_eq!((*kept_threshold_bytes, *kept_max_dimension), (4_000_000, 1568));
  assert_eq!(*largest_bytes, 52_428_800);
  assert_eq!(*seam_body_bytes, 50_331_648);
  assert_eq!(audio_extensions.len(), 10);
  assert_eq!(video_extensions.get("video/quicktime").map(String::as_str), Some(".mov"));
}

#[test]
fn a_key_the_policy_does_not_name_is_refused() {
  let text = include_str!("../policy/telegram.toml").replace("[telegram]", "[telegram]\nalbum = 1");

  assert!(Policy::read(&text).is_err());
}

#[test]
fn a_placeholder_is_filled_and_an_optional_part_leaves_nothing_behind() {
  let kinds = &POLICY.kinds;
  let set = fill(&kinds.sticker_set, &[("set", "Animals")]);

  assert_eq!(
    fill(&kinds.sticker, &[("emoji", " \u{1f600}"), ("set", &set)]),
    "[Sticker \u{1f600} (Animals)]"
  );
  assert_eq!(fill(&kinds.photo, &[("caption", "")]), "[Photo]");
  assert_eq!(fill(&kinds.forwarded, &[("from", "Far")]), "[Forwarded from Far] ");
  assert_eq!(fill("{unknown} stays", &[]), "{unknown} stays");
}

#[test]
fn each_reason_is_a_platform_refusal_under_the_label() {
  let gone = POLICY.message_gone("message to delete not found");

  assert_eq!(gone.code(), bmo_chat::ErrorCode::Gone);
  assert_eq!(
    gone.to_string(),
    "Telegram: Telegram can no longer act on that message: message to delete not found"
  );
  assert_eq!(
    POLICY.sticker_missing("Animals", "\u{1f600}"),
    ChatRefusal::Platform {
      label: "Telegram".to_owned(),
      detail: "The sticker set Animals has no sticker for \u{1f600}.".to_owned(),
    }
  );
  assert_eq!(POLICY.sticker_unset().to_string(), "Telegram: No sticker set is configured.");
  assert_eq!(
    POLICY.mode_down("human-reader", Some("the account is not connected")).to_string(),
    "Telegram: Telegram's human-reader mode is not running: the account is not connected."
  );
  assert_eq!(
    POLICY.mode_down("human-avatar", None).to_string(),
    "Telegram: Telegram's human-avatar mode is not running: Telegram does not read by that mode."
  );
}

#[test]
fn a_picture_that_did_not_go_carries_the_platform_s_reason() {
  assert_eq!(
    POLICY.picture_failed(Some("timed out")),
    MissedPictureReason::Platform {
      detail: "It could not be sent: timed out".to_owned()
    }
  );
  assert_eq!(
    POLICY.picture_failed(None),
    MissedPictureReason::Platform {
      detail: "Telegram did not take it.".to_owned()
    }
  );
  assert_eq!(POLICY.picture_left_out().code(), "platform");
}
