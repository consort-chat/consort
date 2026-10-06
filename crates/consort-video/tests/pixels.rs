// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning whatever a camera hands over into the one layout a call accepts.
//!
//! Pure arithmetic over bytes, so every case here is a fixture rather than
//! something that needs the right webcam plugged in. Which is the point: a
//! colour that comes out wrong on a call is nearly impossible to attribute by
//! looking at it, and these are the numbers it would be wrong by.

use std::io::Cursor;

use image::ExtendedColorType;
use image::codecs::jpeg::JpegEncoder;

use consort_video::{FrameError, Picture, PixelFormat, decode, from_bgra, to_rgb};

/// One YUYV macropixel: two pixels sharing a U and a V.
fn macropixel(y0: u8, u: u8, y1: u8, v: u8) -> [u8; 4] {
    [y0, u, y1, v]
}

fn yuyv(width: u32, height: u32, rows: &[Vec<u8>]) -> Result<Picture, FrameError> {
    let bytes: Vec<u8> = rows.concat();
    decode(PixelFormat::Yuyv, width, height, &bytes)
}

/// A baseline JPEG of one flat colour, so the colour tests below carry their
/// own fixture rather than an image file.
fn flat_jpeg(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let pixels: Vec<u8> = (0..(width as usize * height as usize))
        .flat_map(|_| rgb)
        .collect();

    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut Cursor::new(&mut out), 90)
        .encode(&pixels, width, height, ExtendedColorType::Rgb8)
        .expect("the test encoder refused a flat picture");
    out
}

mod yuyv_frames {
    use super::*;

    #[test]
    fn luma_arrives_pixel_for_pixel() {
        let row = macropixel(10, 128, 20, 128).to_vec();

        let picture = yuyv(2, 2, &[row.clone(), row]).unwrap();

        assert_eq!(picture.y, vec![10, 20, 10, 20]);
    }

    #[test]
    fn the_picture_keeps_the_size_it_was_told() {
        let row = macropixel(0, 128, 0, 128).to_vec();

        let picture = yuyv(2, 2, &[row.clone(), row]).unwrap();

        assert_eq!((picture.width, picture.height), (2, 2));
    }

    #[test]
    fn chroma_is_halved_in_both_directions() {
        // 4x2 is two macropixels across and two rows, so one 2x2 block each.
        let row = [macropixel(0, 50, 0, 60), macropixel(0, 70, 0, 80)].concat();

        let picture = yuyv(4, 2, &[row.clone(), row]).unwrap();

        assert_eq!(picture.u.len(), 2, "one U per 2x2 block");
        assert_eq!(picture.v.len(), 2);
    }

    #[test]
    fn two_chroma_rows_are_averaged_rather_than_one_being_dropped() {
        // The regression worth having a number for. Dropping the odd row is
        // the cheaper conversion and loses half the colour detail, which on a
        // webcam looks like nothing in particular rather than like a bug.
        let top = macropixel(0, 0, 0, 0).to_vec();
        let bottom = macropixel(0, 100, 0, 200).to_vec();

        let picture = yuyv(2, 2, &[top, bottom]).unwrap();

        assert_eq!(picture.u, vec![50]);
        assert_eq!(picture.v, vec![100]);
    }

    #[test]
    fn an_odd_last_row_uses_itself_rather_than_reading_past_the_frame() {
        let row = macropixel(0, 10, 0, 20).to_vec();

        let picture = yuyv(2, 3, &[row.clone(), row.clone(), row]).unwrap();

        assert_eq!(picture.u.len(), 2, "three rows is two chroma rows");
        assert_eq!(picture.u, vec![10, 10]);
    }

    #[test]
    fn a_short_buffer_is_refused_rather_than_read_past() {
        let bytes = macropixel(0, 128, 0, 128);

        let refused = decode(PixelFormat::Yuyv, 2, 2, &bytes);

        assert_eq!(
            refused,
            Err(FrameError::Short {
                wanted: 8,
                got: 4,
                format: PixelFormat::Yuyv,
            })
        );
    }

