# Contributing

Read [docs/clean-room.md](docs/clean-room.md) first: it decides which sources
you may use for the part of the engine you work on.

## Commits, pull requests and issues

- Write commit subjects in the imperative mood, under 72 characters, with an
  optional `crate:` prefix (`ow3-formats: read MPQ hash table`).
- Explain in the body why the change is needed when the diff does not make it
  obvious.
- Add a `Co-authored-by` trailer only for people who co-wrote the change.

## Writing style

These rules apply to documentation, specs, comments, commit messages, pull
requests and issues. They keep the text easy to edit by hand in any editor and
keyboard layout.

- Use plain ASCII punctuation: `-` or `--` for dashes, `->` for arrows,
  `"` and `'` for quotes, `...` for ellipsis. Non-ASCII is fine for names and
  for non-English text where it is part of the content.
- Prefer commas, colons and separate sentences over long dashes.
- State what something is or does directly. Avoid framing sentences as
  "this, not that" unless the contrast is the actual point.
- Keep sentences short and concrete; prefer an example to an adjective.
- Wrap Markdown prose at about 80 columns.
