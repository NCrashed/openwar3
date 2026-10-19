---
id: SPEC-0029
title: Gathering gold and lumber (Human peasant)
status: draft
reference_version: 1.27b
analysts: ["analyst workspace (test maps logged every tick)"]
---

## Behaviour

This spec covers a Human peasant (`hpea`) gathering gold from a gold
mine (`ngol`) and lumber from trees (summer tree wall `LTlt`), and
bringing it to a town hall (`htow`) or a lumber mill (`hlum`). Ticks
and the update cycle are those of SPEC-0018. It is a first version: the
cycle, its timing and the numbers are given; the exact geometry of the
approach is given by measured examples (open questions).

### Data

| Value | Field | Peasant |
|---|---|---|
| gold carried per trip | `DataC` of the harvest ability `Ahar` | 10 |
| lumber per hit and per trip | `DataA`, `DataB` of `Ahar` | 1 and 10 |
| time between two hits on a tree | `Dur` of `Ahar` | 1.1 s |
| reach to a tree | `Rng` of `Ahar` | 116 |
| radius to look for another tree | `Area` of `Ahar` | 900 |
| gold of a new mine | `DataA` of the mine's ability `Agld` | 12500 |
| units inside a mine at a time | (measured) | 1 |
| time inside the mine | (measured) | 1.0 s |

A building can take resources back when it has a return ability: the
town hall `Argl` (gold and lumber), the lumber mill `Arlm` (lumber
only). A tree is a destructable with 50 life; its life is the lumber
left in it.

### Orders

- `harvest` (852018) on a gold mine or a tree: from a script
  `IssueTargetOrder(u, "harvest", target)`; from the interface the
  gather button (`Ahar`, at (3,1)) or a right click on a mine or tree.
- While the peasant carries resources back, its current order is
  `resumeharvesting` (852017); when the resources are delivered it
  becomes `harvest` (852018) again and the peasant goes back to the same
  mine or tree.
- `returnresources` (852020) is the order of the interface's return
  button (not studied).

### Reaching a building or a tree

- **A building** (gold mine, town hall, lumber mill) is reached at the
  first update at which the peasant's footprint (SPEC-0020: class 1,
  the cells `x-1 .. x`, `y-1 .. y` around the cell `(x, y)` of its
  position) touches a cell of the building's pathing texture that
  blocks walking (red, SPEC-0022), touching meaning that the two cells
  are equal or neighbours, also diagonal ones (Chebyshev distance of
  the cells at most 1). The red cells are the inner 8 x 8 of the gold
  mine's `16x16Goldmine` (the outer ring blocks building only), the
  inner 12 x 12 of the town hall's `16x16Simple` and the whole 10 x 10
  of the lumber mill's `10x10SimpleSolid`, placed by SPEC-0022
  (snapping and placement). Because the footprint lies to the lower
  left of the position, the distance from the building's centre at
  which this happens depends on the side: for a mine at (cx, cy) the
  peasant entered at 190.9 from the east and 190.4 from the north, at
  156.8 from the west and 156.4 from the south, at 263.3 from the
  north-east and about 218 from the other diagonals; it delivered at a
  town hall at 218.8 / 219.3 coming from the west / south and 252.9 /
  253.3 from the east / north.
- **A tree** is reached at the first update at which the distance
  between the peasant's position and the tree's is at most
  `Rng + 16` = 132 (the reach of `Ahar` plus the peasant's collision
  size 16): in 16 approaches from 8 directions the distance at the
  first hit was between 128.0 and 132.0.

### Gold

1. The peasant walks to the mine (SPEC-0018, SPEC-0020). When it
   reaches it and the mine is free, it enters: it is hidden
   (`IsUnitHidden` true) and stays where it stood. In test RUF (mine at
   (-4096, -5120), the peasant coming from the north) it entered at
   (-4080.40, -4932.93), 187.7 from the mine's centre.
2. It stays inside for 1.0 s (200 ticks; 201 at the first entry of the
   test), then appears again at the
   same position, carrying 10 gold, with order 852017; at that moment
   the mine's gold (`GetResourceAmount`) drops by 10.
3. It walks to the nearest building that takes gold and delivers when
   it reaches it: the player's gold grows by 10 and its order becomes
   852018 (in RUF at (-4080.02, -4317.42), 221.4 from the centre of the
   town hall at (-4096, -4096)). Then it walks back to the mine.

The cycles of one peasant on that layout repeated with the same
numbers: entering the mine in ticks 568, 2135, 3485, 4835, 6185, 7536
(after the first, every 1350 or 1351 ticks), leaving 200 ticks after
each entry, delivering 682 or 574 ticks after leaving.

