//! A picture known by its first bytes, its transparency, redrawn for an upload and kept inside its size.

use std::io::Cursor;

use image::{DynamicImage, ImageFormat, RgbImage};
use telegram_picture::{Keeping, for_upload, inside, is_jpeg, is_transparent, kept, raster_type};

fn png(picture: &DynamicImage) -> Vec<u8> {
  let mut out = Cursor::new(Vec::new());

  picture.write_to(&mut out, ImageFormat::Png).unwrap();
  out.into_inner()
}

fn half_clear() -> DynamicImage {
  DynamicImage::ImageRgba8(image::RgbaImage::from_fn(4, 2, |x, _| {
    image::Rgba([0, 0, 0, if x < 2 { 0 } else { 255 }])
  }))
}

fn kept_small_jpeg() -> Vec<u8> {
  kept(
    vec![0xff, 0xd8, 0xff, 1],
    Keeping {
      threshold_bytes: 10,
      max_dimension: 150,
      quality: 80,
    },
  )
}

#[test]
fn a_picture_is_known_by_its_first_bytes() {
  assert_eq!(raster_type(&[0xff, 0xd8, 0xff, 0]), Some("image/jpeg"));
  assert_eq!(raster_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
  assert_eq!(raster_type(b"\0\0\0\x1cftypavif"), Some("image/avif"));
  assert_eq!(raster_type(b"hello"), None);
}

#[test]
fn transparency_is_any_pixel_less_than_opaque() {
  let opaque = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])));

  assert!(is_transparent(&png(&half_clear())).unwrap());
  assert!(!is_transparent(&png(&opaque)).unwrap());
  assert!(is_transparent(b"no picture").is_err());
}

#[test]
fn a_transparent_picture_goes_up_on_white_and_an_opaque_png_as_it_came() {
  let (redrawn, kind) = for_upload(png(&half_clear()), "image/png", 90).unwrap();
  let read = image::load_from_memory(&redrawn).unwrap().to_rgb8();

  assert_eq!(kind, "image/jpeg");
  assert!(read.get_pixel(0, 0).0.iter().all(|channel| *channel > 240));
  assert!(read.get_pixel(3, 0).0.iter().all(|channel| *channel < 15));
  assert!(for_upload(b"no picture".to_vec(), "image/webp", 90).is_none());
}

#[test]
fn a_large_picture_is_kept_inside_its_largest_side() {
  let wide = DynamicImage::ImageRgb8(RgbImage::from_pixel(300, 100, image::Rgb([200, 10, 10])));
  let kept = kept(
    png(&wide),
    Keeping {
      threshold_bytes: 10,
      max_dimension: 150,
      quality: 80,
    },
  );

  assert!(is_jpeg(&kept));
  assert_eq!(image::load_from_memory(&kept).unwrap().width(), 150);
  assert_eq!(inside(3000, 2000, 1568), (1568, 1045));
  assert_eq!(kept_small_jpeg(), vec![0xff, 0xd8, 0xff, 1]);
}
