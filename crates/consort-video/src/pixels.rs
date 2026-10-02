// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning whatever a camera hands over into the one layout a call accepts.
//!
//! Arithmetic over bytes, with no device behind it, so it is tested as data.
//! Both paths land on limited-range BT.601, which is what libwebrtc encodes.

use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

/// What a camera is handing over.
///
/// The two a V4L2 webcam reliably offers. H.264 is also common and is not
/// here: decoding it would mean a second codec to carry, and the SFU would
/// re-encode anyway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    /// Packed 4:2:2, four bytes per two pixels, `Y0 U Y1 V`.
    Yuyv,
    /// One baseline JPEG per frame.
    Mjpeg,
}

impl PixelFormat {
    /// The V4L2 `FourCC` this is, for matching what a device offers.
    pub fn fourcc(self) -> [u8; 4] {
        match self {
            Self::Yuyv => *b"YUYV",
            Self::Mjpeg => *b"MJPG",
        }
    }

    /// The format this `FourCC` names, where it is one this can read.
    pub fn from_fourcc(fourcc: &[u8; 4]) -> Option<Self> {
        [Self::Yuyv, Self::Mjpeg]
            .into_iter()
            .find(|format| &format.fourcc() == fourcc)
    }
}

impl std::fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yuyv => write!(f, "YUYV"),
            Self::Mjpeg => write!(f, "MJPEG"),
        }
    }
}

/// One frame, planar I420 with tight strides.
///
/// Tight, so the plane lengths are the whole contract: the transport reads
/// `width.div_ceil(2)` by `height.div_ceil(2)` out of each chroma plane and
/// refuses a buffer that cannot supply it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// Luma, `width * height`.
    pub y: Vec<u8>,
    /// Blue-difference chroma, one per 2x2 block.
    pub u: Vec<u8>,
    /// Red-difference chroma, one per 2x2 block.
    pub v: Vec<u8>,
    /// When the device captured this, in microseconds on a monotonic clock.
    ///
    /// Filled in by whatever read the frame, not by [`decode`], which only
    /// knows about pixels. libwebrtc paces and orders frames by this, so a
    /// constant here makes the encoder's rate control guess.
    pub timestamp_us: i64,
}

/// The smallest side a thumbnail can have, so its chroma planes are exact.
const SMALLEST: u32 = 2;

impl Picture {
    /// A copy sampled down to fit inside `max_width` by `max_height`.
    ///
    /// Nearest neighbour, because the one reader is a self view a couple of
    /// hundred pixels wide and the cost is paid on the capture thread. A frame
    /// already inside the bound is copied at the size it is.
    pub fn thumbnail(&self, max_width: u32, max_height: u32) -> Self {
        let (width, height) = fitted(self.width, self.height, max_width, max_height);
        let (w, h) = (width as usize, height as usize);
        let (source_w, source_h) = (self.width as usize, self.height as usize);
        let (chroma_w, chroma_h) = (w.div_ceil(2), h.div_ceil(2));
        let (source_chroma_w, source_chroma_h) = (source_w.div_ceil(2), source_h.div_ceil(2));

        let mut y = vec![0u8; w * h];
        for row in 0..h {
            let from = row * source_h / h;
            for column in 0..w {
                y[row * w + column] = self.y[from * source_w + column * source_w / w];
            }
        }

        let mut u = vec![0u8; chroma_w * chroma_h];
        let mut v = vec![0u8; chroma_w * chroma_h];
        for row in 0..chroma_h {
            let from = row * source_chroma_h / chroma_h;
            for column in 0..chroma_w {
                let at = from * source_chroma_w + column * source_chroma_w / chroma_w;
                u[row * chroma_w + column] = self.u[at];
                v[row * chroma_w + column] = self.v[at];
            }
        }

        Self {
            width,
            height,
            y,
            u,
            v,
            timestamp_us: self.timestamp_us,
        }
    }
}

/// The largest even size inside the bound that keeps `width` by `height`'s shape.
///
/// Rounded down to even and never below [`SMALLEST`], so a chroma plane is a
/// whole number of 2x2 blocks whatever was asked for.
fn fitted(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    let scaled =
        |side: u64, by: u64, over: u64| u32::try_from(side * by / over).unwrap_or(u32::MAX) & !1;
    let even = |side: u32| (side & !1).max(SMALLEST);

    if width <= max_width && height <= max_height {
        return (even(width), even(height));
    }

    // Whichever bound binds harder, compared as one fraction rather than two
    // divisions so a narrow frame is not rounded to nothing.
    let (w, h) = (u64::from(width), u64::from(height));
    if w * u64::from(max_height) > h * u64::from(max_width) {
        (even(max_width), even(scaled(h, u64::from(max_width), w)))
    } else {
        (even(scaled(w, u64::from(max_height), h)), even(max_height))
    }
}

