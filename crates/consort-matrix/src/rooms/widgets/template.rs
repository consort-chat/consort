// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Filling in a widget URL's template variables.
//!
//! MSC2764's `URL Templating` section: the URL a widget carries is a template,
//! variable names are the keys of the widget's `data` object plus four
//! defaults, and the client MUST escape the values it substitutes. Read at
//! `specification/widgets.rst` on the MSC2764 branch.

use std::collections::BTreeMap;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::{Map, Value};

use super::dto::Viewer;

/// Everything but RFC 3986's unreserved characters.
///
/// A template value is data. One carrying `/`, `?`, `#`, `:` or `@` must not be
/// able to move the widget to another path, let alone another origin, and the
/// spec's own worked example (`test:value` becoming `test%3Avalue`) wants this
/// much encoding anyway.
const VARIABLE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Substitute `template`'s variables.
pub(super) fn fill(template: &str, data: &Map<String, Value>, viewer: &Viewer) -> String {
    let variables = variables(data, viewer);
    let mut filled = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(at) = rest.find('$') {
        filled.push_str(&rest[..at]);
        let after = &rest[at + 1..];

        match variables
            .iter()
            .find(|(name, _)| after.starts_with(name.as_str()))
        {
            Some((name, value)) => {
                filled.extend(utf8_percent_encode(value, VARIABLE));
                rest = &after[name.len()..];
            }
            // A `$` naming nothing is a literal `$`. Writing it out and moving
            // past it is also what makes the pass single: a substituted value
            // lands in `filled` and is never read again, which is the spec's
            // rule that nested variables are not supported.
            None => {
                filled.push('$');
                rest = after;
            }
        }
    }

    filled.push_str(rest);
    filled
}

/// The variables in scope, longest name first.
fn variables(data: &Map<String, Value>, viewer: &Viewer) -> Vec<(String, String)> {
    let mut variables: BTreeMap<String, String> = data
        .iter()
        .filter_map(|(name, value)| simple(value).map(|value| (name.clone(), value)))
        .collect();

    // Inserted after the widget's own data, because the spec says these four
    // take priority over anything it names. Without that, a widget could claim
    // to be in a room it is not in and be believed.
    variables.insert("matrix_user_id".to_owned(), viewer.user_id.clone());
    variables.insert("matrix_room_id".to_owned(), viewer.room_id.clone());
    variables.insert(
        "matrix_display_name".to_owned(),
        viewer.display_name.clone(),
    );
    // `$matrix_avatar_url` wants an HTTP URL for the viewer's avatar, and on a
    // homeserver that has moved to authenticated media there is none that does
    // not carry this session's access token. The spec's own value for an absent
    // avatar is the empty string: docs/adr/0016-what-a-widget-is-allowed-to-load.md
    variables.insert("matrix_avatar_url".to_owned(), String::new());

    let mut variables: Vec<_> = variables.into_iter().collect();
    // Longest name first, so that `$matrix_room_id` is not read as `$matrix_room`
    // followed by a literal `_id`. The name itself breaks a tie, because two
    // names of one length still have to resolve the same way every time.
    variables.sort_by(|one, two| {
        two.0
            .len()
            .cmp(&one.0.len())
            .then_with(|| one.0.cmp(&two.0))
    });
    variables
}

