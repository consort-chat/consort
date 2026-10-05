// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Getting everybody else's pictures out of the call and onto the card.
//!
//! [`crate::hearing`]'s mirror, and the same bug was waiting here: a
//! subscribed video track is a decoder writing frames into a stream, and a
//! stream nobody reads is frames decoded and dropped. Consort shipped exactly
//! that for cameras and shared screens, so two people could each send both and
//! each see neither.
//!
//! [`Seen`] is a trait for the reason [`crate::hearing::Heard`] is one: what is
//! on the other end is a webview, and this crate must not know that. What
//! arrives there is a still, asked for at the size it is drawn:
//! `docs/adr/0014-ask-for-a-remote-picture-at-the-size-it-is-drawn.md`.

use std::collections::BTreeSet;
use std::sync::Arc;

use matrix_rtc_media::{MediaStreamKind, Participant as MediaParticipant, VideoFrame};
use serde::{Deserialize, Serialize};

use crate::roster;

/// Which of the two pictures somebody can be sending.
///
/// Both at once is ordinary: a camera fills its owner's own square and a
/// shared screen earns a square of its own, which is
/// `docs/adr/0008-one-square-for-everything-in-a-call.md`.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Camera,
    Screen,
}

impl Kind {
    /// What a transport calls this stream.
    pub fn stream(self) -> MediaStreamKind {
        match self {
            Self::Camera => MediaStreamKind::Camera,
            Self::Screen => MediaStreamKind::ScreenShare,
        }
    }

    /// Whether this membership is sending one the call can see.
    fn live(self, member: &MediaParticipant) -> bool {
        match self {
            Self::Camera => roster::camera_live(member),
            Self::Screen => roster::screen_live(member),
        }
    }
}

/// One frame that arrived from the call, planar I420 with tight strides.
///
/// [`crate::OutgoingPicture`]'s mirror, and a type of its own for the reason
/// the two directions have traits of their own. It carries no capture time:
/// what this becomes is a still, and a still has no use for one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingPicture {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

/// Somewhere to put the pictures the other people in a call are sending.
///
/// `Send + Sync` because one is shared by every pump task at once, and
/// `'static` because those tasks outlive the call that started them by however
/// long it takes them to notice.
///
/// Keyed by membership, like the audio and for the same reason: a picture
/// belongs to the device sending it. The person it belongs to travels with
/// every frame rather than being declared in a pass of its own, because unlike
/// a queue of audio a still needs nothing hung on it between frames.
pub trait Seen: Send + Sync + 'static {
    /// Keep this frame as the newest one `member_id`, who is `user_id`, is
    /// sending of `kind`.
    ///
    /// Must not block. It is called from the call thread, which is also
    /// servicing the SFU.
    fn see(&self, member_id: &str, user_id: &str, kind: Kind, picture: IncomingPicture);

    /// `member_id` has stopped sending one of `kind`; throw the picture away.
    ///
    /// Rather than leaving the last frame drawable, which for somebody who has
    /// just covered their camera is the one picture they did not want kept.
    fn forget(&self, member_id: &str, kind: Kind);

    /// Throw away every picture from everybody. What the end of a call needs.
    fn clear(&self);
}

/// A shared handle on somewhere to put a call's pictures.
pub type Eyes = Arc<dyn Seen>;

/// Everybody in `participants` whose `kind` we should be pulling.
///
/// Our own membership is excluded, for the reason
/// [`crate::hearing::audible`] excludes it: the SFU does not send it back.
///
/// A muted one is excluded too, which is where this parts company with the
/// audio. A muted microphone is attached anyway so that unmuting is instant,
/// and the cost is nothing because no samples arrive. A camera that is off has
/// a picture behind it that somebody switched off on purpose, and the roster
/// draws no square for it either way: `roster::camera_live` is the one answer
/// to who has something worth drawing.
pub fn wanted(participants: &[MediaParticipant], kind: Kind) -> BTreeSet<String> {
    participants
        .iter()
        .filter(|member| !member.is_local)
        .filter(|member| kind.live(member))
        .map(|member| member.member_id.clone())
        .collect()
}