/// Convert planar I420 to packed RGB, three tight bytes a pixel.
///
/// The inverse of [`luma`], [`blue_difference`] and [`red_difference`]: limited
/// range BT.601, so 16 comes back black and 235 comes back white. For putting a
/// frame in front of somebody, which wants RGB whatever carries it.
pub fn to_rgb(picture: &Picture) -> Vec<u8> {
    let (w, h) = (picture.width as usize, picture.height as usize);
    let chroma_w = w.div_ceil(2);
    let mut rgb = vec![0u8; w * h * 3];

    for row in 0..h {
        for column in 0..w {
            let chroma = (row / 2) * chroma_w + column / 2;
            let (r, g, b) = colour(
                picture.y[row * w + column],
                picture.u[chroma],
                picture.v[chroma],
            );
            let at = (row * w + column) * 3;
            rgb[at] = r;
            rgb[at + 1] = g;
            rgb[at + 2] = b;
        }
    }

    rgb
}

/// One pixel back out of limited-range BT.601.
fn colour(y: u8, u: u8, v: u8) -> (u8, u8, u8) {
    let luma = 298 * (i32::from(y) - 16);
    let (blue, red) = (i32::from(u) - 128, i32::from(v) - 128);

    (
        clamp(luma + 409 * red + 128),
        clamp(luma - 100 * blue - 208 * red + 128),
        clamp(luma + 516 * blue + 128),
    )
}

/// One fixed-point channel, back into a byte.
fn clamp(value: i32) -> u8 {
    (value >> 8).clamp(0, 255) as u8
}