/// The value of a `data` entry, when it has one a URL can carry.
fn simple(value: &Value) -> Option<String> {
    // The spec gives objects and arrays no defined behaviour and encourages
    // widget authors to stick to these three, so the others are left in the URL
    // as the literal `$name` their author will notice.
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            _ => panic!("a widget's data is an object"),
        }
    }

    fn viewer() -> Viewer {
        Viewer {
            user_id: "@alice:example.org".to_owned(),
            room_id: "!room:example.org".to_owned(),
            display_name: "Alice".to_owned(),
        }
    }

    fn filled(template: &str, values: Value) -> String {
        fill(template, &data(values), &viewer())
    }

    #[test]
    fn the_specs_own_worked_example_comes_out_as_the_spec_says() {
        assert_eq!(
            filled(
                "https://example.com?var1=$hello&answer=$answer",
                json!({ "hello": "world", "answer": 42 })
            ),
            "https://example.com?var1=world&answer=42"
        );
    }

    #[test]
    fn the_default_variables_are_substituted() {
        assert_eq!(
            filled(
                "https://example.org/?r=$matrix_room_id&u=$matrix_user_id&n=$matrix_display_name",
                json!({})
            ),
            "https://example.org/?r=%21room%3Aexample.org&u=%40alice%3Aexample.org&n=Alice"
        );
    }

    #[test]
    fn a_default_variable_wins_over_the_same_name_in_data() {
        // The spec says the defaults MUST take priority. Without that, a widget
        // could name itself a room it is not in and be believed.
        assert_eq!(
            filled(
                "https://example.org/?r=$matrix_room_id",
                json!({ "matrix_room_id": "!elsewhere:example.org" })
            ),
            "https://example.org/?r=%21room%3Aexample.org"
        );
    }

    #[test]
    fn a_value_is_escaped_rather_than_pasted_in() {
        assert_eq!(
            filled("https://example.org/$v", json!({ "v": "test:value" })),
            "https://example.org/test%3Avalue"
        );
    }

    #[test]
    fn a_value_cannot_add_a_path_or_a_query_of_its_own() {
        // The guard that stops a widget's own data reaching past the URL its
        // author wrote.
        assert_eq!(
            filled(
                "https://example.org/$v",
                json!({ "v": "../../admin?token=1#x" })
            ),
            "https://example.org/..%2F..%2Fadmin%3Ftoken%3D1%23x"
        );
    }

    #[test]
    fn a_value_naming_another_variable_is_left_as_it_stands() {
        // The spec: nested variables are not supported, and the literal
        // `$answer` is what the URL gets.
        assert_eq!(
            filled(
                "https://example.org/?v=$hello",
                json!({ "hello": "$answer", "answer": 42 })
            ),
            "https://example.org/?v=%24answer"
        );
    }

    #[test]
    fn the_longest_variable_name_at_a_position_wins() {
        // Without this, `$ab` is read as `$a` followed by a literal `b`.
        assert_eq!(
            filled(
                "https://example.org/?v=$ab",
                json!({ "a": "one", "ab": "two" })
            ),
            "https://example.org/?v=two"
        );
    }

    #[test]
    fn a_dollar_naming_nothing_stays_a_dollar() {
        assert_eq!(
            filled("https://example.org/?v=$nope&w=$", json!({})),
            "https://example.org/?v=$nope&w=$"
        );
    }

    #[test]
    fn numbers_and_booleans_are_substituted_and_objects_and_arrays_are_not() {
        // The spec gives complex types no defined behaviour, so they are left
        // for the widget's author to see rather than guessed at.
        assert_eq!(
            filled(
                "https://example.org/?n=$n&b=$b&o=$o&a=$a&z=$z",
                json!({ "n": 42, "b": true, "o": { "x": 1 }, "a": [1], "z": null })
            ),
            "https://example.org/?n=42&b=true&o=$o&a=$a&z=$z"
        );
    }

    #[test]
    fn the_viewers_avatar_is_never_handed_to_a_widget() {
        // `$matrix_avatar_url` wants an HTTP URL for the viewer's avatar. On a
        // homeserver that has moved to authenticated media there is no such URL
        // that does not carry this session's access token, and handing that to a
        // remote origin would be giving it the account. The spec's own value for
        // an absent avatar is the empty string, so that is what it gets.
        assert_eq!(
            filled("https://example.org/?a=$matrix_avatar_url", json!({})),
            "https://example.org/?a="
        );
    }

    #[test]
    fn a_widget_cannot_name_its_own_avatar_variable_either() {
        assert_eq!(
            filled(
                "https://example.org/?a=$matrix_avatar_url",
                json!({ "matrix_avatar_url": "https://evil.example/pixel" })
            ),
            "https://example.org/?a="
        );
    }

    #[test]
    fn a_template_with_no_variables_is_returned_as_it_was() {
        assert_eq!(
            filled("https://example.org/wordle", json!({})),
            "https://example.org/wordle"
        );
    }

    #[test]
    fn the_same_variable_twice_is_substituted_twice() {
        assert_eq!(
            filled("https://example.org/$v/$v", json!({ "v": "x" })),
            "https://example.org/x/x"
        );
    }
}
