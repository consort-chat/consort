// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning memberships into people.
//!
//! A MatrixRTC roster is per membership, which is per device. What an interface
//! draws is per person. Collapsing the two is arithmetic over a list, with no
//! `Client` and no SFU behind it, so it lives here where a test can reach it
//! rather than in [`crate::livekit`] where nothing can.

use consort_matrix::Participant;
use matrix_rtc_media::{MediaStreamKind, Participant as MediaParticipant};

/// Whether this membership has muted its microphone.
///
/// Exactly [`camera_live`] negated, and for the same reason: the question is
/// what the call can hear, and a membership publishing no microphone at all
/// cannot be heard. It used to answer "not muted" for that, on the argument
/// that publishing nothing is not a choice somebody made. The argument is
/// wrong about the case it matters in. Joining from a lobby with the
/// microphone off publishes no audio track, so the person who most needs to be
/// told their microphone is off was the one person nobody could see it on, and
/// they find out by talking to a room that cannot hear them.
///
/// It also puts us back in step with every other client. Element Call reads a
/// missing publication as muted too.
///
/// The cost is an icon during the moment between a membership appearing and
/// its first publication landing, which is a mute that is true while it lasts.
pub fn microphone_muted(member: &MediaParticipant) -> bool {
    !member
        .streams
        .iter()
        .any(|stream| stream.kind == MediaStreamKind::Microphone && !stream.muted)
}

/// Whether this membership has a camera the call can see.
///
/// The microphone test with the other stream in it, and no other difference:
/// both ask what the call can pick up, and a membership publishing nothing of
/// that kind cannot be picked up. Which is how Element Call behaves too, since
/// it publishes no camera track until somebody turns one on.
pub fn camera_live(member: &MediaParticipant) -> bool {
    member
        .streams
        .iter()
        .any(|stream| stream.kind == MediaStreamKind::Camera && !stream.muted)
}

/// When this membership joined the call, `ours` standing in for our own.
///
/// The transport reports arrivals, and an arrival is something somebody else
/// does. LiveKit tells this client who turned up after it connected and there
/// is no event in which it turns up itself, so our own membership came back
/// with no time on it and the card belonging to the person reading it was the
/// one card in the call with no "in call" line.
///
/// `ours` is the moment this session's join returned, which is a fact this
/// process holds outright rather than one the SFU has to be asked for. It
/// stands in for exactly one row: everybody else keeps the transport's record,
/// which is the only thing that can be right about people who were already in
/// the call when we arrived.
pub fn arrived_at(member: &MediaParticipant, ours: u64) -> Option<u64> {
    member.joined_at_ms.or(member.is_local.then_some(ours))
}

/// Attach each named person's mute state, given one entry per membership.
///
/// `memberships` is `(user_id, muted)` in roster order, and `people` is the
/// deduplicated, named result of resolving those user IDs.
///
/// Muted only if every one of their memberships is. Somebody on a laptop and a
/// phone is one person here, and drawing a mute for a person still speaking
/// from the other device is worse than drawing nothing: it says the room cannot
/// hear them when it can.
pub fn with_mutes(people: Vec<Participant>, memberships: &[(String, bool)]) -> Vec<Participant> {
    people
        .into_iter()
        .map(|person| {
            let mut theirs = memberships
                .iter()
                .filter(|(user_id, _)| *user_id == person.id)
                .peekable();
            // `all` is true of nothing, so the emptiness has to be asked about
            // separately. A person nothing in the roster matches is not
            // somebody who has muted every device they own.
            let muted = theirs.peek().is_some() && theirs.all(|(_, muted)| *muted);
            person.with_muted(muted)
        })
        .collect()
}

/// Attach each named person's camera, given one entry per membership.
///
/// `memberships` is `(user_id, camera_live)` in roster order, matching
/// [`with_mutes`].
///
/// On camera if *any* of their memberships is, which is the opposite of the
/// mute rule above and right for the same reason. Somebody sitting in front of
/// a laptop camera with a phone in their pocket is on camera: drawing them as
/// off would say the call cannot see them when it can, and both rules are
/// chosen so the icon never claims less exposure than there is.
pub fn with_cameras(people: Vec<Participant>, memberships: &[(String, bool)]) -> Vec<Participant> {
    people
        .into_iter()
        .map(|person| {
            let live = memberships
                .iter()
                .any(|(user_id, live)| *user_id == person.id && *live);
            person.with_camera(live)
        })
        .collect()
}