    #[test]
    fn a_buffer_longer_than_the_frame_is_accepted_and_the_tail_ignored() {
        // V4L2 reports the whole buffer length, which for some drivers is the
        // allocation rather than the frame. Refusing those would refuse every
        // frame from that camera.
        let row = macropixel(1, 128, 2, 128).to_vec();
        let padded: Vec<u8> = [row.clone(), row, vec![0xff; 64]].concat();

        let picture = decode(PixelFormat::Yuyv, 2, 2, &padded).unwrap();

        assert_eq!(picture.y, vec![1, 2, 1, 2]);
    }

    #[test]
    fn an_odd_width_is_refused_because_a_macropixel_is_two_pixels_wide() {
        let refused = decode(PixelFormat::Yuyv, 3, 2, &[0; 12]);

        assert_eq!(refused, Err(FrameError::OddWidth { width: 3 }));
    }

    #[test]
    fn a_frame_with_no_pixels_in_it_is_refused() {
        let refused = decode(PixelFormat::Yuyv, 0, 0, &[]);

        assert_eq!(refused, Err(FrameError::Empty));
    }
}

mod nv12_frames {
    use super::*;

    /// A luma plane followed by one row of interleaved `U V` pairs per two
    /// rows of picture, which is the whole of NV12.
    fn nv12(
        width: u32,
        height: u32,
        luma: &[u8],
        chroma: &[[u8; 2]],
    ) -> Result<Picture, FrameError> {
        let bytes: Vec<u8> = [luma, chroma.concat().as_slice()].concat();
        decode(PixelFormat::Nv12, width, height, &bytes)
    }

    #[test]
    fn luma_is_copied_as_it_arrives() {
        let picture = nv12(2, 2, &[1, 2, 3, 4], &[[128, 128]]).unwrap();

        assert_eq!(picture.y, vec![1, 2, 3, 4]);
        assert_eq!((picture.width, picture.height), (2, 2));
    }

    #[test]
    fn the_interleaved_chroma_is_split_into_its_two_planes() {
        // 4x4 is two chroma samples across and two down. Swapping U and V here
        // turns a face blue, which is the one mistake this conversion has room
        // for.
        let luma = [0u8; 16];
        let chroma = [[10, 20], [30, 40], [50, 60], [70, 80]];

        let picture = nv12(4, 4, &luma, &chroma).unwrap();

        assert_eq!(picture.u, vec![10, 30, 50, 70]);
        assert_eq!(picture.v, vec![20, 40, 60, 80]);
    }

    #[test]
    fn a_short_buffer_is_refused_rather_than_read_past() {
        let refused = decode(PixelFormat::Nv12, 2, 2, &[0; 5]);

        assert_eq!(
            refused,
            Err(FrameError::Short {
                wanted: 6,
                got: 5,
                format: PixelFormat::Nv12,
            })
        );
    }

    #[test]
    fn a_buffer_longer_than_the_frame_is_accepted_and_the_tail_ignored() {
        let bytes = [9, 9, 9, 9, 100, 200, 0xff, 0xff, 0xff];

        let picture = decode(PixelFormat::Nv12, 2, 2, &bytes).unwrap();

        assert_eq!(
            (picture.u.as_slice(), picture.v.as_slice()),
            (&[100][..], &[200][..])
        );
    }

    #[test]
    fn an_odd_size_still_fills_every_chroma_sample() {
        // 3x3 rounds up to two chroma samples each way, as every other path
        // here does, so the planes are the length the transport copies.
        let luma = [0u8; 9];
        let chroma = [[1, 2], [3, 4], [5, 6], [7, 8]];

        let picture = nv12(3, 3, &luma, &chroma).unwrap();

        assert_eq!(picture.u, vec![1, 3, 5, 7]);
        assert_eq!(picture.v, vec![2, 4, 6, 8]);
    }

    #[test]
    fn a_frame_with_no_pixels_in_it_is_refused() {
        assert_eq!(decode(PixelFormat::Nv12, 0, 0, &[]), Err(FrameError::Empty));
    }

    #[test]
    fn it_is_not_offered_to_video4linux() {
        // The V4L2 path decodes what `from_fourcc` admits, and nobody has
        // pointed it at an NV12 camera to see whether its strides are tight.
        assert_eq!(PixelFormat::from_fourcc(b"NV12"), None);
    }
}

mod mjpeg_frames {
    use super::*;

    /// Within what a JPEG at quality 90 and a BT.601 round trip can promise.
    fn near(got: u8, wanted: u8, label: &str) {
        let slack = 6;
        assert!(
            got.abs_diff(wanted) <= slack,
            "{label}: got {got}, wanted about {wanted}"
        );
    }

