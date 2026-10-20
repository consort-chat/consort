// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Where a room wants its widgets, and in what order.
//!
//! `io.element.widgets.layout`, whose empty state key carries one entry per
//! widget it places. Element's own documentation is the only description of it
//! there is (`docs/widget-layouts.md` in element-hq/element-web): a `widgets`
//! object keyed by widget ID, each entry carrying `container` (`top` or
//! `right`), `index`, `width` and `height`.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use super::dto::{Container, Widget};

/// A percentage, which is what the layout's two sizes are.
const WIDEST: f64 = 100.0;

/// What one widget's layout entry says.
#[derive(Debug, Deserialize)]
struct Placement {
    container: Option<String>,
    index: Option<i64>,
    width: Option<f64>,
    height: Option<f64>,
}

/// The layout event's content.
#[derive(Debug, Default, Deserialize)]
struct Layout {
    #[serde(default)]
    widgets: BTreeMap<String, Placement>,
}

/// Put `widgets` where `layout` asks for them, in the order it asks for.
///
/// `layout` is the content of the room's `io.element.widgets.layout` event, or
/// `None` when the room has none.
pub(super) fn arrange(widgets: Vec<Widget>, layout: Option<Value>) -> Vec<Widget> {
    // A room can hold any JSON under that state key. One that is not a layout
    // places nothing, and a widget that cannot be placed is still a widget the
    // room has.
    let layout: Layout = layout
        .and_then(|content| serde_json::from_value(content).ok())
        .unwrap_or_default();

    let mut placed: Vec<(Container, Option<i64>, Widget)> = widgets
        .into_iter()
        .map(|widget| {
            let Some(placement) = layout.widgets.get(&widget.id) else {
                return (widget.container, None, widget);
            };

            let container = container_of(placement.container.as_deref());
            (
                container,
                placement.index,
                Widget {
                    container,
                    width: percentage(placement.width),
                    height: percentage(placement.height),
                    ..widget
                },
            )
        })
        .collect();

    // An index nobody set sorts after one somebody did, which Element's
    // documentation does not cover either way. The widget ID breaks the last
    // tie, and not for tidiness: an order that depends on what the state store
    // happened to return is an order that changes on every sync, and the panel
    // would reshuffle itself in front of whoever is looking at it.
    placed.sort_by(|one, two| {
        one.0
            .cmp(&two.0)
            .then_with(|| one.1.is_none().cmp(&two.1.is_none()))
            .then_with(|| one.1.cmp(&two.1))
            .then_with(|| one.2.id.cmp(&two.2.id))
    });

    placed.into_iter().map(|(_, _, widget)| widget).collect()
}

/// Which container a layout entry names.
fn container_of(name: Option<&str>) -> Container {
    // A name nobody here knows is not refused: a container Element adds later
    // should leave the widget listed and out of the way rather than make it
    // disappear from a room that still has it.
    match name {
        Some("top") => Container::Top,
        _ => Container::Right,
    }
}