/// Attach when each named person joined the call, given one entry per
/// membership.
///
/// `memberships` is `(user_id, joined_at_ms)` in roster order, matching
/// [`with_mutes`].
///
/// The earliest of their memberships, because somebody who opened the call on
/// a laptop and then picked up a phone has been in it since the laptop. `None`
/// where no membership of theirs has a time yet, which is everybody drawn from
/// room state and anybody whose media has not appeared.
pub fn with_since(
    people: Vec<Participant>,
    memberships: &[(String, Option<u64>)],
) -> Vec<Participant> {
    people
        .into_iter()
        .map(|person| {
            let since = memberships
                .iter()
                .filter(|(user_id, _)| *user_id == person.id)
                .filter_map(|(_, since)| *since)
                .min();
            person.with_since(since)
        })
        .collect()
}

/// Mark the people every one of whose memberships has deafened itself.
///
/// Every one, matching [`with_mutes`] and for the same reason: somebody
/// deafened on their laptop who is still listening on their phone can hear
/// you, and telling you otherwise would be worse than saying nothing.
///
/// `memberships` pairs each membership with the person it belongs to;
/// `deafened` is the membership IDs that have said so. A membership nobody has
/// heard from is not deafened, which is what makes this correct in a call with
/// Element Call or an older Consort in it.
pub fn with_deafened(
    people: Vec<Participant>,
    memberships: &[(String, String)],
    deafened: &[String],
) -> Vec<Participant> {
    with_flag(people, memberships, deafened, Participant::with_deafened)
}

/// Mark the people every one of whose memberships has said it is away.
///
/// Every one, for the reason [`with_deafened`] says: somebody away on their
/// laptop who is at their phone is at their computer, and a clock beside their
/// name would tell the room to stop expecting an answer they are able to give.
pub fn with_away(
    people: Vec<Participant>,
    memberships: &[(String, String)],
    away: &[String],
) -> Vec<Participant> {
    with_flag(people, memberships, away, Participant::with_away)
}

/// The shape both of the above are.
///
/// One function rather than two copies, because the rule is the interesting
/// part and it is the same rule: a person is flagged only when every
/// membership they have in this call says so, and a person with no memberships
/// at all is not flagged. Getting that wrong in one of two copies is the kind
/// of divergence nobody notices until somebody joins on a second device.
fn with_flag(
    people: Vec<Participant>,
    memberships: &[(String, String)],
    flagged: &[String],
    set: fn(Participant, bool) -> Participant,
) -> Vec<Participant> {
    people
        .into_iter()
        .map(|person| {
            let mut theirs = memberships
                .iter()
                .filter(|(_, user_id)| *user_id == person.id)
                .peekable();

            // `all` is true of nothing, so the emptiness has to be asked about
            // separately. Same trap as `with_mutes`.
            let every =
                theirs.peek().is_some() && theirs.all(|(member_id, _)| flagged.contains(member_id));

            set(person, every)
        })
        .collect()
}

#[cfg(test)]
mod deafening {
    use super::*;

    fn person(user_id: &str) -> Participant {
        Participant::named(user_id, "Somebody")
    }

    fn membership(member_id: &str, user_id: &str) -> (String, String) {
        (member_id.to_owned(), user_id.to_owned())
    }

