// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! What you are sending, small enough to put in the call card.
//!
//! The capture thread leaves the newest frame here and the card asks for it at
//! the size it is drawing. One slot per thing being sent: see
//! `docs/adr/0007-draw-the-self-view-from-a-still.md` for why a still that is
//! asked for, `0008` for why the shared screen gets a second one, and `0014`
//! for why the size travels with the ask.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use consort_video::{Picture, to_rgb};
use image::{ExtendedColorType, ImageEncoder, codecs::jpeg::JpegEncoder};

use crate::theirview::clamped;

/// How hard to compress it. Neither a face nor a desktop at this size has
/// edges that artefact badly.
const QUALITY: u8 = 70;

/// The newest frame of one thing being sent.
///
/// Cheap to clone: every clone is the same slot, the way
/// `consort_call::Camera` is. One goes to the capture thread, which only
/// [`offer`](Self::offer)s, and one stays in `AppState` for the command to ask.
///
/// One of these per thing being sent. The camera and the shared screen each
/// have their own, because both can be going out at once.
#[derive(Clone, Default)]
pub struct SelfView(Arc<Mutex<Option<Picture>>>);

impl SelfView {
    /// An empty slot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keep the newest frame, replacing whatever was there.
    ///
    /// Runs on the capture thread, so it copies and nothing else. The sampling
    /// and the encode happen in [`latest`](Self::latest), which knows the size
    /// being drawn and only runs when something is drawing it.
    pub fn offer(&self, frame: &Picture) {
        // Copied before the lock: the capture thread should never wait on a
        // card mid-encode, which is `theirview`'s rule here too.
        let kept = frame.clone();
        *self.held() = Some(kept);
    }

    /// The newest frame as a data URL, no larger than `bound` on its long edge.
    ///
    /// `None` before the first frame of a camera that has just been opened, and
    /// after one goes off and [`clear`](Self::clear) empties this.
    ///
    /// The frame is kept rather than taken, so a card asking faster than the
    /// capture produces frames redraws the same picture instead of blinking off
    /// and on. Sampled under the lock and encoded outside it, the way
    /// [`crate::theirview`] does it: an encode is a hundred times the sampling.
    pub fn latest(&self, bound: u32) -> Option<String> {
        let bound = clamped(bound);
        let sampled = self.held().as_ref()?.thumbnail(bound, bound);

        encode(&sampled)
    }

    /// Throw away the picture and the frame behind it.
    ///
    /// Called when the camera or the share goes off, so the card stops
    /// showing the last thing it saw. The same reason
    /// `consort_call::Camera::clear` exists.
    pub fn clear(&self) {
        *self.held() = None;
    }

    fn held(&self) -> MutexGuard<'_, Option<Picture>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// One picture as a `data:` URL.
///
/// `data:` rather than a scheme of its own because `img-src` in the policy
/// already allows it, and a picture this size is kilobytes rather than the
/// megabytes `crate::media` exists to stream. Shared with
/// [`crate::theirview`], which draws everybody else's the same way.
pub(crate) fn encode(picture: &Picture) -> Option<String> {
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

        assert_eq!(view.latest(320), None);
    }

    #[test]
    fn a_frame_comes_back_as_a_jpeg_data_url() {
        let view = SelfView::new();

        view.offer(&frame(640, 480, 120));

        let url = view.latest(320).expect("a frame was offered");
        assert_eq!(drawn(&url).color().channel_count(), 3);
    }

    #[test]
    fn the_picture_is_sampled_down_to_something_a_card_can_hold() {
        // The whole point of the still. A 720p frame is 1.38 MB and the card is
        // a couple of hundred pixels wide.
        let view = SelfView::new();

        view.offer(&frame(1280, 720, 120));

        let picture = drawn(&view.latest(320).unwrap());
        assert_eq!((picture.width(), picture.height()), (320, 180));
    }

    #[test]
    fn a_bigger_box_is_drawn_from_more_pixels() {
        // Issue #194. Our own share goes on the stage, which is up to the whole
        // window, and a picture made for a seventy-pixel square is unreadable
        // blown up to it.
        let view = SelfView::new();

        view.offer(&frame(1920, 1080, 120));

        let picture = drawn(&view.latest(960).unwrap());
        assert_eq!((picture.width(), picture.height()), (960, 540));
    }

