// Copyright 2026 The Consort contributors
// SPDX-License-Identifier: AGPL-3.0-only

//! Slash commands: what a line beginning with `/` turns into.
//!
//! Here rather than in the composer because the split between a command and a
//! message is behaviour worth testing, and `State<'_, AppState>` is not
//! reachable from a test. The frontend hands over the raw line and gets back
//! either something to send or a refusal.
//!
//! The table is Element's, read off `element-hq/element-web@develop`, because
//! parity is the point: a command that resolves differently here is a command
//! that works in one client.

use matrix_sdk::ruma::events::room::message::{MessageType, RoomMessageEventContent};
use matrix_sdk::ruma::serde::JsonObject;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// One of the six animations a message can ask for.
///
/// A closed set rather than the msgtype string, so nothing downstream has to
/// hold the string that names it. [`effect_of`] is what turns one into this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Effect {
    Confetti,
    Fireworks,
    Rainfall,
    Snowfall,
    SpaceInvaders,
    Hearts,
}

/// One effect, as Element defines it.
struct Chat {
    effect: Effect,
    /// What somebody types, without the slash.
    command: &'static str,
    /// The `msgtype` the message carries when it has words of its own.
    msgtype: &'static str,
    /// The body an effect with nothing after it sends, as an emote.
    fallback: &'static str,
    /// Emoji that play it when a plain message merely contains one.
    emojis: &'static [&'static str],
}

/// The six, with their msgtypes copied rather than composed.
///
/// `io.element.effects.space_invaders` is plural and underscored where the
/// other five are singular. That is upstream's inconsistency and copying it
/// exactly is the whole point: a composed string would be an effect that fires
/// here and nowhere else.
const EFFECTS: &[Chat] = &[
    Chat {
        effect: Effect::Confetti,
        command: "confetti",
        msgtype: "nic.custom.confetti",
        fallback: "sends confetti \u{1F389}",
        emojis: &["\u{1F38A}", "\u{1F389}"],
    },
    Chat {
        effect: Effect::Fireworks,
        command: "fireworks",
        msgtype: "nic.custom.fireworks",
        fallback: "sends fireworks \u{1F386}",
        emojis: &["\u{1F386}"],
    },
    Chat {
        effect: Effect::Rainfall,
        command: "rainfall",
        msgtype: "io.element.effect.rainfall",
        fallback: "sends rainfall \u{1F327}\u{FE0F}",
        emojis: &["\u{1F327}\u{FE0F}", "\u{26C8}\u{FE0F}", "\u{1F326}\u{FE0F}"],
    },
    Chat {
        effect: Effect::Snowfall,
        command: "snowfall",
        msgtype: "io.element.effect.snowfall",
        fallback: "sends snowfall \u{2744}",
        emojis: &["\u{2744}", "\u{1F328}"],
    },
    Chat {
        effect: Effect::SpaceInvaders,
        command: "spaceinvaders",
        msgtype: "io.element.effects.space_invaders",
        fallback: "sends space invaders \u{1F47E}",
        emojis: &["\u{1F47E}", "\u{1F30C}"],
    },
    Chat {
        effect: Effect::Hearts,
        command: "hearts",
        msgtype: "io.element.effect.hearts",
        fallback: "sends hearts \u{1F49D}",
        emojis: &["\u{1F49D}"],
    },
];

/// The four emoticons, each a prefix and whatever was typed after it.
///
/// Sent as plain text rather than as markdown, which matters for more than
/// tidiness: every one of these is punctuation, and `¯\_(ツ)_/¯` read as
/// markdown loses the backslash and turns the underscores into emphasis.
const EMOTICONS: &[(&str, &str)] = &[
    ("shrug", "\u{AF}\\_(\u{30C4})_/\u{AF}"),
    (
        "tableflip",
        "(\u{256F}\u{B0}\u{25A1}\u{B0}\u{FF09}\u{256F}\u{FE35} \u{253B}\u{2501}\u{253B}",
    ),
    (
        "unflip",
        "\u{252C}\u{2500}\u{2500}\u{252C} \u{30CE}( \u{309C}-\u{309C}\u{30CE})",
    ),
    ("lenny", "( \u{361}\u{B0} \u{35C}\u{296} \u{361}\u{B0})"),
];

/// What a line in the composer turned out to be.
#[derive(Debug)]
pub struct Resolution {
    content: RoomMessageEventContent,
    effect: Option<Effect>,
}