    #[test]
    fn black_lands_on_the_bottom_of_the_studio_range() {
        // 16 rather than 0, because this is what libwebrtc encodes and what
        // every other client decodes: limited-range BT.601. Handing it
        // full-range luma washes a call out at both ends.
        let frame = flat_jpeg(16, 16, [0, 0, 0]);

        let picture = decode(PixelFormat::Mjpeg, 16, 16, &frame).unwrap();

        near(picture.y[0], 16, "luma");
        near(picture.u[0], 128, "U");
        near(picture.v[0], 128, "V");
    }

    #[test]
    fn white_lands_on_the_top_of_the_studio_range() {
        let frame = flat_jpeg(16, 16, [255, 255, 255]);

        let picture = decode(PixelFormat::Mjpeg, 16, 16, &frame).unwrap();

        near(picture.y[0], 235, "luma");
        near(picture.u[0], 128, "U");
        near(picture.v[0], 128, "V");
    }

    #[test]
    fn red_is_not_blue() {
        // The one mistake a flat grey test cannot catch, and the one that
        // actually happens: R and B swapped between the decoder's output and
        // the colour matrix. V is the red-difference channel, so red pushes it
        // up and blue pushes U up.
        let red = decode(PixelFormat::Mjpeg, 16, 16, &flat_jpeg(16, 16, [255, 0, 0])).unwrap();
        let blue = decode(PixelFormat::Mjpeg, 16, 16, &flat_jpeg(16, 16, [0, 0, 255])).unwrap();

        assert!(
            red.v[0] > 200 && red.u[0] < 110,
            "red should be high V and low U, got V={} U={}",
            red.v[0],
            red.u[0]
        );
        assert!(
            blue.u[0] > 200 && blue.v[0] < 130,
            "blue should be high U, got U={} V={}",
            blue.u[0],
            blue.v[0]
        );
    }

    #[test]
    fn the_size_comes_from_the_jpeg_rather_than_from_what_the_driver_claimed() {
        // V4L2 reports the negotiated frame size, and an MJPEG camera is free
        // to send a smaller picture than the format it agreed to. Trusting the
        // claim over the picture means reading past the decoded buffer.
        //
        // Two sizes, because one would also be satisfied by a constant.
        for (width, height) in [(16, 16), (32, 24)] {
            let frame = flat_jpeg(width, height, [0, 0, 0]);

            let picture = decode(PixelFormat::Mjpeg, 640, 480, &frame).unwrap();

            assert_eq!((picture.width, picture.height), (width, height));
            assert_eq!(picture.y.len(), (width * height) as usize);
            assert_eq!(
                picture.u.len(),
                (width.div_ceil(2) * height.div_ceil(2)) as usize
            );
        }
    }

    #[test]
    fn a_grayscale_frame_decodes_to_a_grey_picture_rather_than_being_refused() {
        // An infrared sensor beside a webcam is a real device, and its MJPEG is
        // one component rather than three. What makes this work is the decoder
        // being asked for RGB out: without that it hands back luma, which is a
        // third of the bytes the colour conversion reads.
        let mut grey = Vec::new();
        JpegEncoder::new_with_quality(&mut Cursor::new(&mut grey), 90)
            .encode(&vec![120u8; 16 * 16], 16, 16, ExtendedColorType::L8)
            .expect("the test encoder refused a grey picture");

        let picture = decode(PixelFormat::Mjpeg, 16, 16, &grey).unwrap();

        assert_eq!((picture.width, picture.height), (16, 16));
        near(picture.y[0], 120, "luma");
        near(picture.u[0], 128, "U");
        near(picture.v[0], 128, "V");
    }

    #[test]
    fn something_that_is_not_a_jpeg_is_refused_with_a_reason() {
        let refused = decode(PixelFormat::Mjpeg, 16, 16, b"not a jpeg at all");

        assert!(
            matches!(refused, Err(FrameError::Jpeg(_))),
            "expected a named JPEG failure, got {refused:?}"
        );
    }

    #[test]
    fn an_empty_buffer_is_refused_rather_than_handed_to_the_decoder() {
        let refused = decode(PixelFormat::Mjpeg, 16, 16, &[]);

        assert_eq!(
            refused,
            Err(FrameError::Short {
                wanted: 1,
                got: 0,
                format: PixelFormat::Mjpeg,
            })
        );
    }
}

