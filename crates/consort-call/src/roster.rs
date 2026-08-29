// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning memberships into people.
//!
//! A MatrixRTC roster is per membership, which is per device. What an interface
//! draws is per person. Collapsing the two is arithmetic over a list, with no
//! `Client` and no SFU behind it, so it lives here where a test can reach it
//! rather than in [`crate::livekit`] where nothing can.

use consort_matrix::Participant;
// `SpeakingMember` is not re-exported from the crate root; the module is
// public, so this is the path rather than a private reach-in.
use matrix_rtc_media::event::SpeakingMember;
use matrix_rtc_media::{MediaStreamKind, Participant as MediaParticipant};

/// Whether this membership has muted its microphone.
///
/// False for a membership publishing no microphone at all, which is not the
/// same event: it is somebody still connecting, or a client that publishes
/// nothing. Saying "muted" for that would put an icon on a person who has not
/// touched anything, describing a state they are on their way out of.
pub fn microphone_muted(member: &MediaParticipant) -> bool {
    member
        .streams
        .iter()
        .find(|stream| stream.kind == MediaStreamKind::Microphone)
        .is_some_and(|stream| stream.muted)
}

/// Whether this membership has a camera the call can see.
///
/// The microphone test with the other stream in it, and one deliberate
/// difference: a membership publishing no camera at all is not on camera,
/// where a membership publishing no microphone is not muted. That is not an
/// inconsistency, it is the same principle applied to two questions whose
/// honest default differs. "Muted" is a claim about somebody having chosen
/// something, and nothing published is not a choice. "On camera" is a claim
/// about what the call can see, and nothing published means it cannot see
/// them. Which is also how every client that predates video here behaves,
/// including Element Call, which publishes no camera track until somebody
/// turns one on.
pub fn camera_live(member: &MediaParticipant) -> bool {
    member
        .streams
        .iter()
        .any(|stream| stream.kind == MediaStreamKind::Camera && !stream.muted)
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

/// The people behind `speakers`, by Matrix user ID and without repeats.
///
/// The SFU answers in memberships, because that is what it has; a roster is
/// drawn per person. Somebody talking on their laptop with their phone also in
/// the call is one person talking, and saying their name twice would light two
/// entries that are the same human.
///
/// Speakers whose membership is not in the roster are dropped. That is the
/// window between a membership being signalled and this session having caught
/// up with it, and inventing a user ID for it would be worse than briefly
/// missing a green ring.
pub fn speaking_users(
    speakers: &[SpeakingMember],
    memberships: &[MediaParticipant],
) -> Vec<String> {
    let mut talking = Vec::new();

    for speaker in speakers {
        let Some(member) = memberships
            .iter()
            .find(|member| member.member_id == speaker.member_id)
        else {
            continue;
        };
        if !talking.contains(&member.user_id) {
            talking.push(member.user_id.clone());
        }
    }

    talking
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
mod speaking {
    use super::*;

    fn membership(member_id: &str, user_id: &str) -> MediaParticipant {
        MediaParticipant {
            member_id: member_id.to_owned(),
            user_id: user_id.to_owned(),
            device_id: None,
            is_local: false,
            reachable: true,
            streams: Vec::new(),
        }
    }

    fn talking(member_id: &str) -> SpeakingMember {
        SpeakingMember {
            member_id: member_id.to_owned(),
            level: 0.8,
        }
    }

    #[test]
    fn a_speaking_membership_names_its_person() {
        let people = speaking_users(
            &[talking("ada-laptop")],
            &[membership("ada-laptop", "@ada:example.org")],
        );

        assert_eq!(people, vec!["@ada:example.org".to_owned()]);
    }

    #[test]
    fn one_person_on_two_devices_is_named_once() {
        // The roster is per person, so naming them twice would light two
        // entries that are the same human.
        let people = speaking_users(
            &[talking("ada-laptop"), talking("ada-phone")],
            &[
                membership("ada-laptop", "@ada:example.org"),
                membership("ada-phone", "@ada:example.org"),
            ],
        );

        assert_eq!(people, vec!["@ada:example.org".to_owned()]);
    }

    #[test]
    fn a_speaker_this_session_has_not_heard_of_is_dropped() {
        // The window between a membership being signalled and this session
        // catching up. Inventing a user ID would be worse than a missing ring.
        let people = speaking_users(
            &[talking("a-stranger")],
            &[membership("ada-laptop", "@ada:example.org")],
        );

        assert!(people.is_empty());
    }

    #[test]
    fn nobody_talking_is_nobody_named() {
        assert!(speaking_users(&[], &[membership("ada-laptop", "@ada:example.org")]).is_empty());
    }

    #[test]
    fn everybody_talking_at_once_is_named_in_the_order_the_sfu_gave() {
        // Loudest first where the transport orders them, which is what makes
        // an interface that only has room for a few names show the right ones.
        let people = speaking_users(
            &[talking("bob-laptop"), talking("ada-laptop")],
            &[
                membership("ada-laptop", "@ada:example.org"),
                membership("bob-laptop", "@bob:example.org"),
            ],
        );

        assert_eq!(
            people,
            vec!["@bob:example.org".to_owned(), "@ada:example.org".to_owned()]
        );
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
    fn publishing_no_microphone_at_all_is_not_a_mute() {
        // Somebody still connecting, or a client that publishes nothing. They
        // have not touched a button, and an icon saying they have describes a
        // state they are on their way out of.
        assert!(!microphone_muted(&membership("@ada:example.org", vec![])));
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
