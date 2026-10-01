// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Your own camera, small enough to put in the call card.
//!
//! The capture thread leaves the newest frame here sampled down, and the card
//! asks for it. Why a still that is asked for rather than frames pushed at the
//! webview: `docs/adr/0007-draw-the-self-view-from-a-still.md`.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use consort_video::{Picture, to_rgb};
use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder};

/// The largest self view that will be made, in pixels.
///
/// The card is a couple of hundred pixels wide and twice that expanded, so this
/// is generous for it and still a fiftieth of a 720p frame.
const BOUND: (u32, u32) = (320, 320);

/// How hard to compress it. A camera frame has no edges that artefact badly.
const QUALITY: u8 = 70;

/// The newest camera frame, and whatever was last drawn from one.
///
/// Cheap to clone: every clone is the same slot, the way
/// `consort_call::Camera` is. One goes to the capture thread, which only
/// [`offer`](Self::offer)s, and one stays in `AppState` for the command to ask.
#[derive(Clone, Default)]
pub struct SelfView(Arc<Mutex<Held>>);

#[derive(Default)]
struct Held {
    /// Sampled down by the capture thread, not yet encoded.
    fresh: Option<Picture>,
    /// The last picture [`SelfView::latest`] produced.
    ///
    /// Kept so that a card asking faster than the camera produces frames
    /// redraws the same picture rather than blinking off and on.
    drawn: Option<String>,
}

impl SelfView {
    /// An empty slot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Take the newest frame, sampled down, replacing whatever was waiting.
    ///
    /// Runs on the capture thread, so it samples and nothing else: the colour
    /// conversion and the encode happen in [`latest`](Self::latest), which only
    /// runs when something is actually drawing this.
    pub fn offer(&self, frame: &Picture) {
        let mut held = self.held();
        held.fresh = Some(frame.thumbnail(BOUND.0, BOUND.1));
    }

    /// The newest frame as a data URL, or the last one when none has arrived.
    ///
    /// `None` only when no frame has arrived since the camera was switched on,
    /// which is the moment between opening a device and its first frame.
    pub fn latest(&self) -> Option<String> {
        let mut held = self.held();

        if let Some(picture) = held.fresh.take() {
            held.drawn = encode(&picture);
        }

        held.drawn.clone()
    }

    /// Throw away the picture and the frame behind it.
    ///
    /// Called when the camera goes off, so the card stops showing the last
    /// thing it saw. The same reason `consort_call::Camera::clear` exists.
    pub fn clear(&self) {
        *self.held() = Held::default();
    }

    fn held(&self) -> MutexGuard<'_, Held> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One picture as a `data:` URL.
///
/// `data:` rather than a scheme of its own because `img-src` in the policy
/// already allows it, and a self view is kilobytes rather than the megabytes
/// `crate::media` exists to stream.
fn encode(picture: &Picture) -> Option<String> {
    let rgb = to_rgb(picture);
    let mut jpeg = Vec::new();

    JpegEncoder::new_with_quality(&mut jpeg, QUALITY)
        .write_image(&rgb, picture.width, picture.height, ExtendedColorType::Rgb8)
        // Nothing `offer` produces is refused: a thumbnail is at least 2x2 and
        // `to_rgb` gives exactly the bytes claimed. Answered rather than
        // unwrapped because the caller is the thread reading the camera.
        .inspect_err(|error| tracing::warn!(%error, "could not encode the self view"))
        .ok()?;

    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(&jpeg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat I420 frame of one colour, at whatever size.
    fn frame(width: u32, height: u32, y: u8) -> Picture {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));

        Picture {
            width,
            height,
            y: vec![y; w * h],
            u: vec![128; cw * ch],
            v: vec![128; cw * ch],
            timestamp_us: 0,
        }
    }

    /// What is actually in a data URL, decoded back to a picture.
    fn drawn(url: &str) -> image::DynamicImage {
        let base64 = url
            .strip_prefix("data:image/jpeg;base64,")
            .unwrap_or_else(|| panic!("not a JPEG data URL: {}", &url[..40.min(url.len())]));
        let jpeg = STANDARD.decode(base64).expect("the URL holds base64");

        image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
            .expect("the URL holds a JPEG")
    }

    #[test]
    fn nothing_is_drawn_before_a_frame_arrives() {
        // The gap between opening a device and its first frame. A card drawing
        // an empty picture there would flash a black square.
        let view = SelfView::new();

        assert_eq!(view.latest(), None);
    }

    #[test]
    fn a_frame_comes_back_as_a_jpeg_data_url() {
        let view = SelfView::new();

        view.offer(&frame(640, 480, 120));

        let url = view.latest().expect("a frame was offered");
        assert_eq!(drawn(&url).color().channel_count(), 3);
    }

