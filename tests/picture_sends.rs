//! The bot account's pictures and albums (docs/telegram.md, 「Sends」), sent to a stand-in of the Bot API and
//! named on a stand-in of the web: what went to Telegram — a picture by its address, or uploaded by its name,
//! its type and what it is — and what each send answered. Under `/hidden/` lies a page Telegram cannot fetch
//! for itself, which only an upload reaches.

mod support;

use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::PathBuf;

use bmo_chat::{AlbumItem, ChatRefusal, ChatSend, ChatSendResult, MissedPicture, MissedPictureReason};
use bmo_telegram::policy::POLICY;
use bmo_telegram::send::Sending;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use serde_json::{Value, json};
use support::speaking::{Asked, BotSends, ROOM, refusal, send, speaker, web};

fn encoded(picture: &DynamicImage, format: ImageFormat) -> Vec<u8> {
  let mut out = Cursor::new(Vec::new());

  picture.write_to(&mut out, format).unwrap();
  out.into_inner()
}

fn opaque() -> Vec<u8> {
  encoded(
    &DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, Rgb([192, 48, 48]))),
    ImageFormat::Jpeg,
  )
}

fn clear() -> Vec<u8> {
  encoded(
    &DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 0]))),
    ImageFormat::Png,
  )
}

fn solid() -> DynamicImage {
  DynamicImage::ImageRgb8(RgbImage::from_pixel(6, 4, Rgb([30, 140, 60])))
}

fn half() -> DynamicImage {
  DynamicImage::ImageRgba8(RgbaImage::from_pixel(6, 4, Rgba([30, 140, 60, 128])))
}

// Every other kind of picture a page serves, each named by the type it is served as.
fn kinds() -> BTreeMap<String, (Vec<u8>, String)> {
  let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="8"><rect x="0" y="0" width="6" height="8" fill="#1e8c3c"/></svg>"##;

  [
    ("solid.png", encoded(&solid(), ImageFormat::Png), "image/png"),
    ("half.webp", encoded(&half(), ImageFormat::WebP), "image/webp"),
    ("half.avif", encoded(&half(), ImageFormat::Avif), "image/avif"),
    ("solid.gif", encoded(&solid(), ImageFormat::Gif), "image/gif"),
    ("half.tiff", encoded(&half(), ImageFormat::Tiff), "image/tiff"),
    ("shape.svg", svg.to_vec(), "image/svg+xml"),
  ]
  .into_iter()
  .map(|(name, bytes, kind)| (name.to_owned(), (bytes, kind.to_owned())))
  .collect()
}

struct Web {
  base: String,
  pages: BTreeMap<String, (Vec<u8>, String)>,
}

impl Web {
  async fn new() -> Self {
    let mut pages = kinds();

    pages.insert("red.jpg".to_owned(), (opaque(), "image/jpeg".to_owned()));
    pages.insert("clear.png".to_owned(), (clear(), "image/png".to_owned()));
    pages.insert("page.html".to_owned(), (b"<p>no picture</p>".to_vec(), "text/html".to_owned()));

    Self {
      base: web(pages.clone()).await,
      pages,
    }
  }

  fn at(&self, path: &str) -> String {
    format!("{}{path}", self.base)
  }
}

// Telegram as it stands to a picture: one under `/hidden/` it cannot fetch by its address.
fn hidden(asked: &Asked) -> Option<Value> {
  let named = asked.body["photo"].as_str().into_iter().chain(
    asked.body["media"]
      .as_array()
      .into_iter()
      .flatten()
      .filter_map(|one| one["media"].as_str()),
  );
  let mut named = named;

  (!asked.uploaded && named.any(|address| address.contains("/hidden/")))
    .then(|| refusal(400, "Bad Request: wrong file identifier/HTTP URL specified"))
}

fn photo(address: &str, caption: Option<&str>) -> ChatSend {
  ChatSend::Photo {
    image_url: address.to_owned(),
    caption: caption.map(str::to_owned),
    reply_to: None,
  }
}

fn album(addresses: &[String], caption: Option<&str>) -> ChatSend {
  ChatSend::Album {
    items: addresses
      .iter()
      .map(|address| AlbumItem::Address {
        address: address.clone(),
      })
      .collect(),
    caption: caption.map(str::to_owned),
    reply_to: None,
  }
}

fn none_went(missed: Vec<MissedPicture>) -> ChatSendResult {
  ChatSendResult::Refused {
    refusal: ChatRefusal::NoPictureSent {
      label: "Telegram".to_owned(),
      missed,
    },
    gone: false,
  }
}

fn missed(address: &str, reason: MissedPictureReason) -> MissedPicture {
  MissedPicture {
    address: address.to_owned(),
    reason,
  }
}

#[tokio::test]
async fn a_picture_goes_by_its_address_with_its_caption_as_html() {
  let web = Web::new().await;
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let red = web.at("/pics/red.jpg");

  assert_eq!(
    send(&speaker, photo(&red, Some("**red** photo"))).await,
    Ok(ChatSendResult::Sent {
      message_id: Some(500),
      missed: Vec::new()
    })
  );
  assert_eq!(
    bot.bodies(),
    [json!({ "chat_id": ROOM, "photo": red, "caption": "<b>red</b> photo", "parse_mode": "HTML" })]
  );
}

