// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Finding an SFU the way the deployments that exist actually advertise one.
//!
//! MSC4143 grew two discovery mechanisms and only one of them is widely
//! deployed. The newer one is an endpoint,
//! `GET /_matrix/client/unstable/org.matrix.msc4143/rtc/transports`, which
//! `matrix-rtc-livekit` asks first and which a homeserver older than it
//! answers with a 404. The older one is a key in the server's own discovery
//! document, `org.matrix.msc4143.rtc_foci` in `.well-known/matrix/client`,
//! which is what Element Call reads and therefore what every deployment that
//! has ever run Element Call already has.
//!
//! Upstream tries the endpoint and then a URL the operator typed into a
//! config file. That leaves the common case, a working Element Call
//! deployment on a homeserver that predates the endpoint, unable to join
//! without hand configuration, which is a bad first experience for a
//! deployment that is not doing anything wrong.
//!
//! So this module reads the other mechanism. It is the parsing half only: the
//! request lives in [`crate::livekit`], because CI has no server to ask and
//! everything that decides anything belongs where a test can reach it.
//!
//! ## Why the parsing is deliberately forgiving
//!
//! A discovery document is written by hand, by an operator, in YAML that
//! became JSON somewhere along the way. Entries with a typo, a focus type
//! nobody here supports, a list that is not a list: none of those are reasons
//! to refuse a perfectly good LiveKit URL sitting next to them. Anything this
//! module cannot make sense of is skipped rather than raised, and the result
//! is the same as if the key had been absent, which is the case upstream
//! already handles.

use std::net::{Ipv4Addr, Ipv6Addr};

/// The most of a discovery document worth reading.
///
/// A client discovery document is a handful of keys: a homeserver base URL,
/// perhaps an identity server, perhaps a list of foci. Real ones are a few
/// hundred bytes. This is not a limit on how large a valid one may be, because
/// there is no such thing; it is a limit on how much of a stranger's response
/// this process will hold. The host is named by the server name out of the
/// user's own ID, it is asked before anything has authenticated anything, and
/// an unbounded read of an unbounded body is an unbounded allocation.
pub const MAX_DOCUMENT: usize = 64 * 1024;

/// Take as much of a chunk as there is room for, and say whether to ask for
/// another.
///
/// Truncating rather than failing, because the outcome of a document this
/// module cannot parse is already "the server advertises no focus", which is
/// the same outcome as a document that genuinely does not. A caller that
/// stopped early has nothing better to say than that.
pub fn take_chunk(body: &mut Vec<u8>, chunk: &[u8]) -> bool {
    let room = MAX_DOCUMENT - body.len();
    if chunk.len() < room {
        body.extend_from_slice(chunk);
        return true;
    }

    body.extend_from_slice(&chunk[..room]);
    false
}

/// Where a server name publishes its client discovery document.
///
/// Keyed on the server name, the domain in a user ID, and not on the
/// homeserver's API base URL. Those differ whenever a deployment delegates:
/// `@someone:example.org` may well be served by `matrix.example.org`, and it
/// is `example.org` that publishes the document saying so. Asking the API host
/// instead is the mistake that makes delegated setups look like servers that
/// advertise nothing.
pub fn well_known_url(server_name: &str) -> String {
    format!("https://{server_name}/.well-known/matrix/client")
}

/// The LiveKit service URL a discovery document advertises, if it advertises
/// one.
///
/// The foci are ordered by priority, so the first usable one wins. "Usable"
/// means a LiveKit focus carrying an absolute HTTP URL: this value becomes the
/// address a Matrix OpenID token is presented to in exchange for an SFU token,
/// so a `file:` or a bare hostname is refused rather than passed on to be
/// puzzled over further down.
pub fn livekit_focus(document: &str) -> Option<String> {
    let document: serde_json::Value = serde_json::from_str(document).ok()?;

    // The unstable key first and the stable one as a fallback, matching the
    // order ruma reads them in. Every deployment in the wild writes the
    // unstable name today; the alias is here so that stopping does not
    // silently turn discovery off.
    let foci = document
        .get("org.matrix.msc4143.rtc_foci")
        .or_else(|| document.get("m.rtc_foci"))?
        .as_array()?;

    foci.iter().find_map(|focus| {
        if focus.get("type")?.as_str()? != "livekit" {
            return None;
        }

        let url = focus.get("livekit_service_url")?.as_str()?.trim();
        is_http_url(url).then(|| url.to_owned())
    })
}

/// Whether a string is an absolute HTTP URL this token request may be sent to.
///
/// `http` only to this machine. The URL is where a Matrix OpenID token is
/// exchanged for an SFU token, so in the clear anybody on the path takes both:
/// proof of this Matrix identity to a third party, and admission to the call.
/// That the document arrived over TLS says nothing about where it points.
fn is_http_url(candidate: &str) -> bool {
    if let Some(rest) = candidate.strip_prefix("https://") {
        return !rest.is_empty();
    }

    // A stack brought up on a developer machine is still reachable, and so is
    // a hand-configured `service_url_fallback`, which never comes through here.
    candidate
        .strip_prefix("http://")
        .map(authority_of)
        .is_some_and(is_loopback)
}

