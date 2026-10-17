//! An AVIF picture read into pixels: its container by `avif-parse`, its colour and alpha items each one AV1
//! frame decoded by `rav1d` through its dav1d interface, and the frame's YUV turned into RGB by `yuv` with the
//! matrix and range the frame's sequence header names. A frame deeper than eight bits is brought down to
//! eight, since every picture here ends as an eight-bit JPEG.

use std::mem::MaybeUninit;
use std::ptr::{self, NonNull};

use image::{DynamicImage, RgbaImage};
use rav1d::Dav1dResult;
use rav1d::include::dav1d::data::Dav1dData;
use rav1d::include::dav1d::dav1d::{Dav1dContext, Dav1dSettings};
use rav1d::include::dav1d::headers::{
  DAV1D_MC_BT470BG, DAV1D_MC_BT709, DAV1D_MC_BT2020_CL, DAV1D_MC_BT2020_NCL, DAV1D_MC_FCC,
  DAV1D_MC_IDENTITY, DAV1D_MC_SMPTE240, DAV1D_PIXEL_LAYOUT_I400, DAV1D_PIXEL_LAYOUT_I420,
  DAV1D_PIXEL_LAYOUT_I422, DAV1D_PIXEL_LAYOUT_I444,
};
use rav1d::include::dav1d::picture::Dav1dPicture;
use rav1d::src::lib::{
  dav1d_close, dav1d_data_create, dav1d_data_unref, dav1d_default_settings, dav1d_get_picture, dav1d_open,
  dav1d_picture_unref, dav1d_send_data,
};
use yuv::{YuvGrayImage, YuvPlanarImage, YuvRange, YuvStandardMatrix};

use crate::NotAPicture;

fn failed(why: impl std::fmt::Display) -> NotAPicture {
  NotAPicture(format!("AVIF: {why}"))
}

fn checked(result: Dav1dResult) -> Result<(), NotAPicture> {
  if result.0 == 0 {
    Ok(())
  } else {
    Err(failed(format!("the AV1 decoder answered {}", result.0)))
  }
}

// One decoder over one picture's items, closed as it goes.
struct Decoder {
  context: Option<Dav1dContext>,
}

impl Decoder {
  fn new() -> Result<Self, NotAPicture> {
    let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
    let mut context = None;

    // SAFETY: the settings are written whole by the default before they are read, and the context is
    // written by the open before anything reads it.
    unsafe {
      dav1d_default_settings(NonNull::from(&mut settings).cast());

      let settings = settings.assume_init_mut();

      settings.n_threads = std::thread::available_parallelism().map_or(4, |count| count.get()).min(16) as _;
      // A still picture has nothing to pipeline, and its grain is part of what it shows.
      settings.max_frame_delay = 1;
      settings.apply_grain = 1;
      checked(dav1d_open(Some(NonNull::from(&mut context)), Some(NonNull::from(settings))))?;
    }

    Ok(Self { context })
  }

  // The last picture one item decodes to: a layered item's last layer is the whole picture.
  fn frame(&mut self, item: &[u8]) -> Result<Frame, NotAPicture> {
    let context = self.context.ok_or_else(|| failed("the decoder is closed"))?;
    let mut data = Dav1dData::default();
    let mut last: Option<Dav1dPicture> = None;

    // SAFETY: the buffer the data is created with holds `item.len()` bytes and is filled before it is sent;
    // every picture taken out is either kept in `last` or handed back.
    unsafe {
      let buffer = dav1d_data_create(Some(NonNull::from(&mut data)), item.len());
      let Some(buffer) = NonNull::new(buffer) else {
        return Err(failed("no room for the item"));
      };

      ptr::copy_nonoverlapping(item.as_ptr(), buffer.as_ptr(), item.len());

      let sent = loop {
        let result = dav1d_send_data(Some(context), Some(NonNull::from(&mut data)));

        if result.0 == 0 {
          break Ok(());
        }

        let mut pending = Dav1dPicture::default();

        if dav1d_get_picture(Some(context), Some(NonNull::from(&mut pending))).0 != 0 {
          dav1d_picture_unref(Some(NonNull::from(&mut pending)));
          break checked(result);
        }

        if let Some(mut previous) = last.replace(pending) {
          dav1d_picture_unref(Some(NonNull::from(&mut previous)));
        }
      };

      dav1d_data_unref(Some(NonNull::from(&mut data)));
      sent?;

      loop {
        let mut picture = Dav1dPicture::default();
        let result = dav1d_get_picture(Some(context), Some(NonNull::from(&mut picture)));

        if result.0 != 0 {
          dav1d_picture_unref(Some(NonNull::from(&mut picture)));

          return match last {
            Some(picture) => Ok(Frame(picture)),
            None => checked(result).and(Err(failed("no frame decoded"))),
          };
        }

        if let Some(mut previous) = last.replace(picture) {
          dav1d_picture_unref(Some(NonNull::from(&mut previous)));
        }
      }
    }
  }
}

