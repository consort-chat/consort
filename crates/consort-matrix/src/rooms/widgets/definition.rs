// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Turning one widget state event into something that can be drawn, or into
//! nothing at all.
//!
//! The URL here is the only value in Consort that a room member chooses and the
//! application then loads in its own webview, so this is the file that decides
//! what it will load. Every rule is a refusal, the list is a whitelist, and the
//! tests below are the specification.

use serde::Deserialize;
use serde_json::{Map, Value};
use url::{Origin, Url};

use super::dto::{Container, Viewer, Widget};
use super::template;

/// The part of `WidgetCommonProperties` this reads.
///
/// MSC2764 requires `id`, `creatorUserId` and `type`, and neither Element nor
/// any room in the wild carries all three reliably. What is checked instead is
/// what Element checks, for which see [`of`].
#[derive(Debug, Deserialize)]
struct Content {
    #[serde(rename = "type")]
    kind: Option<String>,
    url: Option<String>,
    name: Option<String>,
    #[serde(default)]
    data: Map<String, Value>,
}

/// The widget a state event describes, if it describes a drawable one.
///
/// `state_key` is the widget's ID and `content` is the event's content
/// verbatim, both as the homeserver sent them.
pub(super) fn of(state_key: &str, content: Value, viewer: &Viewer) -> Option<Widget> {
    // Nothing could address a widget with no ID: the layout keys by state key,
    // and so does every later read of the same room.
    if state_key.is_empty() {
        return None;
    }

    let content: Content = serde_json::from_value(content).ok()?;

    // Element's own filter, verbatim (`WidgetUtils.getRoomWidgets`): a type and
    // a URL, or it is not a widget. Removing a widget is done by writing empty
    // content over it, so this is the deletion check as much as a validity one,
    // and it is why MSC2764's `id` and `creatorUserId` are not checked here.
    // Real rooms do not carry them reliably and Element does not look.
    if content.kind?.trim().is_empty() {
        return None;
    }

    let template = content.url?;
    let origin = origin_of(&template)?;
    let url = template::fill(&template, &content.data, viewer);
    // Checked, templated, then checked again. A variable in the host is legal
    // in a template and its value is somebody else's to choose, so the URL that
    // was approved and the URL that would be loaded are not the same string.
    if origin_of(&url)? != origin {
        return None;
    }

    Some(Widget {
        id: state_key.to_owned(),
        // A blank label is worse than none, which is the rule
        // `rooms::facts::name_of` follows for a room.
        name: content.name.filter(|name| !name.trim().is_empty()),
        url,
        // The layout has not been read yet. `layout::arrange` is what moves a
        // widget off the default.
        container: Container::default(),
        width: None,
        height: None,
    })
}