/// The host and port of a URL whose scheme has already been taken off.
fn authority_of(rest: &str) -> &str {
    rest.split(['/', '?', '#']).next().unwrap_or_default()
}

/// Whether an authority names this machine and nowhere else.
///
/// Userinfo is not parsed out, so `evil.example@127.0.0.1` is refused along
/// with everything else that is not a bare host. Refusing it is the safe half.
fn is_loopback(authority: &str) -> bool {
    if let Some(rest) = authority.strip_prefix('[') {
        // Bracketed, so the colons inside are the address and not a port.
        return rest.split_once(']').is_some_and(|(address, _port)| {
            address.parse::<Ipv6Addr>().is_ok_and(|ip| ip.is_loopback())
        });
    }

    let host = authority
        .split_once(':')
        .map_or(authority, |(host, _port)| host);

    host == "localhost" || host.parse::<Ipv4Addr>().is_ok_and(|ip| ip.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The document Element Call deployments actually serve.
    fn advertising(url: &str) -> String {
        format!(
            r#"{{
                "m.homeserver": {{ "base_url": "https://example.org" }},
                "org.matrix.msc4143.rtc_foci": [
                    {{ "type": "livekit", "livekit_service_url": "{url}" }}
                ]
            }}"#
        )
    }

    #[test]
    fn a_chunk_that_fits_is_taken_whole_and_leaves_room() {
        let mut body = Vec::new();

        assert!(take_chunk(&mut body, b"{\"m.homeserver\":"));
        assert!(take_chunk(&mut body, b" {}}"));

        assert_eq!(body, br#"{"m.homeserver": {}}"#);
    }

    #[test]
    fn a_chunk_that_overflows_is_cut_at_the_cap_and_ends_the_read() {
        let mut body = vec![b'x'; MAX_DOCUMENT - 4];

        assert!(!take_chunk(&mut body, &[b'y'; 64]));

        assert_eq!(body.len(), MAX_DOCUMENT);
        assert_eq!(&body[MAX_DOCUMENT - 4..], b"yyyy");
    }

    #[test]
    fn one_enormous_chunk_is_still_only_the_cap() {
        // The shape that matters: a hostile server does not have to send many
        // chunks to make this allocate, it can send one.
        let mut body = Vec::new();

        assert!(!take_chunk(&mut body, &vec![b'y'; MAX_DOCUMENT * 4]));

        assert_eq!(body.len(), MAX_DOCUMENT);
    }

    #[test]
    fn a_full_body_takes_nothing_more() {
        let mut body = vec![b'x'; MAX_DOCUMENT];

        assert!(!take_chunk(&mut body, b"more"));

        assert_eq!(body.len(), MAX_DOCUMENT);
    }

    #[test]
    fn a_truncated_document_advertises_nothing_rather_than_half_a_url() {
        let mut body = Vec::new();
        let whole = advertising("https://sfu.example.org");
        take_chunk(&mut body, &whole.as_bytes()[..whole.len() / 2]);

        assert_eq!(livekit_focus(&String::from_utf8(body).unwrap()), None);
    }

    #[test]
    fn a_well_known_url_is_built_from_the_server_name() {
        assert_eq!(
            well_known_url("example.org"),
            "https://example.org/.well-known/matrix/client"
        );
    }

    #[test]
    fn a_server_name_carrying_a_port_keeps_it() {
        assert_eq!(
            well_known_url("example.org:8448"),
            "https://example.org:8448/.well-known/matrix/client"
        );
    }

    #[test]
    fn the_livekit_focus_is_read_out_of_a_real_document() {
        assert_eq!(
            livekit_focus(&advertising("https://matrix-rtc.example.org/livekit/jwt")),
            Some("https://matrix-rtc.example.org/livekit/jwt".to_owned())
        );
    }

    #[test]
    fn the_stable_key_is_read_as_well_as_the_unstable_one() {
        let document = r#"{
            "m.rtc_foci": [
                { "type": "livekit", "livekit_service_url": "https://sfu.example.org" }
            ]
        }"#;

        assert_eq!(
            livekit_focus(document),
            Some("https://sfu.example.org".to_owned())
        );
    }

    #[test]
    fn the_unstable_key_is_preferred_when_a_document_carries_both() {
        let document = r#"{
            "org.matrix.msc4143.rtc_foci": [
                { "type": "livekit", "livekit_service_url": "https://unstable.example.org" }
            ],
            "m.rtc_foci": [
                { "type": "livekit", "livekit_service_url": "https://stable.example.org" }
            ]
        }"#;

        assert_eq!(
            livekit_focus(document),
            Some("https://unstable.example.org".to_owned())
        );
    }

    #[test]
    fn a_document_that_mentions_no_foci_advertises_nothing() {
        let document = r#"{ "m.homeserver": { "base_url": "https://example.org" } }"#;

        assert_eq!(livekit_focus(document), None);
    }

    #[test]
    fn an_empty_focus_list_advertises_nothing() {
        assert_eq!(
            livekit_focus(r#"{ "org.matrix.msc4143.rtc_foci": [] }"#),
            None
        );
    }

    #[test]
    fn foci_that_are_not_a_list_advertise_nothing() {
        let document = r#"{ "org.matrix.msc4143.rtc_foci": "https://sfu.example.org" }"#;

        assert_eq!(livekit_focus(document), None);
    }

    #[test]
    fn a_focus_of_another_type_is_stepped_over() {
        let document = r#"{
            "org.matrix.msc4143.rtc_foci": [
                { "type": "jitsi", "preferredDomain": "jitsi.example.org" },
                { "type": "livekit", "livekit_service_url": "https://sfu.example.org" }
            ]
        }"#;

        assert_eq!(
            livekit_focus(document),
            Some("https://sfu.example.org".to_owned())
        );
    }

    #[test]
    fn the_first_usable_focus_wins_because_the_list_is_a_priority_order() {
        let document = r#"{
            "org.matrix.msc4143.rtc_foci": [
                { "type": "livekit", "livekit_service_url": "https://first.example.org" },
                { "type": "livekit", "livekit_service_url": "https://second.example.org" }
            ]
        }"#;

        assert_eq!(
            livekit_focus(document),
            Some("https://first.example.org".to_owned())
        );
    }

    #[test]
    fn a_malformed_entry_does_not_hide_a_good_one_behind_it() {
        let document = r#"{
            "org.matrix.msc4143.rtc_foci": [
                { "livekit_service_url": "https://no-type.example.org" },
                { "type": 7 },
                "not even an object",
                { "type": "livekit", "livekit_service_url": "https://sfu.example.org" }
            ]
        }"#;

        assert_eq!(
            livekit_focus(document),
            Some("https://sfu.example.org".to_owned())
        );
    }

    #[test]
    fn a_livekit_focus_with_no_url_advertises_nothing() {
        let document = r#"{
            "org.matrix.msc4143.rtc_foci": [ { "type": "livekit" } ]
        }"#;

        assert_eq!(livekit_focus(document), None);
    }

    #[test]
    fn a_url_that_is_only_whitespace_is_not_a_url() {
        assert_eq!(livekit_focus(&advertising("   ")), None);
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_off_a_usable_url() {
        assert_eq!(
            livekit_focus(&advertising("  https://sfu.example.org  ")),
            Some("https://sfu.example.org".to_owned())
        );
    }

    #[test]
    fn a_url_that_is_not_http_is_refused() {
        for hostile in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "sfu.example.org",
        ] {
            assert_eq!(livekit_focus(&advertising(hostile)), None, "{hostile}");
        }
    }

    #[test]
    fn a_scheme_with_nothing_after_it_is_refused() {
        assert_eq!(livekit_focus(&advertising("https://")), None);
    }

    #[test]
    fn plain_http_is_allowed_so_a_local_stack_is_discoverable() {
        assert_eq!(
            livekit_focus(&advertising("http://localhost:8080")),
            Some("http://localhost:8080".to_owned())
        );
    }

    #[test]
    fn plain_http_to_anywhere_else_is_refused() {
        for reachable in [
            "http://sfu.example.org",
            "http://sfu.example.org:8080/path",
            "http://198.51.100.7",
            "http://[2001:db8::1]:8080",
            // The host is `evil.example`, not the loopback name in front of it.
            "http://localhost@evil.example/",
            // `127.0.0.1.evil.example` is a name, not an address.
            "http://127.0.0.1.evil.example/",
        ] {
            assert_eq!(livekit_focus(&advertising(reachable)), None, "{reachable}");
        }
    }

    #[test]
    fn the_rest_of_the_loopback_interface_counts_too() {
        for local in [
            "http://127.0.0.1:8080",
            "http://127.1.2.3:8080",
            "http://[::1]:8080",
        ] {
            assert_eq!(
                livekit_focus(&advertising(local)),
                Some(local.to_owned()),
                "{local}"
            );
        }
    }

    #[test]
    fn https_anywhere_is_still_what_a_deployment_advertises() {
        assert_eq!(
            livekit_focus(&advertising("https://sfu.example.org")),
            Some("https://sfu.example.org".to_owned())
        );
    }

    #[test]
    fn a_body_that_is_not_json_advertises_nothing() {
        assert_eq!(livekit_focus("<html>404 Not Found</html>"), None);
    }

    #[test]
    fn a_body_that_is_not_an_object_advertises_nothing() {
        assert_eq!(livekit_focus("[]"), None);
    }
}
