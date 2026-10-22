# Playtest notes

Observations from playing the viewer against the original game. Each
entry says what we do now, what the game does (as remembered or still to
be read by the analyst) and its state.

## Icecrown, 2026-10-22

1. Build and training queue. The game limits a queue to 5 units and
   shows the empty slots of the queue. Ask the analyst for the limit and
   the empty slots. Open.
2. Order confirmations. We draw a blinking circle. The game highlights
   the tree, the mine and attacked targets on their own, in another way.
   Ask the analyst how each confirmation looks. Open.
3. Power building (several workers building one structure). Not done.
   Open.
4. Placement of a building. We tint the whole building green or red.
   Ask the analyst whether the game also shows the cells under the
   building that forbid building. Open.
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