    fn ids(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn somebody_whose_only_device_deafened_is_deafened() {
        let people = with_deafened(
            vec![person("@ada:example.org")],
            &[membership("ada-laptop", "@ada:example.org")],
            &ids(&["ada-laptop"]),
        );

        assert!(people[0].deafened);
    }

    #[test]
    fn somebody_nobody_has_heard_from_is_not_deafened() {
        // Element Call, or a Consort too old to say. Guessing would put a
        // headphone icon beside somebody who can hear perfectly well.
        let people = with_deafened(
            vec![person("@ada:example.org")],
            &[membership("ada-laptop", "@ada:example.org")],
            &[],
        );

        assert!(!people[0].deafened);
    }

    #[test]
    fn somebody_still_listening_on_another_device_is_not_deafened() {
        let people = with_deafened(
            vec![person("@ada:example.org")],
            &[
                membership("ada-laptop", "@ada:example.org"),
                membership("ada-phone", "@ada:example.org"),
            ],
            &ids(&["ada-laptop"]),
        );

        assert!(!people[0].deafened);
    }

    #[test]
    fn somebody_deafened_on_every_device_is_deafened() {
        let people = with_deafened(
            vec![person("@ada:example.org")],
            &[
                membership("ada-laptop", "@ada:example.org"),
                membership("ada-phone", "@ada:example.org"),
            ],
            &ids(&["ada-laptop", "ada-phone"]),
        );

        assert!(people[0].deafened);
    }

    #[test]
    fn somebody_with_no_membership_at_all_is_not_deafened() {
        // `all` is true of nothing, so without the emptiness check this is the
        // case that would mark a person with no memberships as deafened.
        let people = with_deafened(vec![person("@ada:example.org")], &[], &ids(&["ada-laptop"]));

        assert!(!people[0].deafened);
    }

    #[test]
    fn one_person_deafening_does_not_deafen_anybody_else() {
        let people = with_deafened(
            vec![person("@ada:example.org"), person("@bob:example.org")],
            &[
                membership("ada-laptop", "@ada:example.org"),
                membership("bob-phone", "@bob:example.org"),
            ],
            &ids(&["ada-laptop"]),
        );

        assert!(people[0].deafened);
        assert!(!people[1].deafened);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_rtc_media::StreamState;

    fn membership(user_id: &str, streams: Vec<StreamState>) -> MediaParticipant {
        MediaParticipant {
            member_id: format!("{user_id}:AAAA"),
            user_id: user_id.to_owned(),
            device_id: None,
            is_local: false,
            reachable: true,
            joined_at_ms: None,
            streams,
        }
    }

    fn microphone(muted: bool) -> StreamState {
        StreamState {
            kind: MediaStreamKind::Microphone,
            muted,
        }
    }

    fn ada() -> Participant {
        Participant::named("@ada:example.org", "Ada")
    }

    #[test]
    fn a_muted_microphone_is_a_muted_membership() {
        assert!(microphone_muted(&membership(
            "@ada:example.org",
            vec![microphone(true)]
        )));
    }

    #[test]
    fn a_live_microphone_is_not() {
        assert!(!microphone_muted(&membership(
            "@ada:example.org",
            vec![microphone(false)]
        )));
    }

    #[test]
    fn publishing_no_microphone_at_all_is_a_mute() {
        // The case this whole rule exists for. Joining from a lobby with the
        // microphone off publishes no audio track, so there is no muted
        // publication to find, only an absence. Reading that absence as "not
        // muted" left the person who cannot be heard looking exactly like the
        // person who can.
        assert!(microphone_muted(&membership("@ada:example.org", vec![])));
    }

    #[test]
    fn a_second_live_microphone_beats_a_muted_one() {
        // One membership publishing twice is unusual, but "can the call hear
        // them" is still answered by any live publication, not by the first
        // one found.
        assert!(!microphone_muted(&membership(
            "@ada:example.org",
            vec![microphone(true), microphone(false)]
        )));
    }

    #[test]
    fn a_camera_says_nothing_about_the_microphone() {
        let member = membership(
            "@ada:example.org",
            vec![
                StreamState {
                    kind: MediaStreamKind::Camera,
                    muted: true,
                },
                microphone(false),
            ],
        );

        assert!(!microphone_muted(&member));
    }

    #[test]
    fn our_own_arrival_is_the_one_this_session_wrote_down() {
        // Nothing else can supply it. The transport reports the people who
        // turn up after us and has no event for us, so this is the one row in
        // the call whose arrival has to come from the client that arrived.
        let ours = MediaParticipant {
            is_local: true,
            ..membership("@ada:example.org", vec![])
        };

        assert_eq!(
            arrived_at(&ours, 1_700_000_000_000),
            Some(1_700_000_000_000)
        );
    }

    #[test]
    fn nobody_else_borrows_the_time_we_joined() {
        // Ours and only ours. Lending it to somebody the call has not placed
        // yet would draw every new arrival as having been here as long as we
        // have.
        let theirs = membership("@ada:example.org", vec![]);

        assert_eq!(arrived_at(&theirs, 1_700_000_000_000), None);
    }

    #[test]
    fn the_transport_answers_for_everybody_it_can_place() {
        let theirs = MediaParticipant {
            joined_at_ms: Some(1_700_000_060_000),
            ..membership("@ada:example.org", vec![])
        };

        assert_eq!(
            arrived_at(&theirs, 1_700_000_000_000),
            Some(1_700_000_060_000)
        );
    }

    #[test]
    fn joining_twice_dates_from_the_first_time() {
        // A laptop, then a phone. They have been in the call since the laptop,
        // and picking up a second device is not arriving again.
        let people = with_since(
            vec![ada()],
            &[
                ("@ada:example.org".to_owned(), Some(1_700_000_060_000)),
                ("@ada:example.org".to_owned(), Some(1_700_000_000_000)),
            ],
        );

        assert_eq!(people[0].since, Some(1_700_000_000_000));
    }

    #[test]
    fn a_membership_with_no_time_yet_does_not_hide_one_that_has() {
        let people = with_since(
            vec![ada()],
            &[
                ("@ada:example.org".to_owned(), None),
                ("@ada:example.org".to_owned(), Some(1_700_000_000_000)),
            ],
        );

        assert_eq!(people[0].since, Some(1_700_000_000_000));
    }

    #[test]
    fn somebody_the_call_has_not_seen_has_no_join_time() {
        // Everybody drawn from room state, and anybody whose media has not
        // appeared yet. An interface asking "how long have they been here"
        // gets no answer rather than a made-up one.
        let people = with_since(vec![ada()], &[]);

        assert_eq!(people[0].since, None);
    }

    #[test]
    fn one_membership_carries_straight_through() {
        let people = with_mutes(vec![ada()], &[("@ada:example.org".to_owned(), true)]);

        assert!(people[0].muted);
    }

    #[test]
    fn somebody_on_two_devices_is_muted_only_when_both_are() {
        // The case this fold exists for. A laptop muted and a phone live is a
        // person the room can hear, and saying otherwise is the one wrong
        // answer worth avoiding here.
        let both = [
            ("@ada:example.org".to_owned(), true),
            ("@ada:example.org".to_owned(), true),
        ];
        let one = [
            ("@ada:example.org".to_owned(), true),
            ("@ada:example.org".to_owned(), false),
        ];

        assert!(with_mutes(vec![ada()], &both)[0].muted);
        assert!(!with_mutes(vec![ada()], &one)[0].muted);
    }

    #[test]
    fn somebody_no_membership_matches_is_not_muted() {
        // `all` is true of nothing, so this is the answer the obvious fold
        // gets wrong: a name with no membership behind it would come back
        // muted.
        let people = with_mutes(vec![ada()], &[("@bob:example.org".to_owned(), true)]);

        assert!(!people[0].muted);
    }

    #[test]
    fn each_person_gets_their_own_answer() {
        let people = with_mutes(
            vec![ada(), Participant::named("@bob:example.org", "Bob")],
            &[
                ("@ada:example.org".to_owned(), true),
                ("@bob:example.org".to_owned(), false),
            ],
        );

        assert!(people[0].muted);
        assert!(!people[1].muted);
    }
}

#[cfg(test)]
mod cameras {
    use super::*;
    use matrix_rtc_media::{MediaStreamKind, StreamState};

    fn ada() -> Participant {
        Participant::named("@ada:example.org", "Ada")
    }

    fn seen(user_id: &str, streams: Vec<StreamState>) -> MediaParticipant {
        MediaParticipant {
            member_id: format!("{user_id}:AAAA"),
            user_id: user_id.to_owned(),
            device_id: None,
            is_local: false,
            reachable: true,
            joined_at_ms: None,
            streams,
        }
    }

    fn lens(muted: bool) -> StreamState {
        StreamState {
            kind: MediaStreamKind::Camera,
            muted,
        }
    }

    #[test]
    fn a_live_camera_is_a_camera() {
        assert!(camera_live(&seen("@ada:example.org", vec![lens(false)])));
    }

    #[test]
    fn a_muted_camera_is_not() {
        // Element Call and Consort both publish the track and mute it rather
        // than tearing it down, so this is the state anybody who turns their
        // camera off mid-call is actually in.
        assert!(!camera_live(&seen("@ada:example.org", vec![lens(true)])));
    }

    #[test]
    fn publishing_no_camera_at_all_is_not_a_camera() {
        // The difference from the mute rule, and the one place these two
        // questions part company. Nothing published means the call cannot see
        // them, which is exactly what the crossed-out icon says. It is also the
        // state somebody who joins with their camera off is in, because no
        // client publishes a camera track before there is a camera to publish.
        assert!(!camera_live(&seen("@ada:example.org", vec![])));
        assert!(!camera_live(&seen(
            "@ada:example.org",
            vec![StreamState {
                kind: MediaStreamKind::Microphone,
                muted: false,
            }]
        )));
    }

    #[test]
    fn somebody_on_two_devices_is_on_camera_if_either_is() {
        // The opposite fold to the mute rule above, and right for the reason
        // behind that one rather than in spite of it: both are chosen so the
        // icon never claims less exposure than there is. A laptop camera live
        // and a phone in a pocket is somebody the call can see.
        let both = vec![
            ("@ada:example.org".to_owned(), false),
            ("@ada:example.org".to_owned(), true),
        ];
        let neither = vec![
            ("@ada:example.org".to_owned(), false),
            ("@ada:example.org".to_owned(), false),
        ];

        assert!(with_cameras(vec![ada()], &both)[0].camera);
        assert!(!with_cameras(vec![ada()], &neither)[0].camera);
    }

    #[test]
    fn somebody_no_membership_matches_has_no_camera() {
        // Room state lists who is in a channel and says nothing else about
        // them. Reporting a camera there would be an invention.
        let people = with_cameras(vec![ada()], &[("@bob:example.org".to_owned(), true)]);

        assert!(!people[0].camera);
    }

    #[test]
    fn a_camera_and_a_mute_are_asked_separately() {
        // Somebody muted with their camera on is ordinary, and so is the
        // reverse. Neither answer may be derived from the other.
        let member = seen(
            "@ada:example.org",
            vec![
                StreamState {
                    kind: MediaStreamKind::Microphone,
                    muted: true,
                },
                lens(false),
            ],
        );

        assert!(microphone_muted(&member));
        assert!(camera_live(&member));
    }
}