mod planes {
    use super::*;

    #[test]
    fn the_planes_are_the_size_the_transport_will_copy() {
        // `to_libwebrtc_i420` reads `width.div_ceil(2)` by
        // `height.div_ceil(2)` out of each chroma plane and errors if the
        // plane is shorter. These are tight strides, so the length is the
        // whole contract.
        let row = [macropixel(0, 128, 0, 128), macropixel(0, 128, 0, 128)].concat();
        let rows: Vec<Vec<u8>> = (0..6).map(|_| row.clone()).collect();

        let picture = yuyv(4, 6, &rows).unwrap();

        assert_eq!(picture.y.len(), 4 * 6);
        assert_eq!(picture.u.len(), 2 * 3);
        assert_eq!(picture.v.len(), 2 * 3);
    }

    #[test]
    fn an_odd_height_still_fills_every_chroma_row() {
        let row = macropixel(0, 128, 0, 128).to_vec();
        let rows: Vec<Vec<u8>> = (0..5).map(|_| row.clone()).collect();

        let picture = yuyv(2, 5, &rows).unwrap();

        assert_eq!(picture.u.len(), 3, "five rows rounds up to three");
    }
}

mod screen_frames {
    use super::*;

    /// One row of BGRA, plus `pad` bytes of whatever the server left there.
    ///
    /// X11 pads rows to a four-byte boundary, so a capture's stride is not its
    /// width, and the padding holds nothing a frame should read.
    fn row(pixels: &[[u8; 3]], pad: usize) -> Vec<u8> {
        let mut bytes: Vec<u8> = pixels
            .iter()
            .flat_map(|[r, g, b]| [*b, *g, *r, 0xff])
            .collect();
        bytes.extend(std::iter::repeat_n(0xcc, pad));
        bytes
    }

    fn bgra(width: u32, height: u32, rows: &[Vec<u8>]) -> Result<Picture, FrameError> {
        let stride = rows.first().map_or(0, Vec::len);
        from_bgra(width, height, stride, &rows.concat())
    }

    /// A frame of one flat colour, `height` identical rows of it.
    fn flat(width: u32, height: u32, rgb: [u8; 3]) -> Result<Picture, FrameError> {
        let pixels = vec![rgb; width as usize];
        let rows = vec![row(&pixels, 0); height as usize];
        bgra(width, height, &rows)
    }

    fn near(got: u8, wanted: u8, label: &str) {
        let off = i32::from(got).abs_diff(i32::from(wanted));
        assert!(off <= 2, "{label}: got {got}, wanted about {wanted}");
    }

    #[test]
    fn the_blue_and_red_channels_are_not_swapped() {
        // The whole reason this entry point exists. X11 hands over BGRA, and
        // reading it as RGB turns a red window blue, which is a bug nobody can
        // attribute from a call.
        let red = flat(2, 2, [255, 0, 0]).unwrap();
        let blue = flat(2, 2, [0, 0, 255]).unwrap();

        near(red.v[0], 240, "red should sit at the top of Cr");
        near(red.u[0], 90, "red should sit near the bottom of Cb");
        near(blue.u[0], 240, "blue should sit at the top of Cb");
        near(blue.v[0], 110, "blue should sit near the bottom of Cr");
    }

    #[test]
    fn padding_at_the_end_of_a_row_is_not_read_as_pixels() {
        // A stride wider than the width. Reading the padding would put the
        // 0xcc filler into the right-hand column of every row.
        let black = row(&[[0, 0, 0], [0, 0, 0]], 8);

        let picture = bgra(2, 2, &[black.clone(), black]).unwrap();

        near(picture.y[1], 16, "the second pixel read into the padding");
        near(picture.y[3], 16, "the last pixel read into the padding");
    }

    #[test]
    fn black_and_white_land_in_the_studio_range() {
        let black = flat(2, 2, [0, 0, 0]).unwrap();
        let white = flat(2, 2, [255, 255, 255]).unwrap();

        near(black.y[0], 16, "black");
        near(white.y[0], 235, "white");
    }

