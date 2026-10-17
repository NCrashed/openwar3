---
id: SPEC-0010
title: Leaderboard rows, GetLocalizedString, dialog clicks
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps)"]
---

## Behaviour

### Leaderboard rows

- `LeaderboardAddItem(lb, label, value, p)` always appends a new row at
  the end, also when `p` already has a row. After adding player 0, player
  1 and player 0 again, `LeaderboardGetItemCount` is 3.
- `LeaderboardGetPlayerIndex(lb, p)` gives the index of the first row of
  `p` (0-based), or -1 when `p` has no row. `LeaderboardHasPlayerItem`
  is true while `p` has at least one row.
- `LeaderboardRemovePlayerItem(lb, p)` removes only the first row of `p`;
  the rows after it move up by one.
- `LeaderboardRemoveItem(lb, i)` removes row `i`; with an index past the
  end it does nothing.
- `LeaderboardClear` removes all rows.
- `LeaderboardGetItemCount` counts rows. `LeaderboardSetSizeByItemCount`
  does not change it (with 3 rows, after setting the size to 5 and then
  to 1, the count stayed 3 and all rows kept their indices).
- Adding a row does not sort the leaderboard: a row with a value between
  existing values was still appended at the end.

Test sequence and the values read after each step (`count`, then
indices):

| Step | Count | Indices |
|---|---|---|
| new leaderboard | 0 | |
| add p0 (value 1) | 1 | p0 0 |
| add p1 (value 5) | 2 | p1 1 |
| add p0 again (value 10) | 3 | p0 0, p1 1 |
| sort by value, descending, then ascending | 3 | p0 0, p1 1 |
| size by item count 5, then 1 | 3 | p1 1 |
| add p2 (value 3) | 4 | p2 3 |
| remove player item p0 | 3 | p0 1 (still has a row), p1 0, p2 2 |
| remove item 0 | 2 | p1 -1, p2 1 |
| remove item 7 | 2 | |
| clear | 0 | p2 -1 |

### GetLocalizedString

`GetLocalizedString(key)` looks the key up in the string lists
(`StringList` blocks) of the interface definition files in
`UI\FrameDef`. One key from each of these files was found:

- `UI\FrameDef\GlobalStrings.fdf` (`GAMEOVER_DEFEAT_MSG`)
- `UI\FrameDef\InfoPanelStrings.fdf` (`AGILITY`)
- `UI\FrameDef\DateStrings.fdf` (`MONTH_01`)
- `UI\FrameDef\NetworkStrings.fdf` (`ERROR_ID_INVALIDPARAMS`)

The result is the text of the entry in the installed language.

- The lookup is case-sensitive and does not trim:
  `gameover_defeat_msg`, `Gameover_Defeat_Msg` and keys with a leading
  or trailing space are not found.
- **A key that is not found gives the key itself** (`"OW3_NOPE"` gives
  `"OW3_NOPE"`, `"hfoo"` gives `"hfoo"`).
- `GetLocalizedString("")` and `GetLocalizedString(null)` give `null`.
- Keys of `UI\WorldEditStrings.txt` (`WESTRING_...`) are not found.
- `HIGH_VOL` is defined twice in `GlobalStrings.fdf`, first with `""`
  and later with a text; the result was the later text.
- `TRIGSTR_` references are resolved as described in SPEC-0003.

### Dialog clicks

When the player clicks a dialog button (tested with the button's hotkey):

1. The dialog is hidden for that player **before** any trigger runs: a
   `DialogDisplay(p, d, true)` in the first trigger action left the
   dialog on screen afterwards.
2. Then all `TriggerRegisterDialogEvent` triggers of the dialog run, in
   the order they were registered.
3. Then all `TriggerRegisterDialogButtonEvent` triggers of the clicked
   button run, in the order they were registered.

Each trigger is handled completely (conditions, then actions) before the
next one. With triggers registered as dialog (t0), button (t1), dialog
(t2), button (t3), the order was: c0 a0 c2 a2 c1 a1 c3 a3.

Event responses:

| | Dialog event trigger | Button event trigger |
|---|---|---|
| `GetClickedButton()` | the button | `null` |
| `GetClickedDialog()` | the dialog | `null` |
| `GetTriggerPlayer()` | the clicking player | the clicking player |
| `GetTriggerEventId()` handle id | 92 | 90 (`EVENT_DIALOG_BUTTON_CLICK`) |

The event id of a dialog event trigger is 92, which is neither
`EVENT_DIALOG_BUTTON_CLICK` (90) nor `EVENT_DIALOG_CLICK` (91).

A button trigger fires only for its own button: clicking the second
button ran only the trigger of the second button.

### Dialogs pause single player games

In single player, game time stops while a dialog is shown to the local
player. A 2 s timer started just before `DialogDisplay` did not expire
during 20 s of real time with the dialog open. After the click it
expired normally, with elapsed time 2.0.

## Edge cases

- `LeaderboardGetPlayerIndex` after `LeaderboardRemovePlayerItem` can
  still find the player, when the player had two rows.

## Open questions

1. Whether the duplicate `HIGH_VOL` result means "last definition wins"
   or "empty entries are ignored".
2. Which of the copies of `InfoPanelStrings.fdf` (`UI\FrameDef`,
   `Custom_V0\...`, `Melee_V0\...`) is used.
3. Whether the leaderboard sort functions reorder rows; the test could
   not tell (the indices of the first rows of p0 and p1 are the same in
   both orders).
4. Pause in multiplayer: whether a dialog stops game time in a game with
   several players.

## Sources

- Test maps on 1.27b under Wine, single player, Russian installation,
  run by the analyst workspace, 2026-10-17. Dialog buttons were pressed
  by sending the button hotkey to the game window.

## Verification

Test maps built from a 12-player melee map without victory conditions.

1. Leaderboard sequence of the table above.
2. `GetLocalizedString` of the keys listed above: entry texts for the
   four files, the key itself for the unknown keys, `null` for `""`.
3. Dialog with buttons "Alpha" (hotkey 65) and "Beta" (66); four triggers
   as above; pressing A gives c0 a0 c2 a2 c1 a1 c3 a3 and the responses
   of the table.
4. The same with `DialogDisplay(Player(0), d, true)` in the first action:
   the dialog is visible after the click.
5. Dialog shown with a 2 s timer running: the timer does not expire until
   a button is pressed.