/// Why a frame could not be made sense of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    /// The negotiated format has no pixels in it.
    Empty,
    /// A YUYV macropixel is two pixels wide, so an odd width cannot be packed.
    OddWidth { width: u32 },
    /// Fewer bytes than the frame needs.
    Short {
        wanted: usize,
        got: usize,
        format: PixelFormat,
    },
    /// The JPEG decoder refused the frame.
    Jpeg(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "the camera negotiated a frame with no pixels in it"),
            Self::OddWidth { width } => {
                write!(f, "a YUYV frame cannot be {width} pixels wide")
            }
            Self::Short {
                wanted,
                got,
                format,
            } => write!(f, "a {format} frame wants {wanted} bytes and got {got}"),
            Self::Jpeg(message) => write!(f, "the frame was not a usable JPEG: {message}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Read one frame of `format` into I420.
///
/// `width` and `height` are what the device negotiated. MJPEG carries its own
/// size and that one wins: see [`from_mjpeg`].
pub fn decode(
    format: PixelFormat,
    width: u32,
    height: u32,
    bytes: &[u8],
) -> Result<Picture, FrameError> {
    match format {
        PixelFormat::Yuyv => from_yuyv(width, height, bytes),
        PixelFormat::Mjpeg => from_mjpeg(bytes),
    }
}

/// Unpack YUYV, averaging each pair of chroma rows.
///
/// Averaging rather than dropping the odd row, which is the cheaper conversion
/// and throws away half the colour detail it was handed for nothing.
fn from_yuyv(width: u32, height: u32, bytes: &[u8]) -> Result<Picture, FrameError> {
    if width == 0 || height == 0 {
        return Err(FrameError::Empty);
    }
    if !width.is_multiple_of(2) {
        return Err(FrameError::OddWidth { width });
    }

    let (w, h) = (width as usize, height as usize);
    let stride = w * 2;
    let wanted = stride * h;
    // Not equality. Some drivers report the buffer allocation rather than the
    // frame, and refusing those refuses every frame that camera ever sends.
    if bytes.len() < wanted {
        return Err(FrameError::Short {
            wanted,
            got: bytes.len(),
            format: PixelFormat::Yuyv,
        });
    }

    let chroma_w = w / 2;
    let chroma_h = h.div_ceil(2);
    let mut y = vec![0u8; w * h];
    let mut u = vec![0u8; chroma_w * chroma_h];
    let mut v = vec![0u8; chroma_w * chroma_h];

    for row in 0..h {
        let source = &bytes[row * stride..row * stride + stride];
        let luma = &mut y[row * w..row * w + w];
        for (pair, macropixel) in source.as_chunks::<4>().0.iter().enumerate() {
            luma[pair * 2] = macropixel[0];
            luma[pair * 2 + 1] = macropixel[2];
        }
    }

    for block in 0..chroma_h {
        let top = block * 2;
        // An odd height leaves the last block one row deep, so it averages
        // with itself rather than reading the row after the frame.
        let bottom = (top + 1).min(h - 1);
        let above = &bytes[top * stride..top * stride + stride];
        let below = &bytes[bottom * stride..bottom * stride + stride];
        for column in 0..chroma_w {
            let at = column * 4;
            u[block * chroma_w + column] = mean(above[at + 1], below[at + 1]);
            v[block * chroma_w + column] = mean(above[at + 3], below[at + 3]);
        }
    }

    Ok(Picture {
        width,
        height,
        y,
        u,
        v,
        timestamp_us: 0,
    })
}

/// Decode one MJPEG frame and convert it to I420.
///
/// The size comes from the JPEG rather than from the negotiated format: an
/// MJPEG camera may send a smaller picture than the format it agreed to, and
/// trusting the claim over the picture reads past the decoded buffer.
fn from_mjpeg(bytes: &[u8]) -> Result<Picture, FrameError> {
    if bytes.is_empty() {
        return Err(FrameError::Short {
            wanted: 1,
            got: 0,
            format: PixelFormat::Mjpeg,
        });
    }

    // Asked for explicitly although it is also this decoder's default, because
    // the colour conversion below reads three bytes per pixel and that is the
    // thing being relied on. Nothing observable changes if it is removed, so
    // the length check further down is what would actually catch a default
    // that moved.
    let options = DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    let rgb = decoder
        .decode()
        .map_err(|error| FrameError::Jpeg(error.to_string()))?;
    let info = decoder
        .info()
        .ok_or_else(|| FrameError::Jpeg("the frame carried no frame header".to_owned()))?;

    let (w, h) = (usize::from(info.width), usize::from(info.height));
    if w == 0 || h == 0 {
        return Err(FrameError::Empty);
    }
    // Unreachable with this decoder, which honours the RGB request even for a
    // one-component JPEG. Kept because the alternative if that ever stops being
    // true is an index out of bounds on the capture thread, and a dropped frame
    // with a reason beats a camera that silently stops.
    if rgb.len() < w * h * 3 {
        return Err(FrameError::Jpeg(format!(
            "{w}x{h} needs {} bytes of RGB and the decoder produced {}",
            w * h * 3,
            rgb.len()
        )));
    }

    Ok(from_rgb(w, h, &rgb))
}

/// Convert packed RGB to I420, chroma from the mean of each 2x2 block.
///
/// Averaging the block's colour once beats converting four pixels and
/// averaging the results: it is three quarters of the arithmetic and does not
/// round four times.
fn from_rgb(w: usize, h: usize, rgb: &[u8]) -> Picture {
    let chroma_w = w.div_ceil(2);
    let chroma_h = h.div_ceil(2);
    let mut y = vec![0u8; w * h];
    let mut u = vec![0u8; chroma_w * chroma_h];
    let mut v = vec![0u8; chroma_w * chroma_h];

    for row in 0..h {
        for column in 0..w {
            let at = (row * w + column) * 3;
            y[row * w + column] = luma(rgb[at], rgb[at + 1], rgb[at + 2]);
        }
    }

    for block_row in 0..chroma_h {
        for block_column in 0..chroma_w {
            let mut totals = [0u32; 3];
            let mut counted = 0u32;
            for row in block_row * 2..(block_row * 2 + 2).min(h) {
                for column in block_column * 2..(block_column * 2 + 2).min(w) {
                    let at = (row * w + column) * 3;
                    totals[0] += u32::from(rgb[at]);
                    totals[1] += u32::from(rgb[at + 1]);
                    totals[2] += u32::from(rgb[at + 2]);
                    counted += 1;
                }
            }
            let [r, g, b] = totals.map(|total| (total / counted) as u8);
            let at = block_row * chroma_w + block_column;
            u[at] = blue_difference(r, g, b);
            v[at] = red_difference(r, g, b);
        }
    }

    Picture {
        width: w as u32,
        height: h as u32,
        y,
        u,
        v,
        timestamp_us: 0,
    }
}

/// Rounded mean of two chroma samples.
fn mean(a: u8, b: u8) -> u8 {
    (u16::from(a) + u16::from(b)).div_ceil(2) as u8
}

/// BT.601 luma, limited range: black is 16 and white is 235.
fn luma(r: u8, g: u8, b: u8) -> u8 {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8
}

/// BT.601 Cb, limited range, centred on 128.
fn blue_difference(r: u8, g: u8, b: u8) -> u8 {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128) as u8
}

/// BT.601 Cr, limited range, centred on 128.
fn red_difference(r: u8, g: u8, b: u8) -> u8 {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128) as u8
}
