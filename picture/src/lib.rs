//! A picture read and redrawn for Telegram (docs/telegram.md, 「Reading a room」 and 「Sends」). It reads JPEG,
//! PNG, WebP, GIF, TIFF, AVIF and SVG and nothing else, tells whether any pixel is less than opaque, and
//! redraws a picture laid on white as JPEG: upright, for an upload Telegram takes as a photo, and brought
//! inside a size, as a picture is kept for the receiver.

mod avif;
mod svg;

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage};

/// Why bytes are no picture this library reads.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct NotAPicture(pub String);

fn failed(why: impl std::fmt::Display) -> NotAPicture {
  NotAPicture(why.to_string())
}

/// The white a picture is laid on wherever its transparency would be lost.
const GROUND: u8 = 255;

/// The type a picture's own first bytes say it is, of those a page serves.
pub fn raster_type(bytes: &[u8]) -> Option<&'static str> {
  let starts = |values: &[u8]| bytes.starts_with(values);
  let text = |offset: usize, value: &str| bytes.get(offset..offset + value.len()) == Some(value.as_bytes());

  if starts(&[0xff, 0xd8, 0xff]) {
    Some("image/jpeg")
  } else if starts(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
    Some("image/png")
  } else if text(0, "GIF87a") || text(0, "GIF89a") {
    Some("image/gif")
  } else if text(0, "RIFF") && text(8, "WEBP") {
    Some("image/webp")
  } else if text(4, "ftyp") && (text(8, "avif") || text(8, "avis")) {
    Some("image/avif")
  } else if starts(&[0x00, 0x00, 0x01, 0x00]) && bytes.len() > 6 {
    Some("image/x-icon")
  } else {
    None
  }
}

/// Whether bytes are a JPEG, read off their first bytes: the one picture never decoded to ask about.
pub fn is_jpeg(bytes: &[u8]) -> bool {
  bytes.starts_with(&[0xff, 0xd8, 0xff])
}

// The picture bytes hold, and whether they ask to be turned to stand upright.
fn decode(bytes: &[u8], upright: bool) -> Result<DynamicImage, NotAPicture> {
  match image::guess_format(bytes) {
    Ok(
      format @ (ImageFormat::Jpeg
      | ImageFormat::Png
      | ImageFormat::Gif
      | ImageFormat::WebP
      | ImageFormat::Tiff),
    ) => {
      let mut decoder =
        ImageReader::with_format(Cursor::new(bytes), format).into_decoder().map_err(failed)?;
      let orientation = decoder.orientation().map_err(failed)?;
      let mut picture = DynamicImage::from_decoder(decoder).map_err(failed)?;

      if upright {
        picture.apply_orientation(orientation);
      }

      Ok(picture)
    }
    _ if bytes.get(4..8) == Some(b"ftyp") => avif::decode(bytes),
    _ if svg::is_svg(bytes) => svg::decode(bytes),
    _ => Err(failed("unsupported image format")),
  }
}

/// Whether any pixel of the picture is less than opaque; a picture with no alpha channel is opaque.
pub fn is_transparent(bytes: &[u8]) -> Result<bool, NotAPicture> {
  let picture = decode(bytes, false)?;

  Ok(picture.color().has_alpha() && picture.to_rgba16().pixels().any(|pixel| pixel.0[3] < u16::MAX))
}

// The picture laid on white: each pixel's colour weighed by its own alpha against the ground, so an
// anti-aliased edge keeps its softness against the white.
fn on_white(picture: &DynamicImage) -> RgbImage {
  let rgba = picture.to_rgba8();

  RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
    let [red, green, blue, alpha] = rgba.get_pixel(x, y).0;
    let laid = |channel: u8| {
      ((u32::from(channel) * u32::from(alpha) + u32::from(GROUND) * (255 - u32::from(alpha)) + 127) / 255)
        as u8
    };

    image::Rgb([laid(red), laid(green), laid(blue)])
  })
}

fn jpeg(picture: &RgbImage, quality: u8) -> Result<Vec<u8>, NotAPicture> {
  let mut out = Vec::new();

  JpegEncoder::new_with_quality(&mut out, quality).encode_image(picture).map_err(failed)?;

  Ok(out)
}

/// The picture upright, laid on white and written as JPEG at a quality on the encoder's scale of 1 to 100.
pub fn jpeg_on_white(bytes: &[u8], quality: u8) -> Result<Vec<u8>, NotAPicture> {
  jpeg(&on_white(&decode(bytes, true)?), quality)
}

/// A picture's bytes as Telegram takes a photo, with their type: a JPEG, or a PNG with no transparency, as it
/// came under the type it was served as; anything else upright on white as JPEG. None where they are no
/// picture.
pub fn for_upload(bytes: Vec<u8>, served: &str, quality: u8) -> Option<(Vec<u8>, String)> {
  if served == "image/jpeg" {
    return Some((bytes, served.to_owned()));
  }

  if served == "image/png" && !is_transparent(&bytes).ok()? {
    return Some((bytes, served.to_owned()));
  }

  jpeg_on_white(&bytes, quality).ok().map(|redrawn| (redrawn, "image/jpeg".to_owned()))
}

/// How a picture is kept: past how many bytes it is brought down, inside how many pixels, at what quality.
#[derive(Clone, Copy, Debug)]
pub struct Keeping {
  pub threshold_bytes: usize,
  pub max_dimension: u32,
  pub quality: u8,
}

/// The size a picture is brought inside a square of `limit` pixels at, never larger than it stands.
pub fn inside(width: u32, height: u32, limit: u32) -> (u32, u32) {
  if width <= limit && height <= limit {
    return (width, height);
  }

  let scale = f64::from(limit) / f64::from(width.max(height));
  let scaled = |side: u32| ((f64::from(side) * scale).round() as u32).clamp(1, limit);

  (scaled(width), scaled(height))
}

/// A picture as it is kept for its message: a JPEG under the threshold, or any picture under it with no
/// transparency, as it came; any other laid on white, brought inside the largest side and written as JPEG.
/// A picture that cannot be redrawn is kept as it came.
pub fn kept(bytes: Vec<u8>, keeping: Keeping) -> Vec<u8> {
  let large = bytes.len() > keeping.threshold_bytes;

  if !large && is_jpeg(&bytes) {
    return bytes;
  }

  let redrawn = (|| -> Result<Option<Vec<u8>>, NotAPicture> {
    if !large && !is_transparent(&bytes)? {
      return Ok(None);
    }

    // Laid on white before it is brought down, which is the same picture as bringing its premultiplied
    // pixels down and laying them on white after, and keeps a transparent pixel's colour out of the edge.
    let laid = on_white(&decode(&bytes, false)?);
    let (width, height) = inside(laid.width(), laid.height(), keeping.max_dimension);
    let brought = if (width, height) == laid.dimensions() {
      laid
    } else {
      image::imageops::resize(&laid, width, height, FilterType::Lanczos3)
    };

    jpeg(&brought, keeping.quality).map(Some)
  })();

  match redrawn {
    Ok(Some(redrawn)) => redrawn,
    Ok(None) => bytes,
    Err(why) => {
      eprintln!("image redraw failed, keeping the original: {why}");
      bytes
    }
  }
}
