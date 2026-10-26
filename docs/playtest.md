# Playtest notes

Observations from playing the viewer against the original game. Each
entry says what we do now, what the game does (as remembered or still to
be read by the analyst) and its state.

## Icecrown, 2026-10-22

1. Build and training queue. The user remembers a limit of 5; the game
   takes 7 (the 8th to 10th train orders are refused; the cost is paid
   when an order enters the queue). Observed in the game: a title with a
   progress bar and the current unit's icon large at the left, below it
   a row of 6 small slots for the rest of the queue; filled slots show
   the unit icons, empty slots dim grey boxes with their slot number
   (3 to 7 with two units queued). Open.
2. Order confirmations. We draw one blinking circle for all. Observed
   in the game (each shows for about 0.2 to 0.4 s):
   - ground: three arrows converging on the point, green for a move,
     red for an attack-move;
   - enemy unit: a red ring at its feet;
   - tree: the tree model lights up (green-yellow tint) and a yellow
     ring at its base;
   - gold mine: a large yellow ring round the mine.
   Open.
3. Power building (several workers building one structure): done, see
   the Echo Isles notes below.
4. Placement of a building. We tint the whole building green or red.
   Observed in the game: the ghost building keeps its normal colours,
   and under it the footprint is drawn per cell, green squares where it
   may be built, red where not (all red in the trees, partly red over
   the mine). Open.
5. The resources a unit costs are not shown (tooltip of the train
   button). Open.
6. Not enough food. The game does not queue a unit then; a unit already
   in the queue waits with its training paused, as ours does. Only the
   first part is missing. Open.
7. Workers cutting trees in a group often lose their tree when it falls
   and stand in the middle of the forest instead of taking another one.
   Open.
8. Human buildings under construction have no construction animations.
   Open.
9. Attack command:
   - the attack button shows no pressed animation;
   - the cursor does not change when the attack command starts;
   - attack on the ground: the units walk to the point as a group, then
     all bunch round the order point at once. Check against the game;
   - check which mouse button issues the attack click after A.
   Observed in the game: after A the cursor becomes a cyan crosshair and
   the command card shows only a cancel button; a left click gives the
   order, a right click cancels with no order; the card marks the
   current order's button with a green frame. At the end of a group's
   attack-move on empty ground the units stop in their formation places
   round the point (a diamond of four footmen about 70 apart), so our
   bunching on the point is wrong.
   Done: an attack-move ends where its walk arrives, at the member's
   place in the formation; while an order waits for its target the card
   shows only cancel and the cursor is a crosshair (the system one for
   now, not the game's cyan art); a left click gives the order, a right
   click or cancel drops it; the current order's button has the green
   frame.
10. No error feedback (message and sound) when gold, lumber or food is
    missing. Open.
11. Footmen trained on the map hit the bear creeps once and then freeze.
    The bears take no damage from the footmen but do from the workers,
    and the bears also freeze after their first hit. Open.
12. Following 11: the creep leader and the tracker round the fountain of
    life do not react to trained footmen at all. Open.
13. The human shop renders its spinning runes wrong: they should be
    billboards facing the camera. Done: billboarded nodes now face the
    camera (checked on the wisps of the fountain of health).

## Echo Isles, 2026-11-16

1. Power building (from Icecrown 3): done. Helpers speed the building
   by Repair's DataD each while its builder works and pay for it; a
   helper takes over when the builder leaves (c-SBZ-1, hub #13, every
   turn checksum and object matched). Construction animations (from
   Icecrown 8): done, a building under construction plays its Birth
   sequence by the progress of the build.
2. The minimap of a map that is not square (Echo Isles) does not match
   its points: the units and buildings drawn on it do not sit where the
   terrain image shows them. The map's picture keeps the playable
   area's proportions, its longer side across and black bands on the
   other; we stretched the area over the whole picture. Done: the points
   take the picture's square.
3. The finished Human farm draws only its ring of stone walls: the barn
   inside, which its Birth sequence shows, is missing from our Stand.
   Seen on c-RTI's farms. Open.