/// The origin of a widget URL, or nothing when it is one this application must
/// not load.
fn origin_of(url: &str) -> Option<Origin> {
    let url = Url::parse(url).ok()?;

    // A whitelist. `javascript:` would run in the webview that holds the
    // session rather than in a frame, `data:` the same with its own markup,
    // `file:` would read the disk, and `http:` would put a plaintext page
    // inside an end-to-end encrypted client where anybody on the path can
    // rewrite it. A scheme the platform gains later is refused by default.
    if url.scheme() != "https" {
        return None;
    }

    // The oldest way of making a URL look like it points somewhere it does not,
    // and Consort draws no URL bar in which anybody could check.
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }

    Some(url.origin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn viewer() -> Viewer {
        Viewer {
            user_id: "@alice:example.org".to_owned(),
            room_id: "!room:example.org".to_owned(),
            display_name: "Alice".to_owned(),
        }
    }

    fn widget(content: Value) -> Option<Widget> {
        of("wordle", content, &viewer())
    }

    fn wordle() -> Value {
        json!({
            "type": "m.custom",
            "name": "Wordle",
            "url": "https://example.org/wordle",
        })
    }

    fn url_of(content: Value) -> Option<String> {
        widget(content).map(|widget| widget.url)
    }

    /// A widget whose URL is the given one and which is otherwise ordinary.
    fn at(url: &str) -> Value {
        json!({ "type": "m.custom", "url": url })
    }

    #[test]
    fn a_widget_with_a_type_and_an_https_url_is_drawable() {
        let widget = widget(wordle()).expect("an ordinary widget is drawable");

        assert_eq!(widget.id, "wordle");
        assert_eq!(widget.name.as_deref(), Some("Wordle"));
        assert_eq!(widget.url, "https://example.org/wordle");
        // The layout has not been read yet, and a widget it says nothing about
        // is not one that appears over the timeline.
        assert_eq!(widget.container, Container::Right);
        assert_eq!(widget.width, None);
        assert_eq!(widget.height, None);
    }

    #[test]
    fn the_widgets_id_is_the_state_key_and_not_the_one_in_the_content() {
        // `content.id` is a second copy nothing makes agree with the state key,
        // and the layout addresses widgets by state key. Believing the content
        // would let one widget answer to another one's layout entry.
        let widget = widget(json!({
            "type": "m.custom",
            "url": "https://example.org/wordle",
            "id": "something-else",
        }))
        .expect("a widget with a content id is still drawable");

        assert_eq!(widget.id, "wordle");
    }

    #[test]
    fn a_removed_widget_is_not_drawable() {
        // Removing a widget is writing empty content over it, so this is the
        // deletion check as much as a validity one.
        assert_eq!(widget(json!({})), None);
    }

    #[test]
    fn a_widget_with_no_url_is_not_drawable() {
        assert_eq!(widget(json!({ "type": "m.custom" })), None);
    }

    #[test]
    fn a_widget_with_no_type_is_not_drawable() {
        assert_eq!(widget(json!({ "url": "https://example.org/wordle" })), None);
    }

    #[test]
    fn a_widget_whose_type_is_blank_is_not_drawable() {
        assert_eq!(
            widget(json!({ "type": "  ", "url": "https://example.org/wordle" })),
            None
        );
    }

    #[test]
    fn a_widget_with_no_state_key_is_not_drawable() {
        // Nothing could address it: the layout keys by state key, and so does
        // every later read of the same room.
        assert_eq!(of("", wordle(), &viewer()), None);
    }

    #[test]
    fn a_javascript_url_is_refused() {
        // It would run in the webview that holds the session, not in a frame.
        assert_eq!(url_of(at("javascript:alert(1)")), None);
    }

    #[test]
    fn a_data_url_is_refused() {
        assert_eq!(url_of(at("data:text/html,<script>alert(1)</script>")), None);
    }

    #[test]
    fn a_file_url_is_refused() {
        assert_eq!(url_of(at("file:///etc/passwd")), None);
    }

    #[test]
    fn a_plain_http_url_is_refused() {
        // A plaintext page inside an end-to-end encrypted client, and anybody
        // on the path can rewrite what it loads.
        assert_eq!(url_of(at("http://example.org/wordle")), None);
    }

    #[test]
    fn a_scheme_nobody_here_knows_is_refused() {
        // The list is a whitelist, so a scheme added to the platform later is
        // refused by default rather than loaded by default.
        assert_eq!(url_of(at("consortmedia://example.org/x")), None);
        assert_eq!(url_of(at("vbscript:msgbox(1)")), None);
    }

    #[test]
    fn a_url_carrying_credentials_is_refused() {
        // The oldest way of making a URL look like it points somewhere else,
        // and Consort draws no URL bar in which anybody could see it.
        assert_eq!(
            url_of(at("https://example.org:pass@evil.example/wordle")),
            None
        );
        assert_eq!(url_of(at("https://user@evil.example/wordle")), None);
    }

    #[test]
    fn something_that_is_not_a_url_at_all_is_refused() {
        assert_eq!(url_of(at("not a url")), None);
        assert_eq!(url_of(at("")), None);
        assert_eq!(url_of(at("/wordle")), None);
    }

    #[test]
    fn a_template_variable_in_the_path_is_filled_in() {
        assert_eq!(
            url_of(json!({
                "type": "m.custom",
                "url": "https://example.org/wordle?room=$matrix_room_id",
            })),
            Some("https://example.org/wordle?room=%21room%3Aexample.org".to_owned())
        );
    }

    #[test]
    fn a_template_that_moves_the_origin_is_refused() {
        // The URL is checked, then templated, then checked again, because a
        // variable in the host is legal in the template and its value is
        // somebody else's to choose.
        assert_eq!(
            url_of(json!({
                "type": "m.custom",
                "url": "https://$host.example.org/wordle",
                "data": { "host": "evil" },
            })),
            None
        );
    }

    #[test]
    fn a_template_that_empties_the_host_is_refused() {
        assert_eq!(
            url_of(json!({
                "type": "m.custom",
                "url": "https://$host/wordle",
                "data": { "host": "evil.example" },
            })),
            None
        );
    }

    #[test]
    fn a_template_that_changes_the_port_is_refused() {
        assert_eq!(
            url_of(json!({
                "type": "m.custom",
                "url": "https://example.org:$port/wordle",
                "data": { "port": "8443" },
            })),
            None
        );
    }

    #[test]
    fn a_blank_name_is_no_name_at_all() {
        // Rendering a widget with an empty label is worse than rendering one
        // with none, which is the same rule `rooms::facts::name_of` follows.
        let widget = widget(json!({
            "type": "m.custom",
            "url": "https://example.org/wordle",
            "name": "   ",
        }))
        .expect("a blank name does not make a widget undrawable");

        assert_eq!(widget.name, None);
    }

    #[test]
    fn content_that_is_not_an_object_is_not_a_widget() {
        assert_eq!(of("wordle", json!("nonsense"), &viewer()), None);
        assert_eq!(of("wordle", Value::Null, &viewer()), None);
    }

    #[test]
    fn a_url_that_is_not_a_string_is_not_a_url() {
        assert_eq!(
            of("wordle", json!({ "type": "m.custom", "url": 7 }), &viewer()),
            None
        );
    }
}