    #[test]
    fn a_stride_shorter_than_the_width_is_refused_rather_than_read_past() {
        // A capture whose geometry disagrees with its buffer. Trusting it reads
        // off the end of a shared memory segment.
        let refused = from_bgra(4, 2, 8, &[0u8; 16]);

        assert!(
            matches!(refused, Err(FrameError::Geometry { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_buffer_shorter_than_the_frame_is_refused() {
        let refused = from_bgra(2, 2, 8, &[0u8; 8]);

        assert!(
            matches!(refused, Err(FrameError::Geometry { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_frame_with_no_pixels_in_it_is_refused() {
        assert_eq!(from_bgra(0, 4, 0, &[]), Err(FrameError::Empty));
        assert_eq!(from_bgra(4, 0, 16, &[]), Err(FrameError::Empty));
    }

    #[test]
    fn an_odd_size_still_fills_every_chroma_sample() {
        // A window can be any size at all, unlike a camera's negotiated modes.
        // The transport reads ceil(w/2) by ceil(h/2) out of each chroma plane
        // and refuses a buffer that cannot supply it.
        let three = row(&[[10, 20, 30], [10, 20, 30], [10, 20, 30]], 0);

        let picture = bgra(3, 3, &[three.clone(), three.clone(), three]).unwrap();

        assert_eq!(picture.y.len(), 9);
        assert_eq!(picture.u.len(), 4, "a 3x3 frame needs a 2x2 chroma plane");
        assert_eq!(picture.v.len(), 4);
    }
}

/// A picture small enough to put in front of somebody, and a picture a webview
/// can draw.
///
/// Both exist for the self view in the call card: the camera is a 1.38 MB I420
/// frame thirty times a second and the card is a square a couple of hundred
/// pixels wide, so what crosses the IPC is sampled down first and converted
/// once, when something asks.
mod a_picture_for_the_card {
    use super::*;

    /// A flat I420 picture, so a sample from anywhere in it is predictable.
    fn flat(width: u32, height: u32, y: u8, u: u8, v: u8) -> Picture {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));

        Picture {
            width,
            height,
            y: vec![y; w * h],
            u: vec![u; cw * ch],
            v: vec![v; cw * ch],
            timestamp_us: 0,
        }
    }

    #[test]
    fn a_thumbnail_keeps_the_shape_of_the_frame_it_came_from() {
        // 16:9 in, 16:9 out. A self view that stretched would be the one thing
        // somebody looking at their own face would notice immediately.
        let picture = flat(1280, 720, 100, 110, 120);

        let small = picture.thumbnail(320, 320);

        assert_eq!((small.width, small.height), (320, 180));
    }

    #[test]
    fn the_size_of_a_thumbnail_is_known_without_making_one() {
        // What is asked of an SFU is the box a still is drawn in, and that
        // box is this. Measured through the same arithmetic the thumbnail
        // uses, so the two cannot disagree.
        let picture = flat(1280, 720, 100, 110, 120);

        let size = picture.thumbnail_size(320, 320);

        let small = picture.thumbnail(320, 320);
        assert_eq!(size, (small.width, small.height));
        assert_eq!(size, (320, 180));
    }

    #[test]
    fn a_thumbnail_is_bounded_by_whichever_side_binds() {
        // Taller than it is wide, so the height is the limit and the width has
        // to come down with it rather than being left at the bound.
        let picture = flat(480, 960, 100, 110, 120);

        let small = picture.thumbnail(320, 180);

        assert_eq!((small.width, small.height), (90, 180));
    }

    #[test]
    fn a_frame_already_small_enough_is_not_enlarged() {
        // Sampling up invents detail and costs bytes to carry it. A small
        // camera is drawn at the size it is.
        let picture = flat(160, 90, 100, 110, 120);

        let small = picture.thumbnail(320, 180);

        assert_eq!((small.width, small.height), (160, 90));
    }

    #[test]
    fn a_thumbnail_has_even_sides_so_its_chroma_planes_are_exact() {
        // I420 carries one chroma sample per 2x2 block. An odd side leaves a
        // half block, and every reader of a `Picture` would need the same
        // rounding rule for it to mean anything.
        let picture = flat(1000, 999, 100, 110, 120);

        let small = picture.thumbnail(101, 101);

        assert!(small.width.is_multiple_of(2), "{} is odd", small.width);
        assert!(small.height.is_multiple_of(2), "{} is odd", small.height);
    }

    #[test]
    fn a_thumbnails_planes_are_tight() {
        let picture = flat(640, 480, 100, 110, 120);

        let small = picture.thumbnail(64, 64);

        let (w, h) = (small.width as usize, small.height as usize);
        assert_eq!(small.y.len(), w * h);
        assert_eq!(small.u.len(), w.div_ceil(2) * h.div_ceil(2));
        assert_eq!(small.v.len(), w.div_ceil(2) * h.div_ceil(2));
    }

    #[test]
    fn a_thumbnail_of_one_flat_colour_is_that_colour() {
        let picture = flat(640, 480, 81, 90, 240);

        let small = picture.thumbnail(64, 64);

        assert!(small.y.iter().all(|&y| y == 81), "the luma moved");
        assert!(
            small.u.iter().all(|&u| u == 90),
            "the blue difference moved"
        );
        assert!(
            small.v.iter().all(|&v| v == 240),
            "the red difference moved"
        );
    }

    #[test]
    fn a_thumbnail_samples_the_part_of_the_frame_it_is_standing_in_for() {
        // The left half of the picture is black and the right half is white, so
        // a thumbnail that sampled the wrong column would come back flat or
        // mirrored rather than merely approximate.
        let mut picture = flat(640, 480, 16, 128, 128);
        for row in 0..480usize {
            for column in 320..640usize {
                picture.y[row * 640 + column] = 235;
            }
        }

        let small = picture.thumbnail(64, 64);

        let width = small.width as usize;
        let row = &small.y[..width];
        assert!(
            row[..width / 2].iter().all(|&y| y == 16),
            "the left half is not black: {row:?}"
        );
        assert!(
            row[width / 2..].iter().all(|&y| y == 235),
            "the right half is not white: {row:?}"
        );
    }

    #[test]
    fn a_thumbnail_never_comes_back_empty() {
        // The card asks for a square and a camera could be any shape. A plane
        // of no bytes is something every reader would have to check for.
        let picture = flat(1280, 720, 100, 110, 120);

        let small = picture.thumbnail(1, 1);

        assert_eq!((small.width, small.height), (2, 2));
        assert_eq!(small.y.len(), 4);
    }

    #[test]
    fn white_comes_back_white_and_black_comes_back_black() {
        // Limited range: 16 is black and 235 is white, so a conversion that
        // forgot the offset would come back dark grey and off-white.
        let black = to_rgb(&flat(2, 2, 16, 128, 128));
        let white = to_rgb(&flat(2, 2, 235, 128, 128));

        assert_eq!(black, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(white, vec![255; 12]);
    }

    #[test]
    fn a_colour_survives_the_round_trip_through_i420() {
        // Through the encoder the camera path uses, so this is the colour a
        // person actually sees in the card rather than an isolated formula.
        for colour in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [128, 64, 32]] {
            let jpeg = flat_jpeg(2, 2, colour);
            let picture = decode(PixelFormat::Mjpeg, 2, 2, &jpeg).unwrap();

            let rgb = to_rgb(&picture);

            for (channel, (&got, &wanted)) in rgb[..3].iter().zip(&colour).enumerate() {
                let drift = i16::from(got).abs_diff(i16::from(wanted));
                assert!(
                    drift <= 12,
                    "channel {channel} of {colour:?} came back {got}, {drift} off"
                );
            }
        }
    }

    #[test]
    fn rgb_is_three_tight_bytes_a_pixel() {
        let picture = flat(4, 6, 100, 110, 120);

        let rgb = to_rgb(&picture);

        assert_eq!(rgb.len(), 4 * 6 * 3);
    }

    #[test]
    fn an_out_of_range_colour_is_clamped_rather_than_wrapped() {
        // The inverse of a limited-range conversion overshoots for chroma a
        // camera can legally send, and a wrap turns a bright edge into a dark
        // one. Two pictures, because no single one drives both ends: bright red
        // takes red past 255, and black with no blue in it takes blue below 0.
        let over = to_rgb(&flat(2, 2, 235, 128, 240));
        let under = to_rgb(&flat(2, 2, 16, 16, 128));

        assert_eq!(over[0], 255, "red wrapped instead of clamping");
        assert_eq!(under[2], 0, "blue wrapped instead of clamping");
    }
}
