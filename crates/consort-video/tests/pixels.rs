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

use consort_video::{FrameError, Picture, PixelFormat, decode};

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