#[tokio::test]
async fn one_telegram_cannot_fetch_goes_up_redrawn_on_white_and_a_file_under_the_picture_root_goes_up_only()
{
  let web = Web::new().await;
  let bot = BotSends::new();
  let root = std::env::temp_dir().join(format!("telegram-picture-root-{}", std::process::id()));
  let kept: PathBuf = root.join("kept").join("clear.png");

  std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
  std::fs::write(&kept, clear()).unwrap();

  let speaker = speaker(
    bot.clone(),
    Sending {
      picture_root: Some(root.clone()),
      ..Sending::default()
    },
  )
  .await;

  bot.answering(hidden);
  send(&speaker, photo(&web.at("/hidden/clear.png"), None)).await.unwrap();
  assert_eq!(bot.methods(), ["sendPhoto", "sendPhoto"]);

  let upload = bot.bodies()[1]["photo"].clone();

  assert_eq!(upload["type"], "image/jpeg");

  bot.answering(hidden);
  send(&speaker, photo(kept.to_str().unwrap(), Some("drawing"))).await.unwrap();
  assert_eq!(
    bot.bodies(),
    [json!({ "chat_id": ROOM.to_string(), "caption": "drawing", "photo": upload })]
  );

  std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn every_kind_of_picture_a_page_serves_goes_up_a_png_with_no_transparency_as_it_came() {
  let web = Web::new().await;
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;

  for name in kinds().keys() {
    bot.answering(hidden);
    send(&speaker, photo(&web.at(&format!("/hidden/{name}")), None)).await.unwrap();
    assert_eq!(bot.methods(), ["sendPhoto", "sendPhoto"], "{name}");

    let up = bot.asked()[1].files["photo"].clone();
    let given = &web.pages[name].0;

    if name == "solid.png" {
      assert_eq!(&up.bytes, given, "a PNG with no transparency goes up as it came");
      continue;
    }

    let redrawn = image::load_from_memory(&up.bytes).unwrap();

    assert_eq!(image::guess_format(&up.bytes).unwrap(), ImageFormat::Jpeg, "{name}");

    if name == "shape.svg" {
      assert_eq!((redrawn.width(), redrawn.height()), (12, 8));
    } else {
      assert_eq!((redrawn.width(), redrawn.height()), (6, 4), "{name}");
    }
  }
}

#[tokio::test]
async fn a_name_no_room_can_be_sent_is_refused_before_anything_goes_and_one_refused_every_way_falls_back() {
  let web = Web::new().await;
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;

  for (address, reason) in [
    ("picture.png", MissedPictureReason::NotAnAddress),
    ("/nowhere/picture.png", MissedPictureReason::MissingFile),
  ] {
    assert_eq!(send(&speaker, photo(address, None)).await, Ok(none_went(vec![missed(address, reason)])));
  }

  assert!(bot.methods().is_empty());

  let red = web.at("/pics/red.jpg");

  bot.answering(|asked| {
    (asked.method != "sendMessage").then(|| refusal(400, "Bad Request: PHOTO_INVALID_DIMENSIONS"))
  });

  let refused = send(&speaker, photo(&red, Some("photo"))).await;

  assert_eq!(bot.methods(), ["sendPhoto", "sendPhoto", "sendMessage"]);
  assert_eq!(bot.bodies()[2]["text"], format!("photo\n{red}"));
  assert_eq!(
    refused,
    Ok(none_went(vec![missed(
      &red,
      POLICY.picture_failed(Some("Bad Request: PHOTO_INVALID_DIMENSIONS"))
    )]))
  );
}

#[tokio::test]
async fn an_album_goes_by_address_as_one_album_its_caption_on_its_first_picture() {
  let web = Web::new().await;
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let addresses = [web.at("/pics/red.jpg"), web.at("/pics/clear.png"), web.at("/pics/red.jpg?2")];

  assert_eq!(
    send(&speaker, album(&addresses, Some("three"))).await,
    Ok(ChatSendResult::Sent {
      message_id: Some(500),
      missed: Vec::new()
    })
  );
  assert_eq!(bot.methods(), ["sendMediaGroup"]);
  assert_eq!(
    bot.bodies()[0]["media"],
    json!([
      { "type": "photo", "media": addresses[0], "caption": "three" },
      { "type": "photo", "media": addresses[1] },
      { "type": "photo", "media": addresses[2] },
    ])
  );
}

#[tokio::test]
async fn an_album_goes_up_where_telegram_cannot_fetch_it_leaving_out_what_is_no_picture_and_one_alone_goes_as_a_photo()
 {
  let web = Web::new().await;
  let bot = BotSends::new();
  let speaker = speaker(bot.clone(), Sending::default()).await;
  let addresses = ["/hidden/red.jpg", "/hidden/clear.png", "/hidden/page.html", "/hidden/gone.png"]
    .map(|path| web.at(path));

  bot.answering(hidden);

  assert_eq!(
    send(&speaker, album(&addresses, Some("**upload**"))).await,
    Ok(ChatSendResult::Sent {
      message_id: Some(500),
      missed: addresses[2..]
        .iter()
        .map(|address| missed(address, POLICY.picture_left_out()))
        .collect()
    })
  );
  assert_eq!(bot.methods(), ["sendMediaGroup", "sendMediaGroup"]);
  assert_eq!(bot.bodies()[1]["media"][0]["caption"], "<b>upload</b>");

  bot.answering(hidden);

  let alone = send(&speaker, album(&["picture.png".to_owned(), web.at("/pics/red.jpg")], None)).await;

  assert_eq!(bot.methods(), ["sendPhoto"]);
  assert_eq!(
    alone,
    Ok(ChatSendResult::Sent {
      message_id: Some(500),
      missed: vec![missed("picture.png", MissedPictureReason::NotAnAddress)]
    })
  );
}
