//! An SVG picture drawn into pixels at its own size, as libvips draws one at its default density, with the
//! machine's fonts for its text.

use std::sync::{Arc, LazyLock};

use image::{DynamicImage, RgbaImage};
use resvg::{tiny_skia, usvg};

use crate::NotAPicture;

// The machine's fonts, read once for every picture this process draws.
static FONTS: LazyLock<Arc<usvg::fontdb::Database>> = LazyLock::new(|| {
  let mut fonts = usvg::fontdb::Database::new();

  fonts.load_system_fonts();
  Arc::new(fonts)
});

fn failed(why: impl std::fmt::Display) -> NotAPicture {
  NotAPicture(format!("SVG: {why}"))
}

/// Whether bytes read as an SVG document: text whose opening names an `<svg` element.
pub fn is_svg(bytes: &[u8]) -> bool {
  let head = &bytes[..bytes.len().min(1_000)];

  String::from_utf8_lossy(head).contains("<svg")
}

/// The picture an SVG document draws, its pixels no longer premultiplied by their alpha.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, NotAPicture> {
  let options = usvg::Options {
    fontdb: FONTS.clone(),
    ..usvg::Options::default()
  };
  let tree = usvg::Tree::from_data(bytes, &options).map_err(failed)?;
  let size = tree.size().to_int_size();
  let mut pixmap =
    tiny_skia::Pixmap::new(size.width(), size.height()).ok_or_else(|| failed("an empty picture"))?;

  resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

  let pixels = pixmap.pixels().iter().flat_map(|pixel| {
    let straight = pixel.demultiply();

    [straight.red(), straight.green(), straight.blue(), straight.alpha()]
  });
  let picture = RgbaImage::from_raw(size.width(), size.height(), pixels.collect())
    .ok_or_else(|| failed("the pixels do not fill the picture"))?;

  Ok(DynamicImage::ImageRgba8(picture))
}