impl Resolution {
    /// The animation to play on this machine, if the line asked for one.
    ///
    /// Sending an effect plays it here too, the way it plays for everybody
    /// else in the room. Nothing comes back off the wire to trigger it: this
    /// build has no local echo, so waiting for the sync would be a second or
    /// two of nothing after the keypress.
    pub fn effect(&self) -> Option<Effect> {
        self.effect
    }

    /// The event to send.
    pub(crate) fn into_content(self) -> RoomMessageEventContent {
        self.content
    }

    /// An ordinary message, with no effect attached.
    fn said(content: RoomMessageEventContent) -> Self {
        Self {
            content,
            effect: None,
        }
    }
}

/// The effect a `msgtype` names, or `None` for every other message.
pub fn effect_of(msgtype: &str) -> Option<Effect> {
    EFFECTS
        .iter()
        .find(|chat| chat.msgtype == msgtype)
        .map(|chat| chat.effect)
}

/// The effect a message's words ask for by carrying an emoji, or `None`.
///
/// Element plays an effect when a plain message merely contains one of these,
/// which is why a room bursts into confetti when somebody types 🎉 without
/// meaning to. Matching that is parity rather than an accident worth keeping
/// out.
pub fn effect_in(body: &str) -> Option<Effect> {
    EFFECTS
        .iter()
        .find(|chat| chat.emojis.iter().any(|emoji| body.contains(emoji)))
        .map(|chat| chat.effect)
}

/// What was typed, as something to send.
///
/// Three rules, and each is here rather than in the composer:
///
/// - A command nobody recognises is a refusal. Sending `/pin this` to the room
///   because Consort did not know the word is public and cannot be taken back.
/// - `/plain` is the escape hatch, and so is a doubled slash, which is
///   Element's other one: `//me` sends the three characters `/me`.
/// - The slash only counts at the start. One mid-message is a slash.
pub fn resolve(body: &str) -> Result<Resolution> {
    // Trimmed before it is judged empty, so a stray newline is not a message.
    if body.trim().is_empty() {
        return Err(Error::EmptyMessage);
    }

    // Leading whitespace skipped, the way Element skips it, so a line that
    // starts with a space still starts with a command.
    let line = body.trim_start();
    let Some(rest) = line.strip_prefix('/') else {
        return Ok(Resolution::said(RoomMessageEventContent::text_markdown(
            body,
        )));
    };

    // The doubled slash, before anything is looked up: `//` is how somebody
    // says they meant the slash. One of the two is eaten and the rest is a
    // message, markdown and all.
    if rest.starts_with('/') {
        return Ok(Resolution::said(RoomMessageEventContent::text_markdown(
            rest,
        )));
    }

    let (command, args) = split_first_word(rest);
    run(command, args)
}

/// One command and whatever was typed after it.
///
/// Split at the first whitespace rather than the first space, so `/me` on its
/// own line carries the lines under it as its argument rather than becoming a
/// command nobody has heard of.
fn split_first_word(rest: &str) -> (&str, &str) {
    match rest.find(char::is_whitespace) {
        None => (rest, ""),
        Some(at) => (&rest[..at], rest[at..].trim_start()),
    }
}

/// One command, resolved.
fn run(command: &str, args: &str) -> Result<Resolution> {
    if let Some((_, prefix)) = EMOTICONS.iter().find(|(name, _)| *name == command) {
        let said = if args.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix} {args}")
        };
        return Ok(Resolution::said(RoomMessageEventContent::text_plain(said)));
    }

    if let Some(chat) = EFFECTS.iter().find(|chat| chat.command == command) {
        // With nothing after it, an emote saying what it is: the effect is the
        // message. With words, the words are the message and the msgtype is
        // what asks for the animation.
        let content = if args.is_empty() {
            RoomMessageEventContent::emote_plain(chat.fallback)
        } else {
            custom(chat.msgtype, args)
        };
        return Ok(Resolution {
            content,
            effect: Some(chat.effect),
        });
    }

    match command {
        // An emote, which is a different event type rather than differently
        // drawn text. Read as markdown, like anything else somebody writes.
        "me" => Ok(Resolution::said(RoomMessageEventContent::emote_markdown(
            needs_words(args)?,
        ))),
        // The escape hatch. No markdown either, so a line of asterisks
        // survives as a line of asterisks.
        "plain" => Ok(Resolution::said(RoomMessageEventContent::text_plain(
            needs_words(args)?,
        ))),
        // What was typed, as the formatted body and as the fallback both. The
        // tags are the point, so nothing escapes them.
        "html" => {
            let said = needs_words(args)?;
            Ok(Resolution::said(RoomMessageEventContent::text_html(
                said, said,
            )))
        }
        // Hidden until a reader asks for it. Escaped, unlike `/html`, because
        // the words here are words: somebody spoilering `a < b` means the
        // characters, and `/html` is next door for anybody who does not.
        "spoiler" => {
            let said = needs_words(args)?;
            Ok(Resolution::said(RoomMessageEventContent::text_html(
                said,
                format!("<span data-mx-spoiler>{}</span>", escaped(said)),
            )))
        }
        _ => Err(Error::UnknownCommand {
            command: command.to_owned(),
        }),
    }
}

