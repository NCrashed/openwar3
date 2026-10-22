# Playtest notes

Observations from playing the viewer against the original game. Each
entry says what we do now, what the game does (as remembered or still to
be read by the analyst) and its state.

## Icecrown, 2026-10-22

1. Build and training queue. The user remembers a limit of 5. The
   analyst read 7 in the game (8th to 10th train orders refused; the cost
   is paid when an order enters the queue). The look of the empty slots
   is not observed; the interface data has only a queue border texture,
   and from memory the first item shows big with its progress bar, the
   next six as small icons, empty slots as the backdrop. Open.
2. Order confirmations. We draw a blinking circle. The game highlights
   the tree, the mine and attacked targets on their own, in another way.
   The interface data has two models: one for an order on a point, one
   for an order on a unit or other target, and target colors (a green
   target light, a yellow highlight of the target). Not yet watched in
   the game. Open.
3. Power building (several workers building one structure). Not done.
   Open.
4. Placement of a building. We tint the whole building green or red.
   The game data tints it bluish when it can be paid and reddish when
   not. Whether the cells that forbid building are shown on their own is
   not observed. Open.
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
   From the analyst's memory (not observed): after A the cursor becomes
   the targeting cursor, a left click gives the order, a right click
   cancels, a click on the ground is an attack-move, and at its end the
   group keeps its formation places round the point (as for a move)
   instead of closing in.
   Open.
10. No error feedback (message and sound) when gold, lumber or food is
    missing. Open.
11. Footmen trained on the map hit the bear creeps once and then freeze.
    The bears take no damage from the footmen but do from the workers,
    and the bears also freeze after their first hit. Open.
12. Following 11: the creep leader and the tracker round the fountain of
    life do not react to trained footmen at all. Open.
13. The human shop renders its spinning runes wrong: they should be
    billboards facing the camera. Open.