    #[test]
    fn the_picture_is_sampled_down_to_something_a_card_can_hold() {
        // The whole point of the still. A 720p frame is 1.38 MB and the card is
        // a couple of hundred pixels wide.
        let view = SelfView::new();

        view.offer(&frame(1280, 720, 120));

        let picture = drawn(&view.latest().unwrap());
        assert_eq!((picture.width(), picture.height()), (320, 180));
    }

    #[test]
    fn the_newest_frame_wins() {
        // A picture is a state, not a sequence: an older frame is the wrong
        // answer rather than missing data.
        let view = SelfView::new();

        view.offer(&frame(64, 64, 16));
        view.offer(&frame(64, 64, 235));

        let picture = drawn(&view.latest().unwrap()).to_luma8();
        assert!(
            picture.pixels().all(|pixel| pixel.0[0] > 200),
            "the older darker frame was drawn"
        );
    }

    #[test]
    fn asking_twice_without_a_new_frame_draws_the_same_picture() {
        // The card asks on a timer and the camera produces frames on its own, so
        // the two do not line up. Answering nothing between frames would blink
        // the picture off and on.
        let view = SelfView::new();
        view.offer(&frame(64, 64, 120));

        let first = view.latest();
        let second = view.latest();

        assert!(first.is_some());
        assert_eq!(first, second);
    }

    #[test]
    fn clearing_leaves_nothing_to_draw() {
        // Switching the camera off. Without this the card keeps showing the
        // last thing the camera saw, which for somebody who just covered it is
        // the one picture they did not want left on screen.
        let view = SelfView::new();
        view.offer(&frame(64, 64, 120));
        view.latest();

        view.clear();

        assert_eq!(view.latest(), None);
    }

    #[test]
    fn clearing_drops_a_frame_that_was_never_drawn() {
        // The camera went off between a frame arriving and anybody asking.
        let view = SelfView::new();
        view.offer(&frame(64, 64, 120));

        view.clear();

        assert_eq!(view.latest(), None);
    }

    #[test]
    fn a_frame_the_encoder_refuses_is_not_a_url() {
        // Unreachable through `offer`, which samples to at least 2x2. Reached
        // directly because the alternative to answering nothing here is a panic
        // on the thread reading the camera.
        let empty = Picture {
            width: 0,
            height: 0,
            y: Vec::new(),
            u: Vec::new(),
            v: Vec::new(),
            timestamp_us: 0,
        };

        assert_eq!(encode(&empty), None);
    }

    /// What one self view costs, against the real camera.
    ///
    /// The numbers in `docs/adr/0007-draw-the-self-view-from-a-still.md` come
    /// from here, so they can be taken again rather than trusted.
    ///
    /// ```sh
    /// cargo test -p consort-app --lib selfview -- --ignored --nocapture measure
    /// ```
    #[test]
    #[ignore = "needs a real camera"]
    fn measure_a_real_self_view() {
        use std::sync::mpsc::channel;
        use std::time::{Duration, Instant};

        use consort_video::{VideoCapture, capture};

        let (frames, inbox) = channel();
        let _stream = consort_video::Host::default()
            .open(
                None,
                capture::WANTED,
                Box::new(move |picture| {
                    let _ = frames.send(picture);
                }),
            )
            .expect("no camera");

        // The first few frames are the device's auto exposure settling.
        for _ in 0..5 {
            let _ = inbox.recv_timeout(Duration::from_secs(5));
        }
        let frame = inbox
            .recv_timeout(Duration::from_secs(5))
            .expect("no frame");
        println!("captured {}x{}", frame.width, frame.height);

        let view = SelfView::new();
        let sampling = Instant::now();
        for _ in 0..200 {
            view.offer(&frame);
        }
        println!("offer (sample down): {:?}", sampling.elapsed() / 200);

        view.offer(&frame);
        let encoding = Instant::now();
        let url = view.latest().unwrap();
        println!(
            "encode: {:?}, url {} B, jpeg ~{} B",
            encoding.elapsed(),
            url.len(),
            url.len() * 3 / 4
        );
        println!(
            "the frame itself: {} B",
            frame.y.len() + frame.u.len() + frame.v.len()
        );
    }

    #[test]
    fn every_clone_is_the_same_slot() {
        // What the capture thread holds and what the command reads have to be
        // one slot, or the card draws nothing while frames arrive.
        let view = SelfView::new();
        let capturing = view.clone();

        capturing.offer(&frame(64, 64, 120));

        assert!(view.latest().is_some());
    }
}