/// `args`, or a refusal when a command that needs words was given none.
fn needs_words(args: &str) -> Result<&str> {
    if args.trim().is_empty() {
        return Err(Error::EmptyMessage);
    }
    Ok(args)
}

/// `said` with the three characters that would be read as markup turned back
/// into text.
fn escaped(said: &str) -> String {
    said.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A message carrying a msgtype ruma has no variant for.
fn custom(msgtype: &str, body: &str) -> RoomMessageEventContent {
    RoomMessageEventContent::new(
        MessageType::new(msgtype, body.to_owned(), JsonObject::new())
            // Infallible here: every msgtype in the table above is a custom
            // one, and `MessageType::new` only deserialises for the ones it
            // has a variant for.
            .expect("an effect msgtype has no ruma variant to deserialise into"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What a resolution would put on the wire: msgtype and body.
    fn sent(body: &str) -> (String, String) {
        let content = resolve(body).expect("the fixture resolves").into_content();
        (
            content.msgtype.msgtype().to_owned(),
            content.msgtype.body().to_owned(),
        )
    }

    /// The `formatted_body` a resolution carries, if it carries one.
    fn html(body: &str) -> Option<String> {
        let content = resolve(body).expect("the fixture resolves").into_content();
        match content.msgtype {
            MessageType::Text(text) => text.formatted.map(|formatted| formatted.body),
            MessageType::Emote(emote) => emote.formatted.map(|formatted| formatted.body),
            _ => None,
        }
    }

    #[test]
    fn an_ordinary_message_is_not_a_command() {
        assert_eq!(
            sent("hello everybody"),
            ("m.text".to_owned(), "hello everybody".to_owned())
        );
    }

    #[test]
    fn a_slash_in_the_middle_of_a_message_is_a_slash() {
        assert_eq!(
            sent("and/or this one"),
            ("m.text".to_owned(), "and/or this one".to_owned())
        );
    }

    #[test]
    fn a_slash_on_the_second_line_is_a_slash() {
        let (msgtype, body) = sent("look\n/me waves");
        assert_eq!(msgtype, "m.text");
        assert_eq!(body, "look\n/me waves");
    }

    #[test]
    fn a_command_still_counts_after_a_leading_space() {
        assert_eq!(sent(" /me waves").0, "m.emote");
    }

    /*
      The failure the parse exists to prevent. A command nobody recognises must
      not become a message: it goes to the room in front of everybody and it
      cannot be taken back.
    */
    #[test]
    fn an_unknown_command_is_refused_and_never_becomes_a_message() {
        let refused = resolve("/pin this").expect_err("an unknown command is refused");
        assert!(
            matches!(&refused, Error::UnknownCommand { command } if command == "pin"),
            "expected an unknown command, got {refused:?}"
        );
    }

    #[test]
    fn an_unknown_command_on_its_own_is_refused_too() {
        assert!(matches!(
            resolve("/rageshake"),
            Err(Error::UnknownCommand { .. })
        ));
    }

    #[test]
    fn the_command_name_is_read_as_typed() {
        // Element's table is case sensitive, so this is parity rather than an
        // oversight: `/ME` is not a command there either.
        assert!(matches!(
            resolve("/ME waves"),
            Err(Error::UnknownCommand { .. })
        ));
    }

    #[test]
    fn a_doubled_slash_sends_the_slash() {
        assert_eq!(
            sent("//me waves"),
            ("m.text".to_owned(), "/me waves".to_owned())
        );
    }

    #[test]
    fn a_doubled_slash_leaves_an_unknown_command_sendable() {
        assert_eq!(sent("//pin this").1, "/pin this");
    }

    #[test]
    fn plain_sends_what_was_typed_without_reading_it_as_markdown() {
        let content = resolve("/plain *not emphasis*")
            .expect("plain resolves")
            .into_content();
        assert_eq!(content.msgtype.body(), "*not emphasis*");
        assert_eq!(html("/plain *not emphasis*"), None);
    }

    #[test]
    fn me_sends_an_emote() {
        assert_eq!(
            sent("/me waves"),
            ("m.emote".to_owned(), "waves".to_owned())
        );
    }

    #[test]
    fn me_carries_the_lines_under_it() {
        assert_eq!(sent("/me waves\nand grins").1, "waves\nand grins");
    }

    #[test]
    fn me_with_nothing_after_it_sends_nothing() {
        assert!(matches!(resolve("/me"), Err(Error::EmptyMessage)));
        assert!(matches!(resolve("/me   "), Err(Error::EmptyMessage)));
    }

    #[test]
    fn shrug_sends_the_shrug() {
        assert_eq!(
            sent("/shrug"),
            (
                "m.text".to_owned(),
                "\u{AF}\\_(\u{30C4})_/\u{AF}".to_owned()
            )
        );
    }

    #[test]
    fn shrug_is_plain_text_so_the_backslash_survives() {
        // Read as markdown this would lose the backslash and emphasise the
        // middle, which is the whole reason these four are not markdown.
        assert_eq!(html("/shrug"), None);
    }

    #[test]
    fn an_emoticon_puts_whatever_followed_it_after_the_glyph() {
        assert_eq!(
            sent("/shrug who knows").1,
            "\u{AF}\\_(\u{30C4})_/\u{AF} who knows"
        );
    }

    #[test]
    fn tableflip_and_unflip_and_lenny_are_all_there() {
        assert_eq!(
            sent("/tableflip").1,
            "(\u{256F}\u{B0}\u{25A1}\u{B0}\u{FF09}\u{256F}\u{FE35} \u{253B}\u{2501}\u{253B}"
        );
        assert_eq!(
            sent("/unflip").1,
            "\u{252C}\u{2500}\u{2500}\u{252C} \u{30CE}( \u{309C}-\u{309C}\u{30CE})"
        );
        assert_eq!(
            sent("/lenny").1,
            "( \u{361}\u{B0} \u{35C}\u{296} \u{361}\u{B0})"
        );
    }

    #[test]
    fn html_sends_what_was_typed_as_the_formatting() {
        assert_eq!(html("/html <b>bold</b>"), Some("<b>bold</b>".to_owned()));
        assert_eq!(sent("/html <b>bold</b>").1, "<b>bold</b>");
    }

    #[test]
    fn spoiler_hides_the_words_behind_the_marker_every_client_reads() {
        assert_eq!(
            html("/spoiler the butler did it"),
            Some("<span data-mx-spoiler>the butler did it</span>".to_owned())
        );
    }

    #[test]
    fn spoiler_escapes_the_words_because_they_are_words() {
        assert_eq!(
            html("/spoiler a < b & c"),
            Some("<span data-mx-spoiler>a &lt; b &amp; c</span>".to_owned())
        );
    }

    #[test]
    fn html_and_spoiler_with_nothing_after_them_send_nothing() {
        assert!(matches!(resolve("/html"), Err(Error::EmptyMessage)));
        assert!(matches!(resolve("/spoiler"), Err(Error::EmptyMessage)));
        assert!(matches!(resolve("/plain  "), Err(Error::EmptyMessage)));
    }

    #[test]
    fn an_empty_composer_sends_nothing() {
        assert!(matches!(resolve(""), Err(Error::EmptyMessage)));
        assert!(matches!(resolve("\n  \n"), Err(Error::EmptyMessage)));
    }

    /*
      One test per msgtype string, each written out rather than built, so a
      typo in any of the six fails on its own line. The fifth is the one that
      matters: `effects` plural with an underscore, where the rest are
      `effect` singular.
    */
    #[test]
    fn confetti_sends_nic_custom_confetti() {
        assert_eq!(sent("/confetti well done").0, "nic.custom.confetti");
    }

    #[test]
    fn fireworks_sends_nic_custom_fireworks() {
        assert_eq!(sent("/fireworks well done").0, "nic.custom.fireworks");
    }

    #[test]
    fn rainfall_sends_io_element_effect_rainfall() {
        assert_eq!(sent("/rainfall oh no").0, "io.element.effect.rainfall");
    }

    #[test]
    fn snowfall_sends_io_element_effect_snowfall() {
        assert_eq!(sent("/snowfall look out").0, "io.element.effect.snowfall");
    }

    #[test]
    fn spaceinvaders_sends_io_element_effects_space_invaders() {
        assert_eq!(
            sent("/spaceinvaders look up").0,
            "io.element.effects.space_invaders"
        );
    }

    #[test]
    fn hearts_sends_io_element_effect_hearts() {
        assert_eq!(sent("/hearts for you").0, "io.element.effect.hearts");
    }

    #[test]
    fn an_effect_carries_the_words_that_followed_it() {
        assert_eq!(sent("/confetti well done").1, "well done");
    }

    #[test]
    fn an_effect_with_nothing_after_it_sends_an_emote_saying_what_it_is() {
        assert_eq!(
            sent("/confetti"),
            ("m.emote".to_owned(), "sends confetti \u{1F389}".to_owned())
        );
        assert_eq!(sent("/spaceinvaders").1, "sends space invaders \u{1F47E}");
    }

    #[test]
    fn an_effect_plays_here_as_well_as_in_the_room() {
        assert_eq!(
            resolve("/confetti").expect("confetti resolves").effect(),
            Some(Effect::Confetti)
        );
        assert_eq!(
            resolve("/hearts for you")
                .expect("hearts resolves")
                .effect(),
            Some(Effect::Hearts)
        );
    }

    #[test]
    fn nothing_else_plays_anything() {
        assert_eq!(resolve("hello").expect("hello resolves").effect(), None);
        assert_eq!(resolve("/shrug").expect("shrug resolves").effect(), None);
    }

    #[test]
    fn an_incoming_msgtype_names_the_effect_that_sent_it() {
        assert_eq!(effect_of("nic.custom.confetti"), Some(Effect::Confetti));
        assert_eq!(effect_of("nic.custom.fireworks"), Some(Effect::Fireworks));
        assert_eq!(
            effect_of("io.element.effect.rainfall"),
            Some(Effect::Rainfall)
        );
        assert_eq!(
            effect_of("io.element.effect.snowfall"),
            Some(Effect::Snowfall)
        );
        assert_eq!(
            effect_of("io.element.effects.space_invaders"),
            Some(Effect::SpaceInvaders)
        );
        assert_eq!(effect_of("io.element.effect.hearts"), Some(Effect::Hearts));
    }

    #[test]
    fn an_ordinary_msgtype_names_no_effect() {
        assert_eq!(effect_of("m.text"), None);
        // The singular that the plural row would be if somebody tidied it.
        assert_eq!(effect_of("io.element.effect.space_invaders"), None);
    }

    #[test]
    fn an_emoji_in_a_message_plays_the_effect_it_belongs_to() {
        assert_eq!(effect_in("well done \u{1F389}"), Some(Effect::Confetti));
        assert_eq!(effect_in("\u{1F38A}"), Some(Effect::Confetti));
        assert_eq!(effect_in("\u{1F386} tonight"), Some(Effect::Fireworks));
        assert_eq!(effect_in("\u{1F327}\u{FE0F}"), Some(Effect::Rainfall));
        assert_eq!(effect_in("\u{26C8}\u{FE0F}"), Some(Effect::Rainfall));
        assert_eq!(effect_in("\u{1F326}\u{FE0F}"), Some(Effect::Rainfall));
        assert_eq!(effect_in("\u{2744}"), Some(Effect::Snowfall));
        assert_eq!(effect_in("\u{1F328}"), Some(Effect::Snowfall));
        assert_eq!(effect_in("\u{1F47E}"), Some(Effect::SpaceInvaders));
        assert_eq!(effect_in("\u{1F30C}"), Some(Effect::SpaceInvaders));
        assert_eq!(effect_in("\u{1F49D}"), Some(Effect::Hearts));
    }

    #[test]
    fn a_message_with_no_such_emoji_plays_nothing() {
        assert_eq!(effect_in("well done"), None);
        assert_eq!(effect_in("\u{1F600}"), None);
    }

    #[test]
    fn an_effect_survives_a_round_trip_through_json() {
        let written = serde_json::to_string(&Effect::SpaceInvaders).expect("it serialises");
        assert_eq!(written, "\"spaceInvaders\"");
        assert_eq!(
            serde_json::from_str::<Effect>(&written).expect("it comes back"),
            Effect::SpaceInvaders
        );
    }
}
