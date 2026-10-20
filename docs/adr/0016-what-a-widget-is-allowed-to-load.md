# 16. Decide what a widget may load before deciding how to draw one

Date: 2026-10-10

## Status

Accepted for the URL rules, which are built and tested. The question of
whether Consort embeds a remote origin at all is a decision nobody has taken
yet: see [What is not decided here](#what-is-not-decided-here).

## Context

A widget is a web page a room asks a client to embed. Issue #139 asks for a
home for one: a Wordle game on Consort's own website, in its own room, drawn
across the top of it the way Element draws one.

The room says so in state. Element writes `im.vector.modular.widgets`, keyed by
the widget's ID, and reads only that: `WidgetStore.ts` still carries
`// TODO: Support m.widget too`. MSC2764 names the same event `m.widget` and
defines its content as `WidgetCommonProperties`, where the field that matters
is `url`. Where the widget goes is a second event, `io.element.widgets.layout`,
which has no spec at all: `docs/widget-layouts.md` in element-hq/element-web is
its only description, and its `top` container is the one #139 asks for.

So the value Consort would load is a URL, written into room state by any member
with the power level to send that event, and it is the first value in this
application with that shape. Everything else a room member can put in front of
somebody here is text, a picture, or an `mxc://` URI that only this client's own
media code resolves. A widget URL is different in kind: it names an origin, and
the thing that would act on it is a browser engine holding the session.

MSC2764 makes it more pointed than a bare URL. The `url` is a *template*.
Variable names are the keys of the widget's own `data` object, the client must
substitute them, and four defaults (`$matrix_user_id`, `$matrix_room_id`,
`$matrix_display_name`, `$matrix_avatar_url`) must take priority over anything
`data` names. Both halves of the template are therefore chosen by whoever wrote
the event: the string with the holes in it, and most of what goes in the holes.

## Decision

The read path refuses a URL before anything is asked to draw it, and the list
of what it accepts is a whitelist. In `rooms::widgets::definition`:

- **`https:` only.** `javascript:` would run in the webview that holds the
  session rather than inside a frame. `data:` is the same with its own markup.
  `file:` would read the disk. `http:` would put a plaintext page inside an
  end-to-end encrypted client, where anybody on the path rewrites what it
  loads. A scheme the platform gains later is refused by default rather than
  accepted by default.
- **No credentials in the URL.** `https://example.org:pass@evil.example/` is
  the oldest way of making a URL look like it points somewhere else, and
  Consort draws no URL bar in which anybody could check.
- **The origin is compared before and after templating.** This is the rule that
  is not obvious. `https://$host.example.org/` is a legal template, and `host`
  is somebody else's to choose, so the URL that passed the checks and the URL
  that would be loaded are not the same string. Both are parsed and their
  origins must match.
- **Values are percent-encoded to RFC 3986's unreserved set.** A value carrying
  `/`, `?`, `#`, `:` or `@` must not be able to move the widget to another path,
  let alone another origin. The spec asks for escaping and its own worked
  example (`test:value` becoming `test%3Avalue`) wants this much.
- **Substitution is single-pass.** A substituted value lands in the output and
  is never read again, which is how the spec's rule that nested variables are
  not supported is met: a `data` value of `$answer` stays the literal
  `$answer`.
- **`$matrix_avatar_url` is answered with the empty string.** It wants an HTTP
  URL for the viewer's avatar. On a homeserver that has moved to authenticated
  media there is no such URL that does not carry this session's access token,
  and handing that to a remote origin is handing it the account. The spec's own
  value for an absent avatar is the empty string, so that is what a widget
  gets, whether or not the viewer has one.

The widget's ID is the state key, never `content.id`. The two are a second,
unenforced copy of each other, nothing makes them agree, and the layout
addresses widgets by state key: believing the content would let one widget
answer to another one's layout entry.

## What is not decided here

Whether Consort embeds a remote origin at all.

`app/src-tauri/tauri.conf.json` sets no `frame-src`, so `default-src 'self'`
governs frames and a remote origin in one is refused today. Widening that is a
trust decision about remote origins inside the window that holds the session,
and it is not a decision a read path gets to make on the way past. The options
and what each costs are in the issue queue against #139; nothing in this
repository should widen that CSP until that question has an answer.

## Consequences

The rules are built and tested and nothing calls them. That is the point: the
half of #139 that needed no decision is done, and the half that does is a
smaller question than it was, because what a widget may load is now settled
whatever the answer turns out to be.

Three things a reader should expect to be surprised by:

The rules refuse more than Element does. Element's filter is `content.type &&
content.url` and nothing else: it will put `http://` in a frame. Consort will
not, and a room whose widget is plaintext shows no widget rather than a
plaintext one.

MSC2764 requires `id` and `creatorUserId` and neither is checked. Real rooms do
not carry them reliably and Element does not look, so checking them would
refuse widgets that work everywhere else.

A widget nobody placed in the layout is not at the top. That is Element's
behaviour, and it is the reason adding a widget to a room does not put a web
page over somebody's timeline without them asking for it.