impl Drop for Decoder {
  fn drop(&mut self) {
    // SAFETY: the context came from the open, and is closed once.
    unsafe { dav1d_close(Some(NonNull::from(&mut self.context))) };
  }
}

// One decoded picture, handed back when it is dropped.
struct Frame(Dav1dPicture);

impl Drop for Frame {
  fn drop(&mut self) {
    // SAFETY: the picture came from the decoder and is handed back once.
    unsafe { dav1d_picture_unref(Some(NonNull::from(&mut self.0))) };
  }
}

// How the chroma planes of a frame are subsampled across and down.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
  Gray,
  I420,
  I422,
  I444,
}

impl Frame {
  fn size(&self) -> (u32, u32) {
    (self.0.p.w.max(0) as u32, self.0.p.h.max(0) as u32)
  }

  fn layout(&self) -> Result<Layout, NotAPicture> {
    Ok(match self.0.p.layout {
      DAV1D_PIXEL_LAYOUT_I400 => Layout::Gray,
      DAV1D_PIXEL_LAYOUT_I420 => Layout::I420,
      DAV1D_PIXEL_LAYOUT_I422 => Layout::I422,
      DAV1D_PIXEL_LAYOUT_I444 => Layout::I444,
      _ => return Err(failed("an unknown pixel layout")),
    })
  }

  fn full_range(&self) -> bool {
    // SAFETY: a decoded picture's sequence header lives as long as the picture.
    self.0.seq_hdr.is_some_and(|header| unsafe { header.as_ref() }.color_range != 0)
  }

  // The matrix its YUV was written with: none for an identity matrix, whose planes are G, B and R, and BT.601
  // where the header names none, as libheif reads such a picture.
  fn matrix(&self) -> Option<YuvStandardMatrix> {
    // SAFETY: as above.
    let named = self.0.seq_hdr.map(|header| unsafe { header.as_ref() }.mtrx);

    match named {
      Some(DAV1D_MC_IDENTITY) => None,
      Some(DAV1D_MC_BT709) => Some(YuvStandardMatrix::Bt709),
      Some(DAV1D_MC_FCC) => Some(YuvStandardMatrix::Fcc),
      Some(DAV1D_MC_BT470BG) => Some(YuvStandardMatrix::Bt470_6),
      Some(DAV1D_MC_SMPTE240) => Some(YuvStandardMatrix::Smpte240),
      Some(DAV1D_MC_BT2020_NCL | DAV1D_MC_BT2020_CL) => Some(YuvStandardMatrix::Bt2020),
      _ => Some(YuvStandardMatrix::Bt601),
    }
  }

