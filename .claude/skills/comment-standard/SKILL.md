---
name: comment-standard
description: The repository's hard limits on comment length, what a comment is for, and where long explanation goes instead. Read before writing a comment, a file header, a config block, or a new markdown file.
metadata:
  origin: consort
---

# Comment Standard

A comment earns its place by saying something the code cannot. Length is not
thoroughness, it is a tax charged to every future reader.

This is a rule, not advice. It governs code, config, workflows and anything
else written as part of a task. It exists because 26% of this repository was
comment when it was first measured, and most of that was prose nobody had time
to read, sitting where it blocked the code it described. Issue #127 has the
measurement.

## When to Use

- Before writing any comment, doc comment, file header, or config key comment.
- Before adding a block of prose to a workflow, a `Cargo.toml`, or a shell
  script.
- When reviewing a diff: a comment that restates its line gets deleted.
- When an explanation has outgrown the limits and needs a home in `docs/`.

## The limits

| Where | Limit |
|---|---|
| Inline, end of line | 1 line |
| Above a function, type, or config key | 3 lines |
| File header | 5 lines, and most files need none |

Two rules sit over the table.

**Never a comment block longer than the code it describes.** A 20 line essay
above a 6 line build step means the reader scrolls past the explanation every
time they want the step.

**Hard ceiling: no comment block over 15 lines, anywhere, for any reason.**
Fifteen lines is not much prose. The 80 column limit expands a wrapped
paragraph vertically, so a block that reads as three sentences on one screen is
already most of the way to the ceiling. Nothing in this repository has earned
an exception.

## What a comment is for

Explaining **why**, where the why is not already obvious:

- a constraint invisible in the code: a version floor, a platform quirk, a
  required ordering
- a deliberate choice that looks like a mistake
- a pointer to the issue, ADR, or doc that holds the rest

```rust
// Sorted before dedupe: dedupe keeps the first of each run.
// X11 only, and the portal is not a fallback: docs/adr/0006-share-a-screen-over-x11.md
// This pin must match the one matrix-rust-rtc uses: docs/DEPENDENCIES.md
```

## What a comment is not for

- Restating the code (`// loop over the users`, `// increment the counter`)
- Narrative reasoning, or the alternatives considered and rejected
- History, changelog entries, or what the code used to do
- Tutorials, background, or a rationale essay

All of that has a home already. Reasoning and rejected alternatives go in the
**commit message** or the **pull request body**, attached to the change rather
than to the file forever. History is in `git log`.

## Long explanation goes in docs/

When something genuinely needs more room than the limits allow, it is
documentation, not a comment. Put it in `docs/`, or in `docs/adr/` if it is a
decision, and link to it from the code in one line.

The argument for this is rot, not tidiness. A rationale paragraph next to code
is never updated when that code changes, so it drifts into being confidently
wrong, which is worse than absent. A doc has a date, a diff of its own, and a
reader who went looking for it.

This applies to anything written as part of a task, not only to source. Do not
answer a task by dumping prose into a config file, a workflow, or a new
top-level markdown file. One line where the reader is, the detail in `docs/`.

## Exempt

Licence and SPDX headers, and the banner a code generator wrote. Nothing else.

A doctest inside a doc comment is code, not prose, and is measured as code.

## Checklist

- [ ] No block longer than the code it describes, and none over 15 lines
- [ ] No comment that restates its line
- [ ] No reasoning, rejected alternatives, or history in a source file
- [ ] Anything longer lives in `docs/` and is linked in one line
- [ ] Every comment left behind answers **why**, not **what**