**Several peasants on one mine.** Only one peasant is inside at a
time. The others wait where they reached the mine (order 852018, not
hidden); when the one inside leaves, the next one enters in the same
tick, in the order in which they reached the mine (test RUJ: three
peasants from the west, north and east entered in the order of their
arrival, ticks 430, 630, 831). With five peasants ordered in the same
tick (test RUF) the mine lost 10 gold every 200 ticks. A peasant leaves
the mine at the point where it entered; when another peasant stood
there, it was put at the centre of a free cell nearby instead
(entered at (-6162.17, -5957.45), left at (-6192, -5936)).

**An empty mine.** When the last gold is taken (the mine had 30 in
test RUJ: three trips), the mine's gold is 0 as the peasant leaves it
and the mine is gone; the peasant delivers its 10 gold and then stands
with order 0.

### Where the resources go

The peasant takes them to the building that takes that resource and
is nearest **along the path**: with one town hall 896 from the mine
behind a wall and another 1344 from it in the open, the gold went to
the one in the open; without the wall, to the nearer one (test RUJ).

### New orders while carrying

A peasant that carries gold and is ordered elsewhere (`move`) keeps the
gold. Given `harvest` on the mine again, it walks to the mine and, when
it reaches it, does not enter: its order becomes 852017 and it first
takes the carried gold back (test RUJ).

### Lumber

1. The peasant walks to the tree and stops within reach of it (in RUF
   at 127.8 from the centre of a tree at (-2048, 4096)).
2. It hits the tree every 1.1 s (220 ticks); each hit takes 1 life from
   the tree and gives the peasant 1 lumber. The first hit came 99 ticks
   after it stopped.
3. After the 10th hit (114 ticks later in the test) its order becomes
   852017 and it carries the 10 lumber to the nearest building that
   takes lumber: with the town hall at (-4096, 4096) and a lumber mill
   at (-2560, 4096) it went to the lumber mill and delivered at
   (-2369.04, 4015.97), 191 from the mill's centre; the player's lumber
   grew by 10.
4. It goes back to the same tree and goes on (next hits 338 and 350
   ticks after the delivery for the two peasants of the test).

When a tree has no life left it dies; a peasant that is not full yet
goes on at the nearest other tree (test RUJ: a tree with 3 life, then
a tree 320 away, until it carried 10) and then returns.

## Edge cases

- Two peasants on two neighbouring trees hit in their own cycles of
  220 ticks (in RUF 36 ticks apart).
- The mine's gold drops when the peasant leaves the mine, not when it
  enters.

## Open questions

1. Whether the rule for trees adds the peasant's collision size or a
   fixed 16, and the search for the free cell when a peasant leaves a
   mine where another one stands.
2. Which peasant enters a busy mine next.
3. How "nearest along the path" is measured (the high-level search of
   SPEC-0020 or another), and the next tree when several are at the
   same distance; a peasant with no mine or tree left nearby.
4. The interface's return button (852020), the automatic gathering
   orders 852021 and 852022, and peasants given a new order while
   carrying.
5. Other races (orc peon and its burrows, undead acolytes and haunted
   mines, night elf wisps and entangled mines).

## Sources

- Test maps RUF and RUG on 1.27b under Wine, 2026-10-20: three
  separate places on a flat map (one peasant on a mine, five peasants
  on another mine, two peasants on trees with a town hall far and a
  lumber mill near), the orders, hidden state, the player's gold and
  lumber, the mines' gold and the trees' life logged at every change,
  and the positions of one gold and one lumber peasant at every tick.
- Test map RUH: names of the order ids by `OrderId2String`.
- Test map RUJ: a busy mine with three peasants, a mine with 30 gold,
  a tree with 3 life and two more trees, two town halls with and
  without a wall in between, a peasant ordered to move while carrying.
- Test map RUI: eight mines with a town hall in eight directions and
  eight trees with a lumber mill, a peasant on each; positions at
  every change of order or hidden state.
- `Units\AbilityData.slk`, `Units\UnitAbilities.slk`,
  `Units\DestructableData.slk`.

## Verification

1. **Gold** (RUF, one peasant, order at tick 100): hidden from tick 568
   to 768 at (-4080.40, -4932.93); visible with order 852017 in tick
   769, when the mine drops from 12500 to 12490; delivery (order back to
   852018) in tick 1451.
2. **Lumber** (RUF): the first tree's life goes 50, 49 (tick 379), 48
   (599), ... 40 (2359); order 852017 in tick 2473; lumber 10 -> 20
   when this peasant delivers in tick 2735; the tree's next hit in tick
   3085.
3. **Approach** (RUI): at every one of the 181 entries into a mine and
   deliveries logged, the footprint of the peasant touches a red cell
   of the building (cell distance 1); at the 16 first hits on trees the
   distance is 128.0 to 132.0.
4. **Busy mine** (RUF, five peasants): the mine's gold drops by 10 at
   ticks 769, 969, 1169, 1369, 1569, ...; each entry is in the tick of
   the previous peasant's exit.