    #[test]
    fn a_frame_smaller_than_the_box_is_not_blown_up() {
        // Asking for more than was captured is answered with what there is.
        // Scaling up here would cost the encode and add nothing.
        let view = SelfView::new();

        view.offer(&frame(640, 360, 120));

        let picture = drawn(&view.latest(1920).unwrap());
        assert_eq!((picture.width(), picture.height()), (640, 360));
    }

    #[test]
    fn a_box_past_the_ceiling_is_brought_back_to_it() {
        // The same ceiling everybody else's picture is held to, so a 4K share
        // on a 4K display cannot ask for a frame nothing will pay for.
        let view = SelfView::new();

        view.offer(&frame(3840, 2160, 120));

        let picture = drawn(&view.latest(4000).unwrap());
        assert_eq!((picture.width(), picture.height()), (1920, 1080));
    }

    #[test]
    fn the_newest_frame_wins() {
        // A picture is a state, not a sequence: an older frame is the wrong
        // answer rather than missing data.
        let view = SelfView::new();

        view.offer(&frame(64, 64, 16));
        view.offer(&frame(64, 64, 235));

        let picture = drawn(&view.latest(320).unwrap()).to_luma8();
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

        let first = view.latest(320);
        let second = view.latest(320);

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
        view.latest(320);

        view.clear();

        assert_eq!(view.latest(320), None);
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

    /// What moving the sampling off the capture thread costs, per 1080p frame.
    ///
    /// The numbers in
    /// `docs/adr/0018-sample-the-self-view-at-the-size-it-is-drawn.md` come
    /// from here. Synthetic rather than captured, so it needs no hardware.
    ///
    /// ```sh
    /// cargo test -p consort-app --lib selfview -- --ignored --nocapture measure_the_cost_of
    /// ```
    #[test]
    #[ignore = "a measurement, not a check"]
    fn measure_the_cost_of_sampling_when_it_is_drawn() {
        use std::time::Instant;

        let shared = frame(1920, 1080, 120);
        let view = SelfView::new();

        let offering = Instant::now();
        for _ in 0..200 {
            view.offer(&shared);
        }
        println!("offer (copy): {:?}", offering.elapsed() / 200);

        let sampling = Instant::now();
        for _ in 0..200 {
            let _ = shared.thumbnail(320, 320);
        }
        println!("what offer used to do: {:?}", sampling.elapsed() / 200);

        for bound in [320, 480, 960, 1920] {
            let asking = Instant::now();
            let url = view.latest(bound).expect("a frame was offered");
            println!(
                "latest({bound}): {:?}, {} B",
                asking.elapsed(),
                url.len() * 3 / 4
            );
        }
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
        let url = view.latest(320).unwrap();
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

    /// What one picture of a real screen costs.
    ///
    /// The camera's numbers are in
    /// `docs/adr/0007-draw-the-self-view-from-a-still.md` and this one's in
    /// `0008`. A desktop is the hard case for a JPEG: text has edges where a
    /// face has none.
    ///
    /// Prints sizes and timings only. Nothing captured is written anywhere.
    ///
    /// ```sh
    /// cargo test -p consort-app --lib selfview -- --ignored --nocapture measure_a_real_screen
    /// ```
    #[test]
    #[ignore = "needs an X11 display"]
    fn measure_a_real_screen() {
        use std::sync::mpsc::channel;
        use std::time::{Duration, Instant};

        use consort_video::{ScreenCapture, Screens};

        let backend = Screens::default();
        let sources = backend.sources().expect("no X11 display");
        let first = sources.first().expect("nothing shareable");
        println!("sharing {}x{}", first.width, first.height);

        let (frames, inbox) = channel();
        let _stream = backend
            .open(
                &first.id,
                Box::new(move |picture| {
                    let _ = frames.send(picture);
                }),
            )
            .expect("the source would not open");

        let frame = inbox
            .recv_timeout(Duration::from_secs(5))
            .expect("no frame");

        let view = SelfView::new();
        let sampling = Instant::now();
        for _ in 0..200 {
            view.offer(&frame);
        }
        println!("offer (sample down): {:?}", sampling.elapsed() / 200);

        view.offer(&frame);
        let encoding = Instant::now();
        let url = view.latest(320).unwrap();
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

        assert!(view.latest(320).is_some());
    }
}
