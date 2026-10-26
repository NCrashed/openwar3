# Roadmap

The short version is in the [README](../README.md#roadmap). This page lists
every area of the engine, how far it is, and, just as important, where we do
not yet know how much work is left.

State as of November 2026.

Legend:

- **done**: implemented and checked against the game (replays, logs or
  measurements);
- **partial**: some of it works and is checked, the rest is missing;
- **not started**: nothing yet;
- **scope known / unknown**: whether we can list what is left. "Unknown"
  means we have not yet looked closely enough to count the work.

## 1. Foundations

| Area | State | Scope | Notes |
| --- | --- | --- | --- |
| MPQ archives | done | known | All non-sound files of the 1.27b archives. Missing: Huffman/ADPCM (sounds), bzip2, LZMA, sparse. |
| Images (BLP, TGA) | done | known | |
| Tables (SLK), profiles, FDF | done | known | |
| Models (MDX 800) | done | known | All game models parse. |
| Map files (w3i, w3e, doo, w3u..w3q, w3r, w3c, w3s, wts) | done | known | Some fields have open meaning questions, see [specs/QUESTIONS.md](../specs/QUESTIONS.md). |
| Replays (w3g) | done | known | Read, played back and compared turn by turn. |
| JASS language and VM | done | known | Bytecode VM with the game's operation costs and thread limits (SPEC-0005). |
| Deterministic arithmetic, math natives, random numbers | done | known | Bit-exact with the game (SPEC-0004, 0007, 0011). |
| Timers, game time, script sleeps | done | known | SPEC-0002. |
| Map loading, player slots | done | known | All 191 maps of the install load and run their scripts. |

## 2. Simulation

The simulation is checked against replays of small test maps made for one
mechanic each. A mechanic counts as done when its replays match the game
turn by turn, including the turn checksum (SPEC-0024).

| Area | State | Scope | Notes |
| --- | --- | --- | --- |
| Unit state: life, mana, regeneration, facing | done | known | SPEC-0013. |
| Movement and path finding around static obstacles | done | known | SPEC-0018, 0020. |
| Units as obstacles, crowds, group orders, formations | done | partial | Matched on the test maps; large armies and narrow passages are less covered. |
| Basic orders, order queue (Shift) | done | known | SPEC-0019, 0031. |
| Attacks, damage, armor, missiles | done | known | SPEC-0015, 0016. |
| Target choice, calls for help, creep camps | done | known | SPEC-0019, 0030, 0048. |
| Resource gathering (Human peasant) | partial | unknown | Gold and lumber work; gold and lumber harvesting match the game down to their internal objects (c-RXI-1, c-RYP-1). Other races' workers (acolytes, wisps, peons) not started. |
| Building and training (Human) | partial | unknown | Placement, construction start, training queue. Other races, repair, cancel, upgrades and research not started. |
| Fog of war | partial | known | What a unit reveals (SPEC-0032). Invisibility and detection not started. |
| Regions and their events | done | known | SPEC-0049. |
| Heroes: experience, levels, attributes, skills | done | known | SPEC-0033 to 0035. |
| Hero abilities | partial | known | Done: Storm Bolt, Thunder Clap, Bash, Critical Strike, Wind Walk, Mirror Image, Blizzard, Water Elemental, Divine Shield, three auras, Brilliance Aura, creep Shockwave, Chain Lightning, War Stomp, Healing Wave, Flame Strike. Built, being matched: Death Coil, Frost Nova, Impale, Carrion Swarm, Mana Burn, Fan of Knives, Entangling Roots. The rest of the 24 melee heroes not specified. |
| Unit abilities (non-hero spells, autocast, morphs, transports) | not started | unknown | AbilityData.slk has about 800 entries; many share a base code, but nobody has counted the distinct mechanics yet. |
| Buffs, dispels, summons in general | partial | unknown | Only what the done abilities need. |
| Items: drops, use, shops, inventories | partial | unknown | Random item choice only (SPEC-0042). |
| Flying, naval and amphibious movement | not started | unknown | |
| Upkeep, food, victory and defeat (melee rules) | partial | unknown | Food is counted; upkeep and the melee end-of-game rules are not. |
| Computer opponents (AI scripts) | not started | unknown | The AI natives exist in the native table (SPEC-0001); none behave yet. |

### Matching the game exactly

Matching unit positions is not enough for a networked game: every turn's
checksum must match too. That checksum covers the engine's table of internal
objects, so every mechanic has to create and free its internal objects in
the game's order.

- All 40 test replays match every turn checksum.
- The hero test c-SAK-1 (Mountain King, Blademaster, creeps): its object
  table matches the game entry by entry (11848 changes to 59 s), and so
  does every one of its 1614 turn checksums (to 161 s).
- The hero test c-RZH-1 (Mountain King, footmen, creeps, with Storm Bolt,
  Thunder Clap and creep Bash): its object table (4881 changes) and all
  629 turn checksums match the game.
- The hero test c-RZJ-1 (Archmage, Paladin, water elementals, footmen, an
  Ogre Lord's camp, with Blizzard, Holy Light, Divine Shield, auras and
  creep Shockwave): its object table (7596 changes) and all 828 turn
  checksums match the game.
- The Thunder Clap probe c-SBG-1 (slows ending by time, hold position):
  its object table and every turn checksum match the game.
- The battle reference c-RXR-1 (footmen, riflemen, grunts, headhunters):
  its object table (6892 changes) and all 408 turn checksums match the
  game.
- The gold harvest test c-RXI-1 (six peasants, a gold mine, a town hall):
  its object table (3263 changes) and all 610 turn checksums match the
  game.
- The player orders test c-RTW-3 (footmen: moves, patrol, follow, group
  and Shift group orders, stop): its object table (1175 changes) and all
  900 turn checksums match the game.
- The lumber harvest test c-RYP-1 (peasants, trees falling, another tree
  looked for): its object table (2356 changes) and all 697 turn checksums
  match the game.
- The gold harvest test with kobolds attacking c-RXD-1: its object table
  (2289 changes) and all 385 turn checksums match the game.
- The Orc hero spells test c-SBI-1 (Chain Lightning, War Stomp and
  Healing Wave on a creep camp): its object table (1431 changes) and all
  161 turn checksums match the game.
- The Flame Strike test c-SBH-1 (the Blood Mage on four footmen: the
  burning area's two phases and its timer of random delays): its object
  table (640 changes) and all 160 turn checksums match the game.
- The Night Elf spells test c-SBK-1 (Mana Burn, Fan of Knives and Entangling
  Roots on a creep camp): its object table (1276 changes) and all 161
  turn checksums match the game.
- The levelling tests c-SBL-1 (Mountain King), c-SBM-1 (Far Seer),
  c-SBN-1 (Death Knight) and c-SBO-1 (Warden), a hero and a footman
  killing paused creeps made vulnerable one by one, with level-ups inside
  the kills: their object tables (1258, 1334, 1264 and 1267 changes) and
  all 161 turn checksums of each match the game.
- The aura test c-SBP-1 (kobold taskmasters and paladins killed while
  their auras run): its object table (1263 changes) and all turn
  checksums match the game.
- The Undead spells test c-SBJ-1 (Death Coil, Frost Nova, Impale and
  Carrion Swarm on a creep camp, with heroes that cannot attack running
  from their attackers): its object table (1830 changes) and all 161
  turn checksums match the game.
- The altar and items test c-SBQ-1 (a Paladin killed and revived at an
  Altar of Kings, an Archmage picking up Claws of Attack and a healing
  potion and drinking it): its object table (1191 changes) and all turn
  checksums match the game.
- The test c-SBR-1 (a level-2 Paladin revived, a Healing Salve of three
  charges used, dropped and picked up again): its object table (798
  changes) and all turn checksums match the game.
- The test c-SBS-1 (a level-3 Paladin revived, six Archmages picking up
  items 95 to 145 away): its object table (1132 changes) and all turn
  checksums match the game.

**Scope unknown.** Each mechanic brings its own internal objects. We learn
them one replay at a time, and only after a mechanic already moves right.
So every row of the table above has a second, hidden part of the work, and
we cannot size it before we get there.

## 3. Presentation (the viewer)

| Area | State | Scope | Notes |
| --- | --- | --- | --- |
| Terrain, cliffs, water, day and night | done | known | SPEC-0012, 0014, 0017, 0023. |
| Models, animation, particles, shadows | partial | known | Animations and skinning work; some effects are simplified. |
| Selection, orders, command card, minimap | partial | known | Playable with mouse and hotkeys. Open differences from the game: [docs/playtest.md](playtest.md). |
| Game interface (menus, lobby, score screen) | not started | unknown | |
| Sound and music | not started | unknown | Needs the missing MPQ sound decompressors first. |

## 4. Multiplayer

| Area | State | Scope | Notes |
| --- | --- | --- | --- |
| Lockstep turns and checksums | partial | known | Turns and their checksums are read and compared from replays. |
| Playing with the original 1.27b client (LAN) | not started | unknown | Needs the network protocol specified, and the checksum match above for every mechanic used in a game. |
| Battle.net or other online play | not planned | | |

## 5. Beyond compatibility

| Area | State | Scope | Notes |
| --- | --- | --- | --- |
| Lifting engine limits | not started | unknown | Comes after melee compatibility. |
| New script APIs, decided in the open | not started | unknown | |
| Public wiki of the mechanics, built from the specs | not started | known | The specs already describe the mechanics; they need diagrams and a reader-friendly form. |

## Where we do not know the size of the work

These are the areas we cannot size yet, roughly from the largest
uncertainty down:

1. **Internal objects of every mechanic.** We need them for the turn
   checksum (see above). We only learn them by tracing each mechanic in the
   game.
2. **Unit abilities.** About 800 ability entries; the number of distinct
   mechanics behind them is not counted.
3. **The other three races.** Their workers, building rules (Undead blight,
   Night Elf uprooting, Orc burrows) and economy have not been looked at.
4. **Computer opponents.** The AI scripts run on natives whose behaviour is
   not specified.
5. **Network protocol** for games with the original client.
6. **Interface parity** with the game (menus, tooltips, every command card
   state).
7. **Items and shops**, beyond random drops.
8. **Flying, naval and amphibious units**, and invisibility.

Each of these gets a first look (a count of the mechanics it holds and a
test map or two) before we can put it into the table with a known scope.
