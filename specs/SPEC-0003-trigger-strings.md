---
id: SPEC-0003
title: Trigger string references (war3map.wts)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (static analysis of the binary, test maps)", "Anton Gushcha (black-box test maps only)"]
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

### Reading war3map.wts

- An entry starts with a line whose first word is `STRING` followed by the
  number. Spaces before `STRING`, several spaces before the number and
  spaces after it are allowed.
- The text starts after the `{` that opens the body. Anything after the
  `{` on the same line is part of the text: `{ ` followed by `text`
  gives a first line consisting of one space, then `text`.
- Text lines are kept as written, including leading spaces.
- The text ends at the first line that starts with `}`. The rest of that
  line is ignored (`}x` also ends the text).
- Lines outside entries are ignored: comment lines such as `// comment`,
  stray text lines and stray `}` lines after an entry.
- Entries need not be separated by blank lines.
- A UTF-8 byte order mark at the start of the file is ignored.
- Entries with a negative number (`STRING -5`) or a number outside the
  32-bit range (`STRING 4294968296`) cannot be reached by any reference
  tried (`TRIGSTR_-5`, `TRIGSTR_1000`, `TRIGSTR_4294968296`).

### What counts as a reference

A string is a reference when it starts with exactly `TRIGSTR_`
(case-sensitive, nothing before it). The number is read from the
characters after the prefix as decimal digits up to the first other
character:

| String | Entry |
|---|---|
| `TRIGSTR_902`, `TRIGSTR_0902` | 902 |
| `TRIGSTR_910abc`, `TRIGSTR_910 ` (trailing space) | 910 |
| `TRIGSTR_` (no digits), `TRIGSTR_+910`, `TRIGSTR_0x38e` | 0 |
| `TRIGSTR_-5` | none (empty text) |
| `trigstr_910`, `Trigstr_910`, ` TRIGSTR_910` (leading space) | not a reference |

### Where references are replaced

A reference is replaced when it is passed as a string argument to one of
these natives; the native then works with the entry text:

`CopySaveGame`, `DefeatConditionSetDescription`, `DialogAddButton`,
`DialogAddQuitButton`, `DialogSetMessage`, `DisplayTextToPlayer`,
`DisplayTimedTextFromPlayer`, `DisplayTimedTextToPlayer`,
`GetLocalizedString`, `LeaderboardAddItem`, `LeaderboardSetItemLabel`,
`LeaderboardSetLabel`, `MultiboardSetItemIcon`, `MultiboardSetItemValue`,
`MultiboardSetItemsIcon`, `MultiboardSetItemsValue`,
`MultiboardSetTitleText`, `QuestItemSetDescription`, `QuestSetDescription`,
`QuestSetTitle`, `RemoveSaveDirectory`, `RenameSaveDirectory`,
`SaveGameExists`, `SetAltMinimapIcon`, `SetCinematicScene`,
`SetIntroShotText`, `SetMapDescription`, `SetMapName`, `SetPlayerName`,
`SetTextTagText`, `StringCase`, `StringLength`, `TimerDialogSetTitle`,
`TriggerRegisterPlayerChatEvent`.

Everything else sees the reference itself:

- script operations: `"TRIGSTR_910" == "text 910"` is false,
  `"TRIGSTR_" + "910" == "TRIGSTR_910"` is true;
- all other natives, for example `SubString("TRIGSTR_910", 0, 4)` is
  `"TRIG"`, `S2I` of a reference to an entry `42` is 0, and `Preload` writes
  the reference text.

Only the whole argument is examined. A string that merely contains a
reference (`"[TRIGSTR_902]"`) is never replaced. It does not matter how
the string was built: a literal, a variable and a concatenation that
produces exactly `TRIGSTR_910` are all replaced (`StringLength` gives 8
for `text 910` in each case). Values stored by these natives keep the
entry text: `LeaderboardGetLabelText`, `MultiboardGetTitleText` return
`text 910` after `TRIGSTR_910` was set.

The game library also uses the same lookup when it reads `war3map.w3i`
strings (map name, description, player and force names); this was not
tested separately.

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

1. How the header number is read: why `STRING -5` and `STRING 4294968296`
   cannot be referenced (rejected entries or a different number range).

## Sources

- Test maps on 1.27b, 2026-10-16 (see Verification).
- Analysis of the 1.27b game library for the list of natives that resolve
  references and for the prefix comparison.
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

Test maps W2a, W2b and W2c (run by the analyst workspace) add the entries
910-922 with the variants listed under "Reading war3map.wts" and check
each reference form with `DisplayTextToPlayer` (read from the message
log) and with `StringLength`, `StringCase`, `SubString`, `S2I`,
`LeaderboardGetLabelText`, `MultiboardGetTitleText` and `Preload`. The
expected results are the tables above.