/// A layout size brought into the range a percentage has.
fn percentage(size: Option<f64>) -> Option<u8> {
    // Element clamps these, and the reason to do the same is that the numbers
    // reach a layout: a height of 4000 is a widget filling the room with no
    // timeline left under it, and nobody typed it on purpose.
    size.filter(|size| !size.is_nan())
        .map(|size| size.clamp(0.0, WIDEST) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn widget(id: &str) -> Widget {
        Widget {
            id: id.to_owned(),
            name: None,
            url: format!("https://example.org/{id}"),
            container: Container::Right,
            width: None,
            height: None,
        }
    }

    fn arranged(ids: &[&str], layout: Value) -> Vec<Widget> {
        arrange(ids.iter().map(|id| widget(id)).collect(), Some(layout))
    }

    fn ids(widgets: &[Widget]) -> Vec<&str> {
        widgets.iter().map(|one| one.id.as_str()).collect()
    }

    fn containers(widgets: &[Widget]) -> Vec<Container> {
        widgets.iter().map(|one| one.container).collect()
    }

    fn only(ids: &[&str], layout: Value) -> Widget {
        arranged(ids, layout)
            .into_iter()
            .next()
            .expect("the layout places the widgets it is given")
    }

    #[test]
    fn a_widget_the_layout_puts_at_the_top_is_at_the_top() {
        let widget = only(
            &["wordle"],
            json!({ "widgets": { "wordle": { "container": "top" } } }),
        );

        assert_eq!(widget.container, Container::Top);
    }

    #[test]
    fn a_widget_the_layout_says_nothing_about_stays_beside_the_timeline() {
        // Element's behaviour, and the reason adding a widget does not put it
        // over somebody's timeline without them asking.
        let widget = only(&["wordle"], json!({ "widgets": {} }));

        assert_eq!(widget.container, Container::Right);
    }

    #[test]
    fn a_room_with_no_layout_event_leaves_every_widget_beside_the_timeline() {
        let arranged = arrange(vec![widget("wordle"), widget("clock")], None);

        assert_eq!(containers(&arranged), vec![Container::Right; 2]);
    }

    #[test]
    fn the_top_widgets_come_before_the_others() {
        let arranged = arranged(
            &["clock", "wordle"],
            json!({ "widgets": { "wordle": { "container": "top" } } }),
        );

        assert_eq!(ids(&arranged), vec!["wordle", "clock"]);
    }

    #[test]
    fn the_index_orders_the_widgets_in_a_container() {
        // Element's documentation: a smaller index is further left.
        let arranged = arranged(
            &["a", "b", "c"],
            json!({ "widgets": {
                "a": { "container": "top", "index": 2 },
                "b": { "container": "top", "index": 0 },
                "c": { "container": "top", "index": 1 },
            } }),
        );

        assert_eq!(ids(&arranged), vec!["b", "c", "a"]);
    }

    #[test]
    fn a_widget_with_no_index_follows_the_ones_that_have_one() {
        // Element's documentation does not say what an absent index means, so
        // this is Consort's answer rather than a reading of anybody's: a widget
        // somebody ordered comes before one nobody did.
        let arranged = arranged(
            &["unordered", "ordered"],
            json!({ "widgets": {
                "unordered": { "container": "top" },
                "ordered": { "container": "top", "index": 9 },
            } }),
        );

        assert_eq!(ids(&arranged), vec!["ordered", "unordered"]);
    }

    #[test]
    fn widgets_the_layout_cannot_separate_are_ordered_by_id() {
        // Not for its own sake: an order that depends on what the state store
        // happened to return is an order that changes on every sync, and the
        // panel would reshuffle itself in front of somebody.
        let arranged = arranged(
            &["b", "a", "c"],
            json!({ "widgets": {
                "a": { "container": "top", "index": 1 },
                "b": { "container": "top", "index": 1 },
                "c": { "container": "top", "index": 1 },
            } }),
        );

        assert_eq!(ids(&arranged), vec!["a", "b", "c"]);
    }

    #[test]
    fn the_sizes_are_carried_across() {
        let widget = only(
            &["wordle"],
            json!({ "widgets": { "wordle": { "container": "top", "width": 60, "height": 40 } } }),
        );

        assert_eq!(widget.width, Some(60));
        assert_eq!(widget.height, Some(40));
    }

    #[test]
    fn a_size_outside_a_percentage_is_brought_to_the_nearest_end() {
        // Element clamps these, and the reason to do the same is that the
        // numbers reach a layout: a height of 4000 is a widget filling the room
        // with no timeline under it, and nobody typed it on purpose.
        let widget = only(
            &["wordle"],
            json!({ "widgets": { "wordle": { "width": 4000, "height": -5 } } }),
        );

        assert_eq!(widget.width, Some(100));
        assert_eq!(widget.height, Some(0));
    }

    #[test]
    fn a_size_that_is_not_a_number_is_no_size_at_all() {
        let widget = only(
            &["wordle"],
            json!({ "widgets": { "wordle": { "width": "wide" } } }),
        );

        assert_eq!(widget.width, None);
    }

    #[test]
    fn a_container_name_nobody_here_knows_is_treated_as_the_default_one() {
        // Not refused: a container Element adds later should leave the widget
        // listed and out of the way, rather than make it vanish.
        let widget = only(
            &["wordle"],
            json!({ "widgets": { "wordle": { "container": "centre" } } }),
        );

        assert_eq!(widget.container, Container::Right);
    }

    #[test]
    fn a_layout_naming_a_widget_the_room_does_not_have_places_nothing() {
        let arranged = arranged(
            &["wordle"],
            json!({ "widgets": { "ghost": { "container": "top" } } }),
        );

        assert_eq!(ids(&arranged), vec!["wordle"]);
        assert_eq!(containers(&arranged), vec![Container::Right]);
    }

    #[test]
    fn a_layout_that_is_not_a_layout_places_nothing() {
        // A room can hold any JSON under that state key, and a widget that
        // cannot be placed is still a widget the room has.
        for nonsense in [json!("top"), json!({ "widgets": 7 }), Value::Null] {
            let arranged = arrange(vec![widget("wordle")], Some(nonsense));

            assert_eq!(ids(&arranged), vec!["wordle"]);
            assert_eq!(containers(&arranged), vec![Container::Right]);
        }
    }

    #[test]
    fn no_widgets_arrange_to_no_widgets() {
        assert!(arrange(Vec::new(), Some(json!({ "widgets": {} }))).is_empty());
    }
}