  // One plane as eight-bit samples, its width and height: the luma plane, or one of the chroma planes as
  // the layout subsamples it.
  fn plane(&self, which: usize) -> Result<(Vec<u8>, u32, u32), NotAPicture> {
    let (width, height) = self.size();
    let (across, down) = match (which, self.layout()?) {
      (0, _) | (_, Layout::I444) => (0, 0),
      (_, Layout::I420) => (1, 1),
      (_, Layout::I422) => (1, 0),
      (_, Layout::Gray) => return Err(failed("a grey picture has no chroma")),
    };
    let width = ((width + across) >> across) as usize;
    let height = ((height + down) >> down) as usize;
    let depth = self.0.p.bpc;
    let wide = depth > 8;
    let unit = if wide { 2 } else { 1 };
    let stride = self.0.stride[usize::from(which != 0)];
    let Some(data) = self.0.data[which] else {
      return Err(failed("a plane is missing"));
    };

    if stride <= 0 || !(stride as usize).is_multiple_of(unit) || (stride as usize) / unit < width {
      return Err(failed("a plane's stride does not hold its row"));
    }

    let stride = stride as usize / unit;
    let mut samples = Vec::with_capacity(width * height);

    for row in 0..height {
      if wide {
        // SAFETY: the decoder lays out whole rows of `stride` samples for every row of the plane.
        let line =
          unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u16>().add(row * stride), width) };
        let shift = (depth - 8) as u32;

        samples.extend(
          line
            .iter()
            .map(|sample| ((u32::from(*sample) + (1 << (shift - 1))) >> shift).min(255) as u8),
        );
      } else {
        // SAFETY: as above, one byte a sample.
        let line =
          unsafe { std::slice::from_raw_parts(data.as_ptr().cast::<u8>().add(row * stride), width) };

        samples.extend_from_slice(line);
      }
    }

    Ok((samples, width as u32, height as u32))
  }

  // The frame as RGBA, opaque.
  fn rgba(&self) -> Result<RgbaImage, NotAPicture> {
    let (width, height) = self.size();
    let range = if self.full_range() { YuvRange::Full } else { YuvRange::Limited };
    let mut out = vec![0u8; width as usize * height as usize * 4];
    let (y, y_width, _) = self.plane(0)?;
    let layout = self.layout()?;

    if layout == Layout::Gray {
      let gray = YuvGrayImage {
        y_plane: &y,
        y_stride: y_width,
        width,
        height,
      };

      yuv::yuv400_to_rgba(&gray, &mut out, width * 4, range, YuvStandardMatrix::Bt601).map_err(failed)?;
    } else {
      let (u, u_width, _) = self.plane(1)?;
      let (v, _, _) = self.plane(2)?;

      match self.matrix() {
        None => {
          // Identity: the planes are G, B and R as they stand, never subsampled.
          for (at, pixel) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            pixel.copy_from_slice(&[v[at], y[at], u[at], 255]);
          }
        }
        Some(matrix) => {
          let planar = YuvPlanarImage {
            y_plane: &y,
            y_stride: y_width,
            u_plane: &u,
            u_stride: u_width,
            v_plane: &v,
            v_stride: u_width,
            width,
            height,
          };
          let converted = match layout {
            Layout::I420 => yuv::yuv420_to_rgba(&planar, &mut out, width * 4, range, matrix),
            Layout::I422 => yuv::yuv422_to_rgba(&planar, &mut out, width * 4, range, matrix),
            _ => yuv::yuv444_to_rgba(&planar, &mut out, width * 4, range, matrix),
          };

          converted.map_err(failed)?;
        }
      }
    }

    RgbaImage::from_raw(width, height, out).ok_or_else(|| failed("the pixels do not fill the picture"))
  }
}

/// The picture an AVIF file holds, with its alpha where it carries one.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage, NotAPicture> {
  let avif = avif_parse::read_avif(&mut &bytes[..]).map_err(failed)?;
  let mut decoder = Decoder::new()?;
  let mut picture = decoder.frame(&avif.primary_item)?.rgba()?;

  if let Some(alpha) = &avif.alpha_item {
    let frame = decoder.frame(alpha)?;
    let (samples, width, height) = frame.plane(0)?;
    let full = frame.full_range();

    if (width, height) != picture.dimensions() {
      return Err(failed("the alpha item is not the picture's size"));
    }

    for (pixel, sample) in picture.pixels_mut().zip(samples) {
      let alpha = if full {
        sample
      } else {
        ((f32::from(sample.clamp(16, 235)) - 16.0) * 255.0 / 219.0).round() as u8
      };

      pixel.0[3] = alpha;

      // A premultiplied picture's colour is divided back out by its alpha.
      if avif.premultiplied_alpha && alpha > 0 && alpha < 255 {
        for channel in &mut pixel.0[..3] {
          *channel = ((f32::from(*channel) * 255.0 / f32::from(alpha)).round()).min(255.0) as u8;
        }
      }
    }
  }

  Ok(DynamicImage::ImageRgba8(picture))
}