/// One decoded frame, out from behind the strides it arrived with.
///
/// By value, so a frame whose rows are already tight is moved rather than
/// copied, which is every frame from a publisher whose width is a multiple of
/// the decoder's alignment.
///
/// `frame.rotation` is ignored. Nothing Consort publishes sets one, and
/// honouring it means transposing three planes per frame for the clients that
/// might: see the ADR.
pub fn incoming(frame: VideoFrame) -> IncomingPicture {
    let buffer = frame.buffer;
    let (width, height) = (buffer.width, buffer.height);
    let (w, h) = (width as usize, height as usize);
    let (chroma_w, chroma_h) = (w.div_ceil(2), h.div_ceil(2));

    IncomingPicture {
        width,
        height,
        y: tighten(buffer.data_y, buffer.stride_y as usize, w, h),
        u: tighten(buffer.data_u, buffer.stride_u as usize, chroma_w, chroma_h),
        v: tighten(buffer.data_v, buffer.stride_v as usize, chroma_w, chroma_h),
    }
}

/// One plane's rows with the padding between them removed.
///
/// Always exactly `width * height` bytes, padded with black rather than cut
/// short: everything downstream reads the length the dimensions claim, and a
/// plane that lied about it would be a panic in whatever drew it.
fn tighten(mut plane: Vec<u8>, stride: usize, width: usize, height: usize) -> Vec<u8> {
    let wanted = width * height;

    if stride == width && plane.len() >= wanted {
        plane.truncate(wanted);
        return plane;
    }

    let mut tight = vec![0u8; wanted];
    for row in 0..height {
        let from = row * stride;
        let Some(source) = plane.get(from..from + width) else {
            break;
        };
        tight[row * width..][..width].copy_from_slice(source);
    }
    tight
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_rtc_media::{I420Buffer, StreamState, VideoRotation};

    fn member(member_id: &str, is_local: bool, kinds: &[MediaStreamKind]) -> MediaParticipant {
        MediaParticipant {
            member_id: member_id.to_owned(),
            user_id: format!("@{member_id}:example.org"),
            device_id: None,
            is_local,
            reachable: true,
            hand_raised_at_ms: None,
            joined_at_ms: None,
            streams: kinds
                .iter()
                .map(|kind| StreamState {
                    kind: *kind,
                    muted: false,
                })
                .collect(),
        }
    }

    fn muted(member_id: &str, kind: MediaStreamKind) -> MediaParticipant {
        let mut member = member(member_id, false, &[kind]);
        member.streams[0].muted = true;
        member
    }

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// A frame whose planes carry `pad` bytes of 255 after every row.
    fn frame(width: u32, height: u32, pad: u32) -> VideoFrame {
        let (w, h) = (width as usize, height as usize);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let padded = |row_width: usize, rows: usize, value: u8| {
            let stride = row_width + pad as usize;
            (0..rows)
                .flat_map(|_| {
                    let mut row = vec![value; row_width];
                    row.extend(std::iter::repeat_n(255, pad as usize));
                    row
                })
                .take(stride * rows)
                .collect::<Vec<u8>>()
        };

        VideoFrame {
            buffer: I420Buffer {
                width,
                height,
                data_y: padded(w, h, 120),
                stride_y: width + pad,
                data_u: padded(cw, ch, 128),
                stride_u: cw as u32 + pad,
                data_v: padded(cw, ch, 128),
                stride_v: cw as u32 + pad,
            },
            rotation: VideoRotation::Deg0,
            timestamp_us: 0,
        }
    }

    #[test]
    fn somebody_with_a_live_camera_is_pulled() {
        let people = vec![member("alice", false, &[MediaStreamKind::Camera])];

        assert_eq!(wanted(&people, Kind::Camera), set(&["alice"]));
    }

    #[test]
    fn a_camera_that_is_off_is_not_pulled() {
        // Where this parts company with the audio. Nothing draws a square for
        // a camera somebody switched off, so a stream pulled for one is a task
        // waiting on frames that will not come.
        let people = vec![muted("alice", MediaStreamKind::Camera)];

        assert!(wanted(&people, Kind::Camera).is_empty());
    }

    #[test]
    fn our_own_camera_is_never_pulled_back_from_the_sfu() {
        // The self view is a local capture. An SFU does not send our own
        // camera back, so a task waiting on it waits forever.
        let people = vec![
            member("me", true, &[MediaStreamKind::Camera]),
            member("alice", false, &[MediaStreamKind::Camera]),
        ];

        assert_eq!(wanted(&people, Kind::Camera), set(&["alice"]));
    }

    #[test]
    fn a_shared_screen_is_a_different_question_from_a_camera() {
        // Somebody can share their slides with their camera off, and the two
        // end up in different squares.
        let people = vec![
            member("alice", false, &[MediaStreamKind::ScreenShare]),
            member("bob", false, &[MediaStreamKind::Camera]),
        ];

        assert_eq!(wanted(&people, Kind::Screen), set(&["alice"]));
        assert_eq!(wanted(&people, Kind::Camera), set(&["bob"]));
    }

    #[test]
    fn somebody_publishing_nothing_is_not_pulled() {
        let people = vec![member("alice", false, &[MediaStreamKind::Microphone])];

        assert!(wanted(&people, Kind::Camera).is_empty());
        assert!(wanted(&people, Kind::Screen).is_empty());
    }

    #[test]
    fn each_kind_names_the_stream_a_transport_knows() {
        // Asking for the wrong one opens a stream that produces no frames, and
        // nothing reports that: it looks exactly like a camera nobody is
        // pointing at anything.
        assert_eq!(Kind::Camera.stream(), MediaStreamKind::Camera);
        assert_eq!(Kind::Screen.stream(), MediaStreamKind::ScreenShare);
    }

    #[test]
    fn the_padding_between_rows_does_not_arrive_with_the_picture() {
        // A decoder's stride is its own business and is routinely wider than
        // the frame. Read as pixels it is a white stripe down the side of
        // everybody's camera, and every row after the first slides left.
        let picture = incoming(frame(6, 4, 10));

        assert_eq!((picture.width, picture.height), (6, 4));
        assert_eq!(picture.y, vec![120; 24]);
        assert_eq!(picture.u, vec![128; 6]);
        assert_eq!(picture.v, vec![128; 6]);
    }

    #[test]
    fn a_frame_that_is_already_tight_arrives_unchanged() {
        let picture = incoming(frame(4, 2, 0));

        assert_eq!(picture.y, vec![120; 8]);
        assert_eq!(picture.u, vec![128; 2]);
    }

    #[test]
    fn an_odd_sized_frame_keeps_whole_chroma_blocks() {
        // 4:2:0 chroma is half the frame rounded up, so a 5x3 frame has 3x2
        // chroma planes. Rounding down loses a row and makes every plane
        // length downstream disagree with the dimensions.
        let picture = incoming(frame(5, 3, 2));

        assert_eq!(picture.y.len(), 15);
        assert_eq!(picture.u.len(), 6);
        assert_eq!(picture.v.len(), 6);
    }

    #[test]
    fn a_plane_shorter_than_its_dimensions_claim_is_padded_rather_than_cut() {
        // Never seen from a decoder, and the alternative is a panic in
        // whatever draws it: everything downstream reads the length the
        // dimensions promise.
        let mut short = frame(8, 8, 0);
        short.buffer.data_y.truncate(20);

        let picture = incoming(short);

        assert_eq!(picture.y.len(), 64);
        assert_eq!(picture.y[20..], vec![0; 44]);
    }
}
