# Measured fits

A fit is code that reproduces what a replay or vector showed without a
rule behind it: a measured delay or offset, a special case for one
situation, or a record order read from one example. Fits are allowed
while they keep the tests honest, but each one is a question we have not
asked yet. When an analyst answer gives the rule, the fit goes, and
often more than one test that leaned on it starts to match by itself.

Fits are marked in the code:

- `PROVISIONAL:` in a comment: the rule is inferred or read from too few
  cases.
- `hub #N` in a comment: the behaviour was taken from a measured note
  rather than a spec.

To list them:

```sh
grep -rn "PROVISIONAL" crates/ow3-sim/src
grep -rn "hub #" crates/ow3-sim/src
```

State as of November 2026: 356 `PROVISIONAL` comments and about 300
`hub #` references in `ow3-sim`, most of them in the mechanics of the
test maps, which match their replays.

## The melee AI game c-SEA-1

These are the fits on the current front, the melee AI game `c-SEA-1`
(three computer players, 180 s). Each row names where the fit lives and
the question that would replace it with a rule.

### Buildings of the AI

| Fit | Where | Question |
| --- | --- | --- |
| The delay from an accepted building plan to the construction start, per building type: Crypt 250 ticks, Altar of Storms 280, Altar of Elders 298, Ancient of War 304, Moon Well 461, Altar of Darkness 1220, Orc Burrow 787 and 1007 | `movement::ai_build_order` | Is the start a fixed delay from the order or the builder's arrival at its site (the Crypt's 1.25 s fits both)? |
| The building sites, as offsets from the town hall per type | `movement::ai_build_order` | How does the AI choose the site (`SetProduce` and the placement search)? |
| The builder's facing at the start: measured constants for the Altars of Storms and of Elders | `movement::start_buildings` | Does every builder keep the heading of its walk, as the Crypt's and the Altar of Darkness' do? |
| The look-ahead of a path from an AI order passes the buildings' red cells | `follow::decide` (`ai_reassigned`) | Does a building's red part stop the look-ahead of every unit, and why not these? |
| The construction records of the AI's Undead, Orc and Night Elf buildings (`ai_constructing`, `ai_supply_constructing`, `ai_wisp_constructing`, `ai_ancient_constructing`) | `table::construction_rest`, `table::ai_supply_construction_started`, `table::construction_finished` | The records of a construction start and finish per race, as a rule. |

### Workers of the AI

| Fit | Where | Question |
| --- | --- | --- |
| An acolyte is put on its haunted mine's place at its next update, the mine's first worker one update later, a produced one at once | `harvest::tick` (`paid_placements`) | Is it the turn to the mine (11537's heading already pointed at the mine's centre, 11576's was 0.85 off)? The placements at 10.23, 18.15, 36.18 and 43.50 would settle it. |
| The paid acolyte's placement records: fixed delays 0.18, 0.175, 0.085028 and 0.34999 s from the order | `table::ai_paid_placement` | What event the records are tied to. |
| The extra or fewer order records of the AI's harvest calls (`ai_harvest_extra` 7 or 4, `ai_harvest_fewer`) | `table`, `game::register_ai_natives` | The records of `HarvestGold` and `HarvestWood` per worker and target. |
| The cleanups retained across AI passes: `ai_harvest_cleanup`, `retained_harvest_cleanup`, `recycled_guard_cleanup`, `starting_harvest_cleanup`, `fill_ai_build_assignment_cleanup`, `retain_repeated_harvest_arrival` | `table` | Which object owns each of these records, so that its own event frees them. |
| The starting workers' common arrival boundary at their mine (`starting_computer_mine`, `starting_mine_at`) and the placement outside the other starting worker's route (`ai_mine_waiting`) | `harvest` | How the starting workers queue at the mine. |
| The first AI reassignment of a worker still on its game-start harvest stands it a whole update (`ai_restart`) | `movement::ai_reassigned` | Why that first order restarts the worker and later ones do not. |
| The Altar of Elders wisp 11938 waits 5 updates at 0.30 in the game; ours does not | `follow` | Which unit it waits for. |

### The AI's other objects

| Fit | Where | Question |
| --- | --- | --- |
| The records of `RecycleGuardPosition` and the recycled guard's order at 1.0 | `table::guard_position_recycled` | What order 851988 is and which timer gives it. |
| The production plan objects of the town hall (`ai_production`) and the retained records of produced workers (`ai_worker_completion_retained`) | `table` | The records of the AI's production requests, as a rule. |
| The haunted mine's record chain anchored at the first paid placement | `table` (`ai_paid_records`) | The anchors 0.279999763 and 1.089998960 offered by the analyst. |

### Creeps

| Fit | Where | Question |
| --- | --- | --- |
| The watches of the change to day are checked from time 0, and the nwna's War Stomp watches fire at 60.000. The game replaced them at queue time 60.070202. Nothing is scheduled at queue 0.065 to 0.075 at the start, so it is neither a check base nor a start of regeneration there; the creep's 0.1 s timers from the load are reset at once and do not cause it either. The checksum cannot tell the two times apart (both in turn 599). | `table` (the change of the hour, `watch_scheduled`) | Which engine timer fires the 60.070202 take, and the chain of delays that adds up to it (a trace from the take back along that timer's own schedule, in one run). |
