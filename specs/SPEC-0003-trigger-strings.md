---
id: SPEC-0003
title: Trigger string references (war3map.wts)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace", "Anton Gushcha (black-box test maps only)"]
---

## Behaviour

A map stores user-visible text in `war3map.wts` as numbered entries:

```
STRING <n>
{
<text, one or more lines>
}
```

Scripts and other map files refer to an entry with a string of the form
`TRIGSTR_<n>`.

### When a reference is replaced

- A string is replaced only when the whole string is a reference:
  `TRIGSTR_` followed by the number. A reference that is part of a longer
  string stays as it is. For example `"[TRIGSTR_902]"` is shown with the
  brackets and the reference text, and so is `"[" + "TRIGSTR_902" + "]"`.
- The replacement happens when the text is used, for example when
  `DisplayTextToPlayer` shows it. A script never sees the entry text: string
  operations on `"TRIGSTR_902"` work on those 11 characters.
- The number is read as a decimal integer, so leading zeros do not matter:
  `TRIGSTR_0902` shows entry 902.

### Which text is shown

| Situation | Shown text |
|---|---|
| Entry defined once | its body |
| Number defined more than once | the body of the **first** definition in the file |
| Header `STRING <n>` followed by a line that is not `{` (for example `#VALUE!`) | empty text |
| Number not defined | empty text (the reference itself is not shown) |
| Body of several lines | all lines, with the line breaks kept |

A malformed entry does not affect the entries after it: the next
`STRING` header is read normally.

## Edge cases

- `(1)TheDeathSheep.w3m` from the shipped scenarios has entry 681 written
  as a header followed by the line `#VALUE!`. The map uses it as the
  description of its instruction quest, and entries 682-684, which follow
  it in the file, as the quest items. In the game the description is empty
  and the three items show their texts.
- Quest titles and descriptions follow the same rules as
  `DisplayTextToPlayer`: in test map W1 a quest titled `TRIGSTR_902` shows
  `text 902`, and its description `TRIGSTR_901` (malformed entry) is empty.
  An optional quest titled `TRIGSTR_903` (undefined) is listed with an empty
  title, and its description `TRIGSTR_900` shows `first 900`.

## Open questions

1. Which other places replace references: dialog texts, leaderboards,
   multiboards, floating text, object data fields, `war3map.w3i` strings.
2. Whitespace: are spaces before `STRING`, after the number, or around the
   braces allowed, and are they part of the text?
3. Is the prefix case-sensitive (`trigstr_902`)?
4. Text that contains a line with a single `}`: does it end the entry?
5. Numbers outside the 32-bit range, negative numbers, and `TRIGSTR_`
   without digits.

## Sources

- Test maps on 1.27b, 2026-10-16 (see Verification).
- `(1)TheDeathSheep.w3m` from the user's installation, for the malformed
  entry.

## Verification

Test map W1: a 12-player melee map whose `war3map.wts` gets these extra
entries, in this order (written on one line each here; in the file each
brace and each text line is on its own line):

```
STRING 900  { first 900 }
STRING 900  { second 900 }
STRING 901  followed by the line #VALUE!
STRING 902  { text 902 }
STRING 904  { line one / line two, on two lines }
```

Entry 903 is not defined. One second after start the script, for each of
900, 901, 902, 903, 904 and 0902, calls:

1. `BJDebugMsg("W1 <n>: [" + "TRIGSTR_<n>" + "]")`
2. `BJDebugMsg("W1 <n> literal: [TRIGSTR_<n>]")`
3. `DisplayTextToPlayer(Player(0), 0, 0, "TRIGSTR_<n>")`

Expected output on 1.27b (lines 1 and 2 show the reference text unchanged
for every n):

| n | line 3 |
|---|---|
| 900 | `first 900` |
| 901 | empty line |
| 902 | `text 902` |
| 903 | empty line |
| 904 | `line one`, `line two` |
| 0902 | `text 902` |

The quest log shows a required quest titled `text 902` with an empty
description, and an optional quest with an empty title and the description
`first 900`.

In `(1)TheDeathSheep.w3m` the instruction quest has an empty description
and three items with the texts of entries 682, 683 and 684.
