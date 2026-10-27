//! The engine's table of internal objects (SPEC-0024, "Object table"): the
//! entries objects hold, the free list (the last freed entry is taken
//! first) and the creation numbers. The turn checksums read the next
//! entry (`C`) and the next creation number (`D`) of this table.

/// One taking or freeing of an entry, for the comparison with the game's
/// lists (`<replay>.objects.tsv`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableEvent {
    pub tick: u64,
    /// True for a taking, false for a freeing.
    pub new: bool,
    pub entry: u32,
    /// The creation number of the object taking or leaving the entry.
    pub creation: u32,
    /// The mechanic, as the analyst's lists label it.
    pub tag: &'static str,
    /// The number of the script's native calls made before it.
    pub call: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ObjectTable {
    /// The number of entries the table has.
    size: u32,
    /// Free entries; the last one is taken next.
    free: Vec<u32>,
    /// The next creation number.
    next: u32,
    /// The next entry and creation number before the first object counted
    /// in the next tick (`Game::table_next`).
    pub boundary: Option<(u32, u32)>,
    /// Attack orders of units let go from a pause, carried out at that tick
    /// (`unpause_starts`).
    pub unpause_starts: Vec<(u64, u32)>,
    /// Objects made by a queue timer (`take_at_q`) and freed later, by key.
    made: std::collections::BTreeMap<u32, u32>,
    /// The next key of such an object.
    made_next: u32,
    /// The objects holding entries: entry -> creation number (`u32::MAX`
    /// in the log for an object made before the table was set).
    live: std::collections::BTreeMap<u32, u32>,
    /// Entries freed by a 0.0001 s timer of the world clock, in the order
    /// they were given up.
    deferred: Vec<(u64, u32, &'static str)>,
    /// The tick the game counts what happens now in: the tick, or the
    /// next one after the tick's queue time reached its end (SPEC-0018:
    /// the game's tick of a queue time is that of the world clock).
    pub now: u64,
    /// The parts of the time of day (its rate and the watches of the hours
    /// where something happens), replaced when the time is set.
    pub day: Vec<u32>,
    /// The walks freed after the updates of this tick (`walks_freed`).
    pub walks_due: Vec<Vec<u32>>,
    /// The pathing parts of each destructable, freed when it dies.
    pub destructable_paths: std::collections::BTreeMap<u32, Vec<u32>>,
    /// A gold mine had a worker inside (its object taken).
    pub mine_used: bool,
    /// The 0.0001 s frees given up by the tick's held impacts and hits
    /// ahead of its cooldowns, freed ahead of a later timer of the tick
    /// (`flush_early`).
    early: Vec<u32>,
    /// The spell order being taken starts its cast at once.
    pub cast_at_once: bool,
    /// An acquisition answering a call or a hit is being made (`acquired`).
    pub answering: bool,
    /// A Wind Walk's fade is being handled (`walk_dropped`).
    pub at_fade: bool,
    /// The unit whose death is being handled, and the objects of its
    /// attackers going on with their attack-moves (`attack_move_resumed`).
    pub dying: Option<u32>,
    /// The summon dying as its time ran out (`unit_died`).
    pub expiring: Option<u32>,
    /// The unit being made is a building whose construction starts
    /// (`unit_made`, `construction_started`).
    pub constructing: bool,
    /// Work on finished buildings that ends at a tick: the building and
    /// its workers (`work_ended`).
    pub work_ends: Vec<(u64, u32, Vec<u32>)>,
    pub resumed: Vec<u32>,
    /// The walks of group orders (`group_move_ordered`) and the members
    /// still on them.
    pub group_walks: Vec<(Vec<u32>, Vec<u32>)>,
    /// Each gold mine's rate part while its timer runs (`mine_queue_timer`).
    pub mine_queues: std::collections::BTreeMap<u32, u32>,
    /// The last frees of a death, after the experience it gives
    /// (`death_rest`): deferred ones and those at a queue time.
    rest: Vec<(u32, &'static str)>,
    /// The death queue object that goes after the death's events
    /// (`death_after`).
    rest_after: Vec<u32>,
    /// Units whose order of the game ended at its point in the updates
    /// (`game_order_arrived`).
    pub homes: Vec<u32>,
    /// Mirror Image (`mirror_copy`, `mirror_missile`): the objects kept by
    /// each copy of the hero, in the order they go, and those of the
    /// missile to each copy's place.
    copies: std::collections::BTreeMap<u32, Vec<u32>>,
    /// The missile objects of units killed by the hit that stunned them.
    dead_missiles: std::collections::BTreeMap<u32, Vec<u32>>,
    /// The parts of the copies' states freed after their enter events
    /// (`copies_freed`).
    copy_parts: Vec<u32>,
    missiles: std::collections::BTreeMap<u32, Vec<u32>>,
    rest_q: Vec<(u32, u32)>,
    rest_w: Vec<(u32, u32)>,
    /// Objects freed in a later tick's batch (`free_at`).
    timed: Vec<(u64, u32, &'static str)>,
    timed_q: Vec<(u32, u32, &'static str)>,
    /// The hit buffs of Shockwaves: caster, unit, object.
    pub shock_buffs: Vec<(u32, u32, u32)>,
    /// Objects freed by a timer of the world clock (`free_at_clock`).
    timed_w: Vec<(u32, u32, &'static str)>,
    /// Units whose order ends at the end of the tick.
    pub ends_late: Vec<u32>,
    /// The takings and freeings, when kept.
    pub log: Option<Vec<TableEvent>>,
    /// The world clock at the start of each tick, kept with the log.
    pub clocks: Option<Vec<(u64, u32)>>,
    /// The number of the script's native calls made so far.
    pub call: u32,
}

impl ObjectTable {
    /// A table of `size` entries whose free list is `free` (taken from the
    /// end) and whose next creation number is `next`.
    pub fn new(size: u32, free: Vec<u32>, next: u32) -> ObjectTable {
        ObjectTable {
            size,
            free,
            next,
            boundary: None,
            unpause_starts: Vec::new(),
            made: Default::default(),
            made_next: 0,
            live: Default::default(),
            deferred: Vec::new(),
            now: 0,
            day: Vec::new(),
            walks_due: Vec::new(),
            mine_used: false,
            destructable_paths: std::collections::BTreeMap::new(),
            early: Vec::new(),
            cast_at_once: false,
            at_fade: false,
            answering: false,
            dying: None,
            expiring: None,
            constructing: false,
            work_ends: Vec::new(),
            resumed: Vec::new(),
            group_walks: Vec::new(),
            mine_queues: std::collections::BTreeMap::new(),
            rest: Vec::new(),
            rest_after: Vec::new(),
            rest_q: Vec::new(),
            rest_w: Vec::new(),
            homes: Vec::new(),
            copies: Default::default(),
            dead_missiles: Default::default(),
            copy_parts: Vec::new(),
            missiles: Default::default(),
            timed: Vec::new(),
            timed_q: Vec::new(),
            shock_buffs: Vec::new(),
            timed_w: Vec::new(),
            ends_late: Vec::new(),
            log: None,
            clocks: None,
            call: 0,
        }
    }

    /// The next entry (`C` of SPEC-0024): the head of the free list, or
    /// the size of the table when it is empty.
    pub fn next_entry(&self) -> u32 {
        self.free.last().copied().unwrap_or(self.size)
    }

    /// The creation number of the object holding `entry`.
    pub fn creation_of(&self, entry: u32) -> u32 {
        self.live.get(&entry).copied().unwrap_or(u32::MAX)
    }

    /// The next creation number (`D` of SPEC-0024).
    pub fn next_creation(&self) -> u32 {
        self.next
    }

    /// A new object takes an entry; gives the entry.
    pub fn take(&mut self, tick: u64, tag: &'static str) -> u32 {
        let entry = self.free.pop().unwrap_or_else(|| {
            self.size += 1;
            self.size - 1
        });
        let creation = self.next;
        self.next += 1;
        self.live.insert(entry, creation);
        if let Some(log) = self.log.as_mut() {
            log.push(TableEvent {
                tick,
                new: true,
                entry,
                creation,
                tag,
                call: self.call,
            });
        }
        entry
    }

    /// The object holding `entry` is freed; the entry goes on the free list.
    pub fn free(&mut self, tick: u64, entry: u32, tag: &'static str) {
        let creation = self.live.remove(&entry).unwrap_or(u32::MAX);
        self.free.push(entry);
        if let Some(log) = self.log.as_mut() {
            log.push(TableEvent {
                tick,
                new: false,
                entry,
                creation,
                tag,
                call: self.call,
            });
        }
    }

    /// `n` new objects take entries, in order; gives the entries.
    pub fn take_n(&mut self, tick: u64, n: usize, tag: &'static str) -> Vec<u32> {
        (0..n).map(|_| self.take(tick, tag)).collect()
    }

    /// The object holding `entry` is freed by a 0.0001 s timer of the
    /// world clock (`flush`).
    /// The creation number of the object holding `entry`.
    pub fn creation(&self, entry: u32) -> Option<u32> {
        self.live.get(&entry).copied()
    }

    pub fn defer(&mut self, entry: u32, tag: &'static str) {
        self.deferred.push((self.now, entry, tag));
    }

    /// The object holding `entry` is freed by a timer of the queue expiring
    /// at `q` (`flush_q`).
    pub fn free_at_q(&mut self, q: u32, entry: u32, tag: &'static str) {
        self.timed_q.push((q, entry, tag));
    }

    /// An object made by a queue timer at `q`, kept: its key, which
    /// `free_at_q` takes as its entry for its free.
    pub fn take_at_q(&mut self, q: u32, tag: &'static str) -> u32 {
        let key = MADE_KEY | self.made_next;
        self.made_next += 1;
        self.timed_q.push((q, key | MADE_NOW, tag));
        key
    }

    /// Whether a timer's free of `entry` may join the 0.0001 s frees: not
    /// one making an object, nor the free of one not made yet.
    fn movable(&self, entry: u32) -> bool {
        entry != TAKE_AT
            && entry & MADE_NOW == 0
            && (entry & MADE_KEY == 0 || self.made.contains_key(&entry))
    }

    /// The queue's timers that expire before `limit`, a script timer of
    /// this tick: their frees join the tick's 0.0001 s frees now, ahead of
    /// those the script timer makes (c-RZJ-1: the Shockwave's wave at
    /// 9.499616, before the orders of the script's 9.5).
    /// `queue_before` with the timers at `limit` itself.
    pub fn queue_through(&mut self, limit: u32) {
        let now = self.now;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timed_q)
            .into_iter()
            .partition(|&(q, e, _)| self.movable(e) && f32::from_bits(q) <= f32::from_bits(limit));
        self.timed_q = later;
        for (_, entry, tag) in due {
            let entry = self.made.remove(&entry).unwrap_or(entry);
            self.deferred.push((now, entry, tag));
        }
    }

    pub fn queue_before(&mut self, limit: u32) {
        let now = self.now;
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timed_q)
            .into_iter()
            .partition(|&(q, e, _)| self.movable(e) && f32::from_bits(q) < f32::from_bits(limit));
        self.timed_q = later;
        for (_, entry, tag) in due {
            let entry = self.made.remove(&entry).unwrap_or(entry);
            self.deferred.push((now, entry, tag));
        }
    }

    /// The queue's timers of this tick, up to `limit`, free their objects.
    pub fn flush_q(&mut self, tick: u64, limit: u32) {
        let (mut due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timed_q)
            .into_iter()
            .partition(|&(q, _, _)| f32::from_bits(q) <= f32::from_bits(limit));
        self.timed_q = later;
        // An object made at the timer, before its frees, freed in its place
        // among them (`TAKE_AT`), or kept (`take_at_q`).
        let mut frees = Vec::new();
        for d in &mut due {
            if d.1 == TAKE_AT {
                d.1 = self.take(tick, d.2);
            } else if d.1 & MADE_NOW != 0 {
                let e = self.take(tick, d.2);
                self.made.insert(d.1 & !MADE_NOW, e);
                continue;
            }
            frees.push(*d);
        }
        for (_, entry, tag) in frees {
            let entry = self.made.remove(&entry).unwrap_or(entry);
            self.free(tick, entry, tag);
        }
    }

    /// The object holding `entry` is freed by a timer of the world clock
    /// expiring at `clock` (`TAKE_AT` for an object made there).
    pub fn free_at_clock(&mut self, clock: u32, entry: u32, tag: &'static str) {
        self.timed_w.push((clock, entry, tag));
    }

    /// The world clock's timers of the tick, up to `limit`, free their
    /// objects; the objects they make come first.
    pub fn flush_clock(&mut self, tick: u64, limit: u32) {
        let (mut due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timed_w)
            .into_iter()
            .partition(|&(c, _, _)| f32::from_bits(c) <= f32::from_bits(limit));
        self.timed_w = later;
        for d in &mut due {
            if d.1 == TAKE_AT {
                d.1 = self.take(tick, d.2);
            }
        }
        for (_, entry, tag) in due {
            self.free(tick, entry, tag);
        }
    }

    /// The object holding `entry` is freed in the 0.0001 s batch of tick
    /// `tick`.
    pub fn free_at(&mut self, tick: u64, entry: u32, tag: &'static str) {
        self.timed.push((tick, entry, tag));
    }

    /// How many 0.0001 s frees are waiting.
    pub fn deferred_count(&self) -> usize {
        self.deferred.len()
    }

    /// The 0.0001 s frees given up since `from` go by a queue timer at `q`
    /// instead, after the tick's 0.0001 s frees.
    pub fn defer_to_q(&mut self, from: usize, q: u32) {
        let moved: Vec<_> = self.deferred.drain(from..).collect();
        for (_, entry, tag) in moved {
            self.timed_q.push((q, entry, tag));
        }
    }

    /// The 0.0001 s frees given up since `from` count as early (`flush_early`).
    pub fn mark_early(&mut self, from: usize) {
        let new: Vec<u32> = self.deferred.iter().skip(from).map(|d| d.1).collect();
        self.early.extend(new);
    }

    /// The early 0.0001 s frees of the tick, ahead of a timer later in it
    /// (`auras::tick`).
    pub fn flush_early(&mut self, tick: u64) {
        let early = std::mem::take(&mut self.early);
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.deferred)
            .into_iter()
            .partition(|&(t, e, _)| t <= tick && early.contains(&e));
        self.deferred = later;
        for (_, entry, tag) in due {
            self.free(tick, entry, tag);
        }
    }

    /// The 0.0001 s timers of the tick expire: their objects are freed in
    /// the order they were given up.
    pub fn flush(&mut self, tick: u64) {
        self.early.clear();
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.deferred)
            .into_iter()
            .partition(|&(t, _, _)| t <= tick);
        self.deferred = later;
        for (_, entry, tag) in due {
            self.free(tick, entry, tag);
        }
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timed)
            .into_iter()
            .partition(|&(t, _, _)| t <= tick);
        self.timed = later;
        for (_, entry, tag) in due {
            self.free(tick, entry, tag);
        }
    }
}

/// The entries a unit holds that are freed later than 0.0001 s after its
/// making.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UnitEntries {
    /// Freed at its death, in this order.
    pub death: Vec<u32>,
    /// Its id: the entry and creation number of the first object of its
    /// making (SPEC-0024, `idsum`).
    pub id: Option<[u32; 2]>,
    /// A dead hero: the queue time it joins its player's list of dead heroes.
    pub dead_listed: Option<u32>,
    /// A dead unit not a hero: the first five of its death order's queue,
    /// and its corpse's end with the objects freed then, for a
    /// Resurrection (`resurrected`).
    pub dead_queue: Vec<u32>,
    /// An Avatar's buff and watch, kept to its end.
    pub avatar: Vec<u32>,
    /// A worker's build order: its order and kept parts, freed at the
    /// construction's start, the sixth part first.
    pub build_order: Vec<u32>,
    /// A worker's work on a building: its order and kept parts.
    pub work: Vec<u32>,
    /// A building under construction: its progress pair, its dispel's
    /// five (from its completion) and the work's part kept to the end.
    pub construction: Vec<u32>,
    pub dispel: Vec<u32>,
    pub construction_work: Option<u32>,
    pub first_helper: Option<u32>,
    /// A builder turning to its building: the fifth part of its work order,
    /// let go when it faces it (`turned_to_work`).
    pub turn_part: Option<u32>,
    /// Engaged in a fight since it was last idle (hub #14): its acquisitions
    /// take the short layout.
    pub engaged: bool,
    /// Its acquisition made its chase walk: none at its reaction's end.
    pub walk_at_acquire: bool,
    /// The object a wake's attack-move left kept (`creeps::wake_checks`).
    pub wake_part: Option<u32>,
    /// Its next acquisition check finds nothing (after a guard give-up).
    pub skip_check: bool,
    /// A building's objects of its making, all of them (for a cancel).
    pub made: Vec<u32>,
    /// A helper's order part freed when its help starts.
    pub repair_part: Option<u32>,
    /// The rate parts of a building's life set anew by its helpers.
    pub construction_rates: Vec<u32>,
    pub dead_corpse: Option<(u32, Vec<u32>)>,
    /// A creep's ability parts freed at its death: before all its other
    /// objects (an aura's first), and with its buffs after the experience
    /// (an aura's buff, a costed ability's); its mana's part, in its core
    /// after the first (c-RZJ-1: the Ogre Lord at 37.137905: 1093 first,
    /// then 1094 and 1096, then 1087, 1089, 1088).
    pub death_first: Vec<u32>,
    pub death_late: Vec<u32>,
    pub death_mana: Option<u32>,
    /// The objects of its making kept to its corpse's end, in the order
    /// they go then (`unit_died`).
    pub corpse: Vec<u32>,
    /// The parts of its state a hero's level replaces (`level_changed`).
    pub level: Vec<u32>,
    /// It holds the objects of a creep camp (it was its first creep).
    pub camp: bool,
    /// The objects of its current order, freed in this order when the
    /// order ends.
    pub order: Vec<u32>,
    /// Its walk: the path and the cluster.
    pub walk: Vec<u32>,
    /// The objects of walks it gave up and the tick it gave them up,
    /// freed at its first update after that tick.
    pub stale: Vec<(u64, u32)>,
    /// The update at which those walks go if the unit stands by then.
    pub stale_update: Option<u64>,
    /// Its walk was made at its order; the first search of the order
    /// takes no new one.
    pub walk_ready: bool,
    /// It walks on its group's walk (`group_move_ordered`): its searches
    /// take none of their own.
    pub group_walk: bool,
    /// The object of its order that came back after a stun, and the tick
    /// the stun's timer frees it in.
    pub order_back: Option<(u32, u64)>,
    /// Its walk was made at its follow leg's end (`follow_leg_ended`).
    pub follow_walk: bool,
    /// It was harvesting when stunned (`stun_objects`).
    pub stun_harvest: bool,
    /// Its walk was made at a backswing's end for the chase of the next
    /// tick, whose order keeps it.
    pub early_walk: bool,
    /// The tick the early walk was made in.
    pub early_walk_at: u64,
    /// The queue time of that walk (the backswing's end).
    pub early_walk_q: u32,
    /// The goal after its swing was taken at the backswing's end; the
    /// chase takes no other.
    pub early_goal: bool,
    /// The backswing end at which it took the goal of a target it chases
    /// with no attack under way (`attack::after_swing_walks`).
    pub swing_goal_at: Option<u32>,
    /// Its walk is that of a goal taken in reach after a swing, which ends
    /// when it faces its target.
    pub quiet_walk: bool,
    /// The objects of its Wind Walk freed at the fade and at the end.
    pub fade: Vec<u32>,
    pub lasting: Vec<u32>,
    /// The object of its order freed when its cast starts.
    pub cast_start: Vec<u32>,
    /// The object of its order freed at its spell's impact.
    pub impact: Vec<u32>,
    /// A stun: its objects freed at its end, its order's objects freed
    /// when the order comes back, the ticks of both, and the object of
    /// its buff's end.
    pub stun_end: Vec<u32>,
    pub stun_order: Vec<u32>,
    pub stun_ticks: Option<(u64, u64)>,
    /// A stun of Impale: at its end a 0.00 hit from the caster comes before
    /// the stun's objects go, and there is no buff end (`impale_ends`).
    pub impale: bool,
    /// Its order is a script's attack order on a unit (`attack_target_ordered`).
    pub script_attack: bool,
    /// The objects of such an order lost with its target, due at the end of
    /// the backswing (`script_attack_lost`), with that queue time.
    pub order_ending: Vec<(u32, u32)>,
    /// A lasting item effect on it: its object and the queue time it ends
    /// (`item_effect`).
    pub item_effect: Option<(u32, u32)>,
    /// The object of a script's invulnerability (`invulnerable_set`).
    pub invulnerable: Option<u32>,
    pub buff_end: Option<u32>,
    /// The queue time of its stun's end.
    pub stun_end_q: u32,
    /// It had an order of the game when stunned.
    pub stun_game_order: bool,
    /// The objects of its acquisition freed at the end of its reaction
    /// delay and at its target's death.
    /// The objects of its last acquisition freed at the next.
    pub acquisition: Vec<u32>,
    pub reaction: Vec<u32>,
    pub target_death: Vec<(u32, u32)>,
    /// It attacks by an order of its own from an acquisition.
    pub own_attack: bool,
    /// The creation number at its last acquisition, for the order of the
    /// attackers at a target's death.
    pub taken_at: u32,
    /// Its last target was taken by answering a call.
    pub by_call: bool,
    /// Its last target was taken by a call of its camp or by answering a
    /// call, not by its own check.
    pub called: bool,
    /// At a death being handled: the objects it kept to it, and whether
    /// its order ends too.
    pub lost: Vec<u32>,
    pub lose_order: bool,
    pub lose_acquisition: bool,
    /// The objects kept to a target's death that wait for the end of the
    /// unit's backswing (`lost_at_backswing_end`).
    pub keep_lost: bool,
    pub lost_later: Vec<u32>,
    /// The object of a thunder clap's slow on the unit, and the queue
    /// time the slow ends.
    pub slow_buff: Option<u32>,
    pub slow_end_q: Option<u32>,
    /// Its timed fog modifier as an area spell's source, and the queue
    /// time it runs to.
    pub fog: Option<(u32, u32)>,
    /// A buff end order waiting behind its move.
    pub buff_end_waiting: Option<u32>,
    /// The object of a channel under way (Blizzard).
    pub channel: Option<u32>,
    /// The object of a Blizzard wave's buff on it.
    pub blizzard_buff: Option<(u32, u32)>,
    /// The parts of a harvest order freed when the walk to the mine ends.
    pub harvest_arrive: Vec<u32>,
    /// The objects of its queued orders, in their order.
    pub queued: std::collections::VecDeque<u32>,
    /// An object of its arrival at a tree, freed at its next update, and
    /// that update's tick.
    pub tree_arrival: Option<(u32, u64)>,
    /// The tick of the update at which its follow leg ends.
    pub follow_leg_end: Option<u64>,
    /// The object of its wait for another tree.
    pub tree_wait: Option<u32>,
    /// Its object while inside a gold mine.
    pub harvest_inside: Option<u32>,
    /// The objects of the aura buffs on it, by aura ability; `u32::MAX`
    /// for one whose object is the bearer's own (made with it).
    pub aura_buffs: Vec<(i32, u32)>,
    /// The object of its attack ability, made with the unit.
    pub attack_ability: Option<u32>,
    /// It walks back to its guard point after its target's death.
    pub guard_walk: bool,
    /// Its attack-move was taken up again after an attack
    /// (`attack_move_arrived`).
    pub resumed_move: bool,
    /// Its Mirror Image's effect object and the costs of the mana watches
    /// it gave up when hidden (`mirror_effect`).
    pub mirror_effect: Option<u32>,
    pub hidden_watches: Vec<i32>,
    /// Its order is one of the game (a creep's return home).
    pub game_order: bool,
    /// The order and the extra object of its order of the game.
    pub game_objects: Vec<u32>,
    /// Watches on its mana, one per ability with a cost: the cost, the
    /// entry, and whether the mana is at or above it.
    pub mana_watches: Vec<(i32, u32, bool)>,
    /// The next check of each watch below its cost, by its entry
    /// (`watches_checked`).
    pub watch_checks: Vec<(u32, u32)>,
    /// The world clock of the last re-base of its mana (a cost paid or a
    /// change of the rate), from which the watches count.
    pub watch_base: u32,
    /// The objects of an order it gave up in this tick, freed at its end.
    pub old_order: Vec<u32>,
    /// The object of its own attack's current target.
    pub current: Option<u32>,
}

/// The objects a unit made by `CreateUnit` takes (SPEC-0024, the analyst's
/// reading of c-SAK-1's map start), in the order of the game: their
/// number, the positions freed 0.0001 s later (in the order of the frees),
/// those freed at its death and those a hero's level replaces.
///
/// PROVISIONAL: counted for the types of c-SAK-1 only (Hmkg, Obla: hero;
/// hfoo: unit; nkob, ngno, nogr: creep) until the parts of a unit are
/// specified; a creep's last 5 are the objects of its guard order, and the
/// first creep of a group takes 1 more at 17 (c-SAK-1: kobolds 8 and 15).
struct Making {
    count: usize,
    deferred: &'static [usize],
    death: &'static [usize],
    level: &'static [usize],
    /// The rest, kept to the corpse's end, in the order they go then.
    corpse: &'static [usize],
}

const HERO: Making = Making {
    count: 24,
    deferred: &[17, 18, 23, 22],
    death: &[10, 11],
    level: &[19, 20],
    corpse: &[],
};
// A hero of the creeps: a hero's 24, a creep's guard parts (24, 25) and
// guard order (26 .. 34, freed 0.0001 s later), then one more, kept
// (c-SBG-1: paladin 2). PROVISIONAL: what goes at its death is not read.
const CREEP_HERO: Making = Making {
    count: 36,
    deferred: &[17, 18, 23, 22, 27, 32, 31, 30, 29, 28, 26, 34, 33],
    death: &[10, 11, 24, 25],
    level: &[19, 20],
    corpse: &[],
};
// PROVISIONAL: the corpse orders but a creep's are taken by the creep's
// pattern (c-SAK-1: kobold 9's corpse at 106.758).
const UNIT: Making = Making {
    count: 18,
    deferred: &[17, 16],
    death: &[10, 11],
    level: &[],
    corpse: &[15, 14, 13, 12, 8, 7, 6, 5, 4, 9, 2, 3, 1, 0],
};
// A unit without an ability of its own (only the hidden inventory one,
// `Aihn`, `Aion`): the same without the ability's object 14 (c-RXR-1: the
// riflemen, 17 each; footmen, grunts and headhunters 18 with Defend,
// Pillage and Berserk).
const UNIT_PLAIN: Making = Making {
    count: 17,
    deferred: &[16, 15],
    death: &[10, 11],
    level: &[],
    corpse: &[14, 13, 12, 8, 7, 6, 5, 4, 9, 2, 3, 1, 0],
};
// A peasant (c-RXI-1, 22 objects): the footman's 0 .. 11, Build's ability
// and second part (12, 13), Move (14), Attack (15), Harvest (16), Call to
// Arms (17), Repair (18), a part of Harvest's (19), the start order's two
// (20, 21, freed 0.0001 s later).
// At its death Harvest's part goes first, then the footman's two (c-RXD-1:
// 33.315006). PROVISIONAL: the corpse's end is not read; taken as the
// footman's, the other extra parts with it.
const PEASANT: Making = Making {
    count: 22,
    deferred: &[21, 20],
    death: &[19, 10, 11],
    level: &[],
    corpse: &[18, 17, 16, 15, 14, 13, 12, 8, 7, 6, 5, 4, 9, 2, 3, 1, 0],
};
// A gold mine (c-RXI-1, 23): the footman's 0 .. 9 and 11, three building
// parts (11 .. 13), the gold ability and its second part (14, 15), one
// more ability object (16), Invulnerable (17), on fire and alarm (18,
// 19), the start order's two (20, 21, freed 0.0001 s later) and one more
// kept (22).
const GOLD_MINE: Making = Making {
    count: 23,
    deferred: &[21, 20],
    death: &[],
    level: &[],
    corpse: &[],
};
/// A building with a Blight Dispel (c-SBT-1, hub #8: ten Human buildings;
/// c-RXI-1: the town hall; c-SBQ-1: the altar and the castle), laid out by
/// [`building_layout`]: none of its own objects go at its death here.
const BUILDING: Making = Making {
    count: 0,
    deferred: &[],
    death: &[],
    level: &[],
    corpse: &[],
};

/// The table objects of a building of type `type_id` with a Blight Dispel
/// (Abds or Abdl), if it has one: their count, where the dispel's five
/// start and how long after the making they go (c-SBT-1, hub #8).
///
/// - 14 as a unit's first, its attack (1), a queue with its second part
///   (2: it trains, researches, revives or sells), an upgrade with its
///   second part (2);
/// - the dispel's ability and four more (5), freed together when the
///   dispel has finished (`DISPEL_PARTS`);
/// - two more, then one for each of its other abilities, a rally (1: it
///   trains or revives; its point is an object of the second pool, not
///   here), three for selling items;
/// - its start order's two, freed 0.0001 s later.
///
/// The dispel's radius starts at DataA and grows by DataA every Dur, up
/// to the Area rounded up to whole 128s, so `2 x ceil(Area / 128) - 1`
/// steps when DataA is 64 (c-SBT-1: 11 steps of 0.08 s for Abds, Area
/// 768; 15 for Abdl, Area 960).
///
/// PROVISIONAL: the step rule fits two Areas, not read as a value; blight
/// on the map is not tested. The three of selling items are read on the
/// arcane vault only.
fn building_layout(state: &super::State, type_id: i32) -> Option<(usize, usize, f32)> {
    let t = state.data.units.get(&type_id)?;
    let (&id, dispel) = t.abilities.iter().find_map(|a| {
        [*b"Abds", *b"Abdl"]
            .contains(&a.to_be_bytes())
            .then(|| Some((a, state.data.abilities.get(a)?)))
            .flatten()
    })?;
    let step = f32::from_bits(dispel.duration);
    let grow = f32::from_bits(dispel.data[0]);
    let area = f32::from_bits(dispel.area);
    let reach = (area / 128.0).ceil() * 128.0;
    let steps = if grow > 0.0 {
        (reach / grow) as usize - 1
    } else {
        0
    };
    let after = step * steps as f32;
    let head = 14
        + usize::from(t.weapon.is_some() || t.weapon2.is_some())
        + 2 * usize::from(t.queue)
        + 2 * usize::from(t.upgrades);
    let others = t.abilities.iter().filter(|&&a| a != id).count();
    let tail = 2 + others + usize::from(t.rally) + 3 * usize::from(t.sells_items);
    Some((head + 5 + tail + 2, head, after))
}

/// The five of a building's dispel, from its first, in the order they go
/// (c-SBT-1, c-SBQ-1: 19, 20, 18, 17, 16 of the altar at 1.880098).
const DISPEL_PARTS: [usize; 5] = [3, 4, 2, 1, 0];
const CREEP: Making = Making {
    count: 28,
    deferred: &[16, 15, 20, 25, 24, 23, 22, 21, 19, 27, 26],
    death: &[10, 11, 17, 18],
    level: &[],
    corpse: &[14, 13, 12, 8, 7, 6, 5, 4, 9, 2, 3, 1, 0],
};
const CAMP_CREEP: Making = Making {
    count: 29,
    deferred: &[16, 15, 21, 26, 25, 24, 23, 22, 20, 28, 27],
    death: &[10, 11, 18, 19],
    level: &[],
    corpse: &[17, 14, 13, 12, 8, 7, 6, 5, 4, 9, 2, 3, 1, 0],
};

/// A summoned unit (SPEC-0044): a player's unit's 18 without its two
/// ability objects (14, 15), with its timed life (16, freed at its death)
/// (c-RZJ-1: the first Water Elemental, 1465 .. 1481).
const SUMMON: Making = Making {
    count: 17,
    deferred: &[15, 14],
    death: &[16, 10, 11],
    level: &[],
    corpse: &[13, 12, 8, 7, 6, 5, 4, 9, 2, 3, 1, 0],
};

/// Unit `h` was summoned by `caster` (SPEC-0044): the spell effect's
/// event, then the unit's objects; the summon event's objects (1482,
/// 1483) follow with the event (c-RZJ-1 at 2.3).
///
/// PROVISIONAL: read for the Water Elemental only.
pub(super) fn summon_made(state: &mut super::State, caster: u32, h: u32, cost: i32) {
    spell_effect_event(state, caster);
    // The watches its cost crosses, before the unit (c-RZJ-1: 3580 and
    // 3581 at the second Water Elemental, 22.797174).
    mana_watches(state, caster, cost);
    let tick = state.table.now;
    let entries = state.table.take_n(tick, SUMMON.count, "summon: unit");
    for &k in SUMMON.deferred {
        state.table.defer(entries[k], "summon: unit");
    }
    if let Some(super::Object::Unit(u)) = state.objects.get_mut(&h) {
        u.entries = UnitEntries {
            death: SUMMON.death.iter().map(|&k| entries[k]).collect(),
            corpse: SUMMON.corpse.iter().map(|&k| entries[k]).collect(),
            attack_ability: Some(entries[13]),
            ..UnitEntries::default()
        };
    }
}

/// The abilities of type `type_id` other than the hidden inventory ones.
///
/// PROVISIONAL: a unit with two of its own is taken as one with one.
fn own_abilities(state: &super::State, type_id: i32) -> usize {
    state.data.units.get(&type_id).map_or(1, |t| {
        t.abilities
            .iter()
            .filter(|&&a| ![*b"Aihn", *b"Aion", *b"Aien", *b"Aiun"].contains(&a.to_be_bytes()))
            .count()
    })
}

/// A hero of type `type_id`: how many fewer objects it has before its
/// attack ability, and how many its own abilities add.
fn hero_layout(state: &super::State, type_id: i32) -> (usize, usize) {
    let Some(t) = state.data.units.get(&type_id) else {
        return (0, 0);
    };
    let fewer = usize::from(t.race == "nightelf");
    let extra = t
        .abilities
        .iter()
        .map(|a| match &a.to_be_bytes() {
            b"AInv" | b"Aihn" | b"Aion" | b"Aien" | b"Aiun" => 0,
            _ => 1,
        })
        .sum();
    (fewer, extra)
}

/// Unit `h` was made by `CreateUnit`: it takes its objects.
///
/// The first creep of a group (SPEC-0030, "Joining") holds the objects of
/// the group.
pub(super) fn unit_made(state: &mut super::State, h: u32) {
    use super::Object;
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let (owner, type_id) = (u.owner, u.type_id);
    let building = (owner != super::creeps::CREEPS)
        .then(|| building_layout(state, type_id))
        .flatten();
    let making = if crate::data::is_hero(type_id) {
        if owner == super::creeps::CREEPS {
            &CREEP_HERO
        } else {
            &HERO
        }
    } else if owner == super::creeps::CREEPS {
        if super::creeps::group_of(state, h) == [h] {
            &CAMP_CREEP
        } else {
            &CREEP
        }
    // The peon too (c-SBI-1: 22, the start order's two deferred).
    } else if type_id == i32::from_be_bytes(*b"hpea") || type_id == i32::from_be_bytes(*b"opeo") {
        &PEASANT
    } else if type_id == i32::from_be_bytes(*b"ngol") {
        &GOLD_MINE
    } else if building.is_some() {
        &BUILDING
    } else if own_abilities(state, type_id) == 0 {
        &UNIT_PLAIN
    } else {
        &UNIT
    };
    // A creep's own abilities (`abilList`) take their objects before the
    // creep's ability object (14), and one with mana a rate part at 12
    // (c-RZJ-1: the Ogre Lord, 34: its mana, Devotion Aura's ability,
    // aura and buff, Shockwave's ability and cost watch; c-RZH-1: unit
    // 10, nkot, the creep Bash's ability; c-SBL-1: the kobold taskmaster's
    // Command Aura).
    // PROVISIONAL: the aura's buff of Brilliance and a second costed
    // ability are not read on a creep.
    let creep = owner == super::creeps::CREEPS && !crate::data::is_hero(type_id);
    let (mana, abilities): (bool, Vec<i32>) = state
        .data
        .units
        .get(&type_id)
        .filter(|_| creep)
        .map_or((false, Vec::new()), |t| {
            (f32::from_bits(t.max_mana) > 0.0, t.abilities.clone())
        });
    // (freed at its death: 1 first, 2 with its buffs, 3 0.0001 s later;
    // cost of a watch)
    let mut parts: Vec<(u8, Option<i32>)> = Vec::new();
    for a in abilities {
        let Some(d) = state.data.abilities.get(&a) else {
            continue;
        };
        let level = d.per_level.first();
        if [*b"AHab", *b"AHad", *b"AOae", *b"AUau", *b"AOac"].contains(&d.code.to_be_bytes()) {
            parts.extend([(0, None), (1, None), (2, None)]);
        // Spiked Carapace (the turtle's Thorns Aura, ANth) two (c-SCA-1,
        // hub #14; PROVISIONAL: when they go is not read).
        } else if d.code.to_be_bytes() == *b"AUts" {
            parts.extend([(0, None), (0, None)]);
        } else if level.is_some_and(|l| l.cost > 0) {
            parts.extend([(0, None), (2, level.map(|l| l.cost))]);
        } else {
            parts.push((0, None));
        }
    }
    // An autocast on from the start (`auto`) takes one more, freed 0.0001 s
    // later; a costed ability its cost watch also without a cooldown
    // (c-SBK-1: the kobold geomancer, Slow on, Abolish Magic of no
    // cooldown).
    if creep && state.data.units.get(&type_id).is_some_and(|t| t.auto != 0) {
        parts.push((3, None));
    }
    // A hero's own abilities (`abilList`, but the inventory) take theirs
    // before its last two (c-SBK-1: Ultravision 1, Shadow Meld 1; c-SBH-1:
    // the Blood Mage's spheres 1, and 3 of the second pool), and a Night Elf hero has one fewer
    // before its attack ability (c-SBK-1: 12 before it, 13 in c-SBJ-1).
    // PROVISIONAL: what the missing object is, and the race as its rule,
    // are not read.
    let (fewer, extra) = if crate::data::is_hero(type_id) {
        hero_layout(state, type_id)
    } else {
        (0, 0)
    };
    let shift = usize::from(mana);
    // A unit with more abilities of its own than one: an object more for
    // each after the first, after its first (c-SCA-1: the archer, 21 with
    // Aco2, Ashm and Ault, hub #14).
    // PROVISIONAL: where the extra ones go at the corpse's end is not read.
    let more = if std::ptr::eq(making, &UNIT) {
        // (Shadowmeld two: PROVISIONAL, which of the three takes the
        // second is not read.)
        let shadowmeld = state
            .data
            .units
            .get(&type_id)
            .is_some_and(|t| t.abilities.contains(&i32::from_be_bytes(*b"Ashm")));
        own_abilities(state, type_id).saturating_sub(1) + usize::from(shadowmeld)
    } else {
        0
    };
    let at = |k: usize| {
        let k = if k >= 15 { k + more } else { k };
        let hero = if k >= 22 { k + extra } else { k };
        let hero = if k >= 13 { hero - fewer } else { hero };
        let k = if hero >= 12 { hero + shift } else { hero };
        if k >= 14 + shift { k + parts.len() } else { k }
    };
    let count = building.map_or(making.count, |b| b.0) + shift + parts.len() + extra - fewer + more;
    let tick = state.table.now;
    let entries = state.table.take_n(tick, count, "map start: unit");
    // (The autocast's first: c-SBK-1, 928.)
    for (i, _) in parts.iter().enumerate().filter(|(_, p)| p.0 == 3) {
        state
            .table
            .defer(entries[14 + shift + i], "map start: autocast");
    }
    for &k in making.deferred {
        state.table.defer(entries[at(k)], "map start: unit");
    }
    let mut pending_dispel = Vec::new();
    if let Some((count, at, after)) = building {
        state.table.defer(entries[count - 1], "map start: unit");
        state.table.defer(entries[count - 2], "map start: unit");
        // A building under construction dispels from its completion
        // (`construction_finished`; c-SBZ-1: the farm, hub #13).
        if std::mem::take(&mut state.table.constructing) {
            pending_dispel = DISPEL_PARTS.map(|k| entries[at + k]).to_vec();
        } else {
            let end = ow3_math::add(state.timers.time(), after.to_bits());
            for k in DISPEL_PARTS {
                state
                    .table
                    .free_at_q(end, entries[at + k], "building: dispel");
            }
        }
    }
    let death: Vec<u32> = making.death.iter().map(|&k| entries[at(k)]).collect();
    let death_mana = mana.then(|| entries[12]);
    let (mut death_first, mut death_late) = (Vec::new(), Vec::new());
    let mut watches = Vec::new();
    for (i, &(dies, cost)) in parts.iter().enumerate() {
        let e = entries[14 + shift + i];
        match dies {
            1 => death_first.push(e),
            2 => death_late.push(e),
            _ => {}
        }
        if let Some(c) = cost {
            watches.push((c, e, true));
        }
    }
    let id = [entries[0], state.table.creation_of(entries[0])];
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.entries = UnitEntries {
            id: Some(id),
            death,
            death_first,
            death_late,
            death_mana,
            mana_watches: watches,
            corpse: {
                let mut corpse: Vec<u32> = making.corpse.iter().map(|&k| entries[at(k)]).collect();
                if let Some(i) = corpse
                    .iter()
                    .position(|&e| e == entries[14])
                    .filter(|_| more > 0)
                {
                    let extra: Vec<u32> = (15..15 + more).rev().map(|k| entries[k]).collect();
                    corpse.splice(i..i, extra);
                }
                corpse
            },
            level: making.level.iter().map(|&k| entries[at(k)]).collect(),
            camp: std::ptr::eq(making, &CAMP_CREEP),
            dispel: pending_dispel,
            made: if std::ptr::eq(making, &BUILDING) {
                entries.clone()
            } else {
                Vec::new()
            },
            attack_ability: if std::ptr::eq(making, &PEASANT) {
                Some(entries[15])
            } else if std::ptr::eq(making, &GOLD_MINE) || type_id == i32::from_be_bytes(*b"htow") {
                None
            } else {
                Some(entries[at(13)])
            },
            ..UnitEntries::default()
        };
    }
}

/// Hero `h` changed its level: the parts of its state that hold the level
/// are made anew and the old ones freed 0.0001 s later (c-SAK-1: the
/// `SetHeroXP` of the map start replaced the parts each hero was made
/// with, the level-up at 30.26 s those of `SetHeroXP`).
pub(super) fn level_changed(state: &mut super::State, h: u32) {
    watches_rescheduled(state, h);
    let tick = state.table.now;
    let Some(super::Object::Unit(u)) = state.objects.get_mut(&h) else {
        return;
    };
    let old = std::mem::take(&mut u.entries.level);
    if old.is_empty() {
        return;
    }
    for e in old {
        state.table.defer(e, "hero level");
    }
    let new = state.table.take_n(tick, 2, "hero level");
    if let Some(super::Object::Unit(u)) = state.objects.get_mut(&h) {
        u.entries.level = new;
    }
}

/// The time of day is set from `old` to `new` hours (SPEC-0017): when that
/// crosses a watched hour, its event takes an object for
/// 0.0001 s, it gets 3 new watches and its old parts are freed 0.0001 s
/// later (c-SAK-1: SetTimeOfDay at the map start, the day standing
/// still, so no new rate part).
///
/// PROVISIONAL: read with the clock suspended right after; a running
/// clock may make a rate part as well.
pub(super) fn time_of_day_set(table: &mut ObjectTable, tick: u64, old: u32, new: u32) {
    // Only a change across a watched hour (dawn or dusk) makes them; else
    // the last old part goes alone (hub #14: c-SCA-1, 0.05 -> 0.0, freed
    // only 819 0.0001 s later).
    // PROVISIONAL: the watched hours inferred from two cases; whether that
    // part goes with the setting or with SuspendTimeOfDay is not read.
    let crosses = [6.0f32, 18.0]
        .iter()
        .any(|&w| (f32::from_bits(old) < w) != (f32::from_bits(new) < w));
    if !crosses {
        if let Some(e) = table.day.pop() {
            table.defer(e, "time of day");
        }
        return;
    }
    let event = table.take(tick, "event object");
    table.defer(event, "event object");
    let new = table.take_n(tick, 3, "time of day");
    for e in std::mem::replace(&mut table.day, new) {
        table.defer(e, "time of day");
    }
}

/// The entries of unit `h`, if it is one.
fn entries(state: &mut super::State, h: u32) -> Option<&mut UnitEntries> {
    match state.objects.get_mut(&h) {
        Some(super::Object::Unit(u)) => Some(&mut u.entries),
        _ => None,
    }
}

/// The order of unit `h` ends: its objects are freed 0.0001 s later.
pub(super) fn order_ended(state: &mut super::State, h: u32) {
    order_ended_with(state, h, None);
}

/// `order_ended`; with `swing_end`, an attack whose swing is past its damage
/// point: the target's object goes now, the swing's at the end of its
/// backswing (c-SBS-1: the Archmage's 1406 at the ogre's invulnerability of
/// 7.97976, 1400 .. 1405 at 8.033432; hub #6).
pub(super) fn order_ended_with(state: &mut super::State, h: u32, swing_end: Option<u32>) {
    let old = entries(state, h).map(|e| {
        e.script_attack = false;
        // The objects of its attack on a target go first, then those of
        // its acquisition and of the order (c-SAK-1: the Mountain King's
        // new order at 2501).
        e.game_order = false;
        e.own_attack = false;
        e.group_walk = false;
        e.follow_walk = false;
        let mut old: Vec<u32> = Vec::new();
        // A reaction delay still running ends with it (c-SAK-1: 2701).
        old.extend(std::mem::take(&mut e.reaction));
        old.extend(e.current.take());
        old.extend(
            std::mem::take(&mut e.target_death)
                .into_iter()
                .map(|(_, o)| o),
        );
        old.extend(std::mem::take(&mut e.acquisition));
        old.extend(std::mem::take(&mut e.order));
        old
    });
    // Those of an order lost with its target, still waiting for the
    // backswing's end, go with it (c-SBO-1: the Warden's 1430 .. 1426, 1424
    // at his new order of 10.5, in the backswing of his kill of 10.08).
    let ending = entries(state, h).map(|e| std::mem::take(&mut e.order_ending));
    let mut old = old;
    for (q, o) in ending.unwrap_or_default() {
        if let Some(k) = state
            .table
            .timed_q
            .iter()
            .position(|t| *t == (q, o, "attack: order end"))
        {
            state.table.timed_q.remove(k);
            if let Some(old) = old.as_mut() {
                old.push(o);
            }
        }
    }
    let old = old.unwrap_or_default();
    match (swing_end, old.split_first()) {
        (Some(q), Some((&first, rest))) => {
            state.table.defer(first, "order end");
            for &e in rest {
                state.table.free_at_q(q, e, "order end: swing");
            }
        }
        _ => {
            for e in old {
                state.table.defer(e, "order end");
            }
        }
    }
    // The last member of a group to end its order gives up the group's walk.
    if let Some(i) = state
        .table
        .group_walks
        .iter()
        .position(|(m, _)| m.contains(&h))
    {
        state.table.group_walks[i].0.retain(|&m| m != h);
        if state.table.group_walks[i].0.is_empty() {
            let (_, walk) = state.table.group_walks.remove(i);
            let own = entries(state, h).map(|e| std::mem::replace(&mut e.walk, walk));
            walk_dropped(state, h);
            if let (Some(e), Some(own)) = (entries(state, h), own) {
                e.walk = own;
            }
        }
    }
}

/// The objects of orders that came back after stuns whose timers end in
/// this tick go 0.0001 s later; `h`'s alone when given.
pub(super) fn order_back_freed(state: &mut super::State, h: Option<u32>) {
    let tick = state.ticks;
    let units: Vec<u32> = match h {
        Some(h) => vec![h],
        None => state.units.clone(),
    };
    for u in units {
        let due = entries(state, u).and_then(|e| {
            e.order_back.filter(|&(_, t)| t <= tick).map(|(o, _)| {
                e.order_back = None;
                o
            })
        });
        if let Some(o) = due {
            state.table.defer(o, "order back after a stun");
        }
    }
}

/// The order unit `h` gave up in this tick ends at the end of the tick:
/// its objects are freed 0.0001 s later.
pub(super) fn old_order_ended(state: &mut super::State, h: u32) {
    let old = entries(state, h).map(|e| std::mem::take(&mut e.old_order));
    for e in old.unwrap_or_default() {
        state.table.defer(e, "order end");
    }
}

/// Unit `h` takes an order of the game (c-SAK-1: a creep's return home at
/// 2565): the order and its record, the old order ends, 13 objects of the
/// move ability (the last 7 freed 0.0001 s later, from the last; the first
/// 6 when the order ends), a walk, and one more object; the order and that
/// object stay to the unit's death.
///
/// PROVISIONAL: read for a creep's return only.
pub(super) fn game_order(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    // Given before its stun's order has come back, it is the order that
    // comes back (`stuns_with`; c-SAK-1: the creep stunned by a bash, its
    // walk home back at 8952).
    if let Some(e) = entries(state, h)
        && e.stun_ticks.is_some()
    {
        e.stun_game_order = true;
        return;
    }
    let order = state.table.take(tick, "game order");
    let record = state.table.take(tick, "order record");
    // An order of the game before it goes: its extra object first, its
    // order after the old order's objects (c-SAK-1: 4017).
    let old = entries(state, h).map(|e| std::mem::take(&mut e.game_objects));
    let old = old.unwrap_or_default();
    if let Some(&extra) = old.get(1) {
        state.table.defer(extra, "game order: extra object");
    }
    state.table.defer(record, "order record");
    order_ended(state, h);
    if let Some(&k) = old.first() {
        state.table.defer(k, "game order");
    }
    // A player's unit sent home takes one fewer (c-SBV-1: footmen 6 and 7 at
    // 29.016130, 12; hub #9).
    // PROVISIONAL: which one it lacks is not read.
    let creep = state
        .unit(Some(h))
        .is_some_and(|u| u.owner == super::creeps::CREEPS);
    let n = if creep { 13 } else { 12 };
    let ability = state.table.take_n(tick, n, "move ability: order start");
    for &e in ability[6..].iter().rev() {
        state.table.defer(e, "move ability: order start");
    }
    walk_made_at_order(state, h);
    let extra = state.table.take(tick, "game order: extra object");
    if let Some(e) = entries(state, h) {
        e.order = ability[..6].iter().rev().copied().collect();
        e.game_order = true;
        e.game_objects = vec![order, extra];
    }
}

/// Unit `h`'s order of the game (a creep's walk home) ends at its point:
/// with a stun's buff end still held, one more object; then 0.0001 s
/// later the order's objects, those of the order of the game, that object
/// and the buff end (c-SAK-1: 9786, 10020).
///
/// PROVISIONAL: what the one more object is, is not read.
pub(super) fn game_order_arrived(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    // Only with a stun's buff end held (c-SAK-1: none at 10020).
    let buffed = entries(state, h).is_some_and(|e| e.buff_end.is_some());
    let other = buffed.then(|| state.table.take(tick, "game order: end"));
    order_ended(state, h);
    let (game, buff) = entries(state, h)
        .map(|e| {
            e.game_order = false;
            (std::mem::take(&mut e.game_objects), e.buff_end.take())
        })
        .unwrap_or_default();
    for e in game {
        state.table.defer(e, "game order");
    }
    if let Some(o) = other {
        state.table.defer(o, "game order: end");
    }
    if let Some(b) = buff {
        state.table.defer(b, "buff end");
    }
}

/// Unit `h`'s attack-move ends at its arrival at its point (c-SAK-1:
/// footman 3 at 57.388206): the first of its order's kept objects is
/// freed, then, for a walk that ended short of the point (`short`), the
/// move ability starts the order once more: 3 objects, freed at once
/// after it (the last first); then the rest of its order's objects. A
/// walk that reached the point takes none (c-RZJ-1: footman 6 at
/// 38.577015, 14.3 from it). Without `short`, an attack-move taken up
/// again after an attack counts as ended short.
///
/// PROVISIONAL: the other callers' rule is not read.
pub(super) fn attack_move_arrived(state: &mut super::State, h: u32, short: Option<bool>) {
    let tick = state.table.now;
    let first = entries(state, h).and_then(|e| (!e.order.is_empty()).then(|| e.order.remove(0)));
    if let Some(f) = first {
        state.table.defer(f, "order end");
    }
    // Only an attack-move taken up again after an attack (c-SAK-1: none
    // for the Blademaster's, never interrupted, at 1573).
    let resumed = entries(state, h).is_some_and(|e| std::mem::take(&mut e.resumed_move));
    let ending = if short.unwrap_or(resumed) {
        state.table.take_n(tick, 3, "attack-move: arrival")
    } else {
        Vec::new()
    };
    for &e in ending.iter().rev() {
        state.table.defer(e, "attack-move: arrival");
    }
    order_ended(state, h);
}

/// Unit `h` takes up its attack-move again after its target's death: a
/// new goal with its walk, and its last acquisition's objects are freed
/// 0.0001 s later.
pub(super) fn attack_move_resumed(state: &mut super::State, h: u32) {
    if let Some(e) = entries(state, h) {
        e.resumed_move = true;
    }
    // Done at the backswing's end already (`attack::after_swing_walks`).
    if entries(state, h).is_some_and(|e| e.walk_ready) {
        return;
    }
    walk_made_at_order(state, h);
    // At a target's death its objects go in `target_died`, in its place
    // among the attackers (c-SAK-1: 3185, 4700).
    if state.table.dying.is_some() {
        state.table.resumed.push(h);
        return;
    }
    let old = entries(state, h).map(|e| std::mem::take(&mut e.acquisition));
    for e in old.unwrap_or_default() {
        state.table.defer(e, "acquisition: attack object");
    }
}

/// Unit `h`'s move from its order queue ends and the next entry is taken
/// up, in its update: a new goal with its walk, then the "attack ends"
/// kept with the move and its last acquisition's objects are freed
/// 0.0001 s later, the latter from the last (c-SAK-1: the Blademaster's
/// attack-move after the move of his fade at 4800, ogre 18's walk home
/// after its move to footman 7's death point at 5184).
pub(super) fn queue_taken_up(state: &mut super::State, h: u32) {
    walk_made_at_order(state, h);
    let old = entries(state, h).map(|e| {
        e.current
            .take()
            .into_iter()
            .chain(std::mem::take(&mut e.acquisition))
            .collect::<Vec<_>>()
    });
    for e in old.unwrap_or_default() {
        state.table.defer(e, "acquisition: attack object");
    }
}

/// Unit `h`, whose target died in its backswing, walks back to its guard
/// point at the backswing's end: a new goal with its walk, and its last
/// acquisition's objects are freed 0.0001 s later, from the last
/// (SPEC-0030, "Losing the target in a backswing"; c-SAK-1: gnoll 17 at
/// 4824).
pub(super) fn guard_walk_made(state: &mut super::State, h: u32) {
    walk_made_at_order(state, h);
    let old = entries(state, h).map(|e| {
        e.guard_walk = true;
        std::mem::take(&mut e.acquisition)
    });
    for e in old.unwrap_or_default() {
        state.table.defer(e, "acquisition: attack object");
    }
}

/// Unit `h` takes a script's attack order to a point (c-SAK-1, 1.5 s):
/// the order and its record, the old order ends, then the 11 objects of
/// the attack ability for the order and the walk. The record and the
/// last 6 of the ability are freed 0.0001 s later (the ability's from
/// the last), the order and the first 5 of the ability when the order
/// ends (`order_ended`, the ability's from the last, then the order).
///
/// PROVISIONAL: read for script attack orders to a point only.
pub(super) fn attack_point_ordered(state: &mut super::State, h: u32) {
    attack_point_parts(state, h, 11);
}

/// A unit that cannot attack runs from its attacker by an attack order to
/// a point: as a script's, with 10 parts of its ability (c-SBJ-1: the
/// Crypt Lord at 10.489942, 1570 .. 1584).
///
/// PROVISIONAL: when the order and the 5 kept parts go is not read.
pub(super) fn flee_ordered(state: &mut super::State, h: u32) {
    attack_point_parts(state, h, 10);
}

fn attack_point_parts(state: &mut super::State, h: u32, n: usize) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script point order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let ability = state.table.take_n(tick, n, "attack ability: order start");
    for &e in ability[5..].iter().rev() {
        state.table.defer(e, "attack ability: order start");
    }
    let mut kept: Vec<u32> = ability[..5].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
    }
    walk_made_at_order(state, h);
}

/// Unit `h` takes a script's attack order on a unit (c-SBL-1 at 2.0 s): the
/// order and its record, the old order ends (an acquisition's objects of
/// that moment go), then 13 objects of the order's start; the record and
/// the last 6 (from the last) are freed 0.0001 s later, the seventh when
/// the reaction delay ends, the first 6 and the order kept with it.
///
/// PROVISIONAL: when the kept ones go is not read.
pub(super) fn attack_target_ordered(state: &mut super::State, h: u32, idle: bool) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script target order");
    // A unit let go from its pause in this tick takes the order alone now;
    // the rest comes 0.01 s later, without a record (c-SBQ-1: the ogre at
    // 2.0, its pause's objects and the order's start at 2.009998).
    // PROVISIONAL: read once.
    if state
        .unit(Some(h))
        .is_some_and(|u| u.unpaused_at == Some(state.ticks))
    {
        if let Some(e) = entries(state, h) {
            e.order = vec![order];
            e.script_attack = true;
        }
        state.table.unpause_starts.push((state.ticks + 2, h));
        return;
    }
    // An idle unit has no order to record (c-SBV-1: the Mountain King after
    // his Avatar at 4.999876, 13 parts and no record).
    // PROVISIONAL: read once.
    let idle = idle && entries(state, h).is_some_and(|e| e.order.is_empty());
    if !idle {
        let record = state.table.take(tick, "order record");
        state.table.defer(record, "order record");
    }
    order_ended(state, h);
    let parts = state.table.take_n(tick, 13, "attack: order start");
    for &e in parts[7..].iter().rev() {
        state.table.defer(e, "attack: order start");
    }
    // The seventh goes when the order is carried out at its reaction's end
    // (c-SBL-1: 1276 at 2.060097).
    let mut kept: Vec<u32> = parts[..6].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
        e.reaction.push(parts[6]);
        e.script_attack = true;
    }
}

/// The attack orders taken in a pause's last tick are carried out 0.01 s
/// later: the pause's objects go, then the start's 13 parts as at
/// `attack_target_ordered` (c-SBQ-1: the ogre at 2.009998).
pub(super) fn unpause_starts(state: &mut super::State) {
    let now = state.ticks;
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut state.table.unpause_starts)
        .into_iter()
        .partition(|&(t, _)| t <= now);
    state.table.unpause_starts = later;
    for (_, h) in due {
        let tick = state.table.now;
        let (order, paused) = entries(state, h)
            .map(|e| {
                (
                    std::mem::take(&mut e.order),
                    std::mem::take(&mut e.game_objects),
                )
            })
            .unwrap_or_default();
        for o in paused {
            state.table.defer(o, "pause: order objects");
        }
        let parts = state.table.take_n(tick, 13, "attack: order start");
        for &e in parts[7..].iter().rev() {
            state.table.defer(e, "attack: order start");
        }
        let mut kept: Vec<u32> = parts[..6].iter().rev().copied().collect();
        kept.extend(order);
        if let Some(e) = entries(state, h) {
            e.order = kept;
            e.reaction.push(parts[6]);
        }
    }
}

/// Hero `h` takes a smart order on an item (c-SBQ-1 at 2.999973): the order
/// and its record, the old order ends, ten parts of the order's start, the
/// last five freed 0.0001 s later, and a walk as for a move. The first five
/// and the order are kept to the pick-up (`item_picked`).
pub(super) fn item_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script target order");
    let record = state.table.take(tick, "order record");
    let parts = state.table.take_n(tick, 10, "item order: start");
    walk_made_at_order(state, h);
    state.table.defer(record, "order record");
    order_ended(state, h);
    for &e in parts[5..].iter().rev() {
        state.table.defer(e, "item order: start");
    }
    let mut kept: Vec<u32> = parts[..5].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
    }
}

/// Hero `h` picks up an item (c-SBQ-1 at 3.119970): the item's ability on
/// the hero, kept; the last of the order's kept parts goes before the
/// pick-up event (`item_picked_end` the rest).
pub(super) fn item_picked(state: &mut super::State, h: u32, item: u32) {
    let tick = state.table.now;
    let ability = state.table.take(tick, "item ability");
    // An item with charges a limit watch on them, kept (c-SBQ-1: the
    // potion's 1242).
    // PROVISIONAL: read for one item with one charge.
    let charged = match state.objects.get(&item) {
        Some(super::Object::Item(i)) => {
            state.data.item_uses.get(&i.type_id).is_some_and(|&u| u > 0)
        }
        _ => false,
    };
    let watch = charged.then(|| state.table.take(tick, "item: limit watch"));
    if let Some(super::Object::Item(i)) = state.objects.get_mut(&item) {
        i.held = std::iter::once(ability).chain(watch).collect();
    }
    let first = entries(state, h).and_then(|e| (!e.order.is_empty()).then(|| e.order.remove(0)));
    if let Some(o) = first {
        state.table.defer(o, "item order: start");
    }
}

/// Hero `h` drops an item: the first of its order's kept parts goes before
/// the drop event (`item_picked_end` the rest after it).
pub(super) fn item_drop_first(state: &mut super::State, h: u32) {
    let first = entries(state, h).and_then(|e| (!e.order.is_empty()).then(|| e.order.remove(0)));
    if let Some(o) = first {
        state.table.defer(o, "item order: start");
    }
}

/// After the pick-up event: the order's other kept parts and the order go,
/// and its walk is given up.
pub(super) fn item_picked_end(state: &mut super::State, h: u32) {
    let rest = entries(state, h)
        .map(|e| std::mem::take(&mut e.order))
        .unwrap_or_default();
    for o in rest {
        state.table.defer(o, "item order: start");
    }
    walk_dropped(state, h);
}

/// Hero `h` uses an item (c-SBQ-1 at 9.0): the order and its record, the old
/// order ends, nine parts, the last five freed 0.0001 s later now; the
/// first four and the order are returned, freed after the use's events
/// (`item_used_end`).
pub(super) fn item_used(state: &mut super::State, h: u32, parts: usize) -> Vec<u32> {
    let tick = state.table.now;
    let order = state.table.take(tick, "script item order");
    let record = state.table.take(tick, "order record");
    // (Nine for a use without a target, eleven with one: c-SBR-1, the salve
    // at 5.5.)
    let parts = state.table.take_n(tick, parts, "item use: order start");
    state.table.defer(record, "order record");
    order_ended(state, h);
    for &e in parts[4..].iter().rev() {
        state.table.defer(e, "item use: order start");
    }
    let mut rest: Vec<u32> = parts[..4].iter().rev().copied().collect();
    rest.push(order);
    rest
}

/// A lasting item effect (Healing Salve's AIrl) on unit `h` for `duration`
/// seconds from now: its object, kept to the effect's end; a use while it
/// runs makes none and starts the effect's time again (c-SBR-1: 1191 made
/// at the first use, freed 45 s after the third, at 60.510059).
/// PROVISIONAL: the end is taken 0.01 s after the duration; the heal over
/// time itself is not applied.
pub(super) fn item_effect(state: &mut super::State, h: u32, _item: u32, duration: u32) {
    let now = state.event_time.unwrap_or_else(|| state.timers.time());
    let end = ow3_math::add(ow3_math::add(now, duration), super::movement::AFTER_DEATH);
    // (One that has ended no more.)
    let old = entries(state, h)
        .and_then(|e| e.item_effect.take())
        .filter(|&(_, q)| f32::from_bits(q) > f32::from_bits(now));
    let o = match old {
        Some((o, q)) => {
            state.table.timed_q.retain(|t| *t != (q, o, "item effect"));
            o
        }
        None => {
            let tick = state.table.now;
            state.table.take(tick, "item effect")
        }
    };
    state.table.free_at_q(end, o, "item effect");
    if let Some(e) = entries(state, h) {
        e.item_effect = Some((o, end));
    }
}

/// After the use's events: a used-up item's ability and watch on the hero
/// and its four objects go (in the order 2, 3, 1, 0), then the order's
/// rest (c-SBQ-1: 1242, 1241, 1001, 1002, 1000, 999, then 1305 .. 1300).
pub(super) fn item_used_end(
    state: &mut super::State,
    rest: Vec<u32>,
    gone: Option<(Vec<u32>, Vec<u32>)>,
) {
    if let Some((held, objects)) = gone {
        for o in held.into_iter().rev() {
            state.table.defer(o, "item ability");
        }
        for k in [2, 3, 1, 0] {
            if let Some(&o) = objects.get(k) {
                state.table.defer(o, "item");
            }
        }
    }
    for o in rest {
        state.table.defer(o, "item use: order start");
    }
}

/// Worker `h` takes a harvest order on a gold mine (c-RXI-1 at 1.5 s): the
/// order and its record, the old order ends, then 14 parts of the order's
/// start and the walk; the record and parts 13 .. 10 (from the last) go
/// 0.0001 s later, parts 9 and 8 when the walk to the mine ends, parts
/// 7 .. 0 and the order when the order ends (the worker leaves the mine
/// with its gold).
pub(super) fn harvest_ordered(state: &mut super::State, h: u32, tree: bool) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script target order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    // On a tree 12 parts, the last 4 freed at once (c-RYP-1 at 1.5 s).
    // PROVISIONAL: the tree's kept parts' ends are taken as the mine's.
    let n = if tree { 12 } else { 14 };
    let parts = state.table.take_n(tick, n, "harvest: order start");
    for &e in parts[n - 4..].iter().rev() {
        state.table.defer(e, "harvest: order start");
    }
    if let Some(e) = entries(state, h) {
        // At the tree only its k 7 goes at the arrival (c-RYP-1: 1265).
        let kept = if tree { n - 5 } else { n - 6 };
        e.harvest_arrive = parts[kept..n - 4].iter().rev().copied().collect();
        e.order = parts[..kept].iter().rev().copied().chain([order]).collect();
    }
    walk_made_at_order(state, h);
}

/// A worker comes to a gold mine first: an object made once, kept to the
/// end (c-RXI-1: 1144 at 871, with four objects of the second pool).
/// PROVISIONAL: whether per mine or per game, and at the first arrival
/// or entry, is not separated.
pub(super) fn harvest_entered(state: &mut super::State, _h: u32) {
    let tick = state.table.now;
    if !state.table.mine_used {
        state.table.mine_used = true;
        state.table.take(tick, "gold mine: in use");
    }
}

/// Worker `h` starts to wait at a gold mine: the mine's reference to it,
/// freed when it goes in (c-RXI-1: worker 4's 1145 at 871, freed at
/// 1071; 1162 .. 1165).
pub(super) fn harvest_waits(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let reference = state.table.take(tick, "gold mine: waiting worker");
    if let Some(e) = entries(state, h) {
        e.harvest_inside = Some(reference);
    }
}

/// Worker `h`, which waited, goes into the gold mine: the reference to
/// it goes 0.0001 s later.
pub(super) fn harvest_waited(state: &mut super::State, h: u32) {
    if let Some(r) = entries(state, h).and_then(|e| e.harvest_inside.take()) {
        state.table.defer(r, "gold mine: waiting worker");
    }
}

/// A worker comes out of mine `mine` at the queue time `at` and the next
/// goes in with more still waiting (c-RXI-1: the exits at 6.349778,
/// 7.349706 and 8.349600; c-RXD-1: 21.947317 and 22.947149): the mine's
/// rate part of a unit state, made ahead of the objects of the way out, or
/// its 11 s timer started again; it is freed 0.0001 s after that timer.
///
/// PROVISIONAL: what the part's value (-1.0/s) means is not read.
pub(super) fn mine_queue_timer(state: &mut super::State, mine: u32, at: u32) {
    let end = ow3_math::add(ow3_math::add(at, 11f32.to_bits()), 0x38D1_B717);
    let running = state
        .table
        .mine_queues
        .get(&mine)
        .copied()
        .filter(|&p| state.table.timed_q.iter().any(|t| t.1 == p));
    let part = match running {
        Some(p) => {
            state.table.timed_q.retain(|t| t.1 != p);
            p
        }
        None => {
            let tick = state.table.now;
            state.table.take(tick, "unit state: rate part")
        }
    };
    state.table.mine_queues.insert(mine, part);
    state.table.free_at_q(end, part, "unit state: rate part");
}

/// Worker `h` comes out of a gold mine with its gold (c-RXI-1: worker 5 at
/// 1071): 13 parts of its order to take the gold back and its walk; then
/// its object of being inside goes, then its harvest order's kept parts
/// and order, then the new parts 12 .. 8 (from the last), all 0.0001 s
/// later; the new parts 7 .. 0 are kept with the new order.
/// PROVISIONAL: when the kept ones go (at the delivery) is not read.
pub(super) fn harvest_left(state: &mut super::State, h: u32, next: Option<u32>) {
    let tick = state.table.now;
    let parts = state.table.take_n(tick, 13, "harvest: return order start");
    walk_made_at_order(state, h);
    // The next worker's reference first, as it goes in (c-RXI-1: 1145).
    if let Some(n) = next {
        harvest_waited(state, n);
    }
    let old = entries(state, h)
        .map(|e| std::mem::take(&mut e.order))
        .unwrap_or_default();
    for o in old {
        state.table.defer(o, "harvest: order");
    }
    for &e in parts[8..].iter().rev() {
        state.table.defer(e, "harvest: return order start");
    }
    if let Some(e) = entries(state, h) {
        e.order = parts[..8].iter().rev().copied().collect();
    }
}

/// Worker `h`'s walk to the gold mine ends: two parts of its order go
/// (c-RXI-1: 1136, 1135 at 4.350023).
pub(super) fn harvest_arrived(state: &mut super::State, h: u32) {
    let parts = entries(state, h).map(|e| std::mem::take(&mut e.harvest_arrive));
    for e in parts.unwrap_or_default() {
        state.table.defer(e, "harvest: order start");
    }
}

/// Unit `h` queues an order behind its current one: one object, kept with
/// the queued order to its end (c-RTW-3: 1271 at 17.673035, freed with
/// the order at 3631).
pub(super) fn order_queued(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let o = state.table.take(tick, "order queue: entry");
    if let Some(e) = entries(state, h) {
        e.queued.push_back(o);
    }
}

/// Unit `h` starts its queued move: as a move order without its own order
/// object and record, the queued entry taking the order's place
/// (c-RTW-3 at 17.728025: 12 parts, the walk, the last 6 freed 0.0001 s
/// later, the first 6 and the entry kept to the order's end).
pub(super) fn queued_move_started(state: &mut super::State, h: u32, group: Option<&[u32]>) {
    let tick = state.table.now;
    let Some(entry) = entries(state, h).and_then(|e| e.queued.pop_front()) else {
        return;
    };
    order_ended(state, h);
    let parts = state.table.take_n(tick, 12, "move ability: order start");
    walk_made_at_order(state, h);
    // The walk replaces the one of the members that started before.
    for &m in group.unwrap_or_default() {
        if m != h && entries(state, m).is_some_and(|e| e.group_walk) {
            if let Some(e) = entries(state, m) {
                e.group_walk = false;
            }
            walk_dropped(state, m);
            if let Some(e) = entries(state, m) {
                e.group_walk = true;
            }
        }
    }
    for &e in parts[6..].iter().rev() {
        state.table.defer(e, "move ability: order start");
    }
    if let Some(e) = entries(state, h) {
        e.order = parts[..6].iter().rev().copied().chain([entry]).collect();
        // A member of a queued group walks on the walk made as it started:
        // its searches take none (c-RTW-3: 27.176439, the walk of 26.786505
        // freed at 27.206434 as the next member's replaced it).
        e.group_walk = group.is_some();
    }
}

/// Unit `h` takes a move order on a unit, following it (c-RTW-3: a right
/// click on a footman at 19.797678): the order and its record, the old
/// order ends, then 17 objects of the move ability (the last 6 freed
/// 0.0001 s later, from the last; the first 11 kept to the order's end)
/// and the walk.
pub(super) fn move_target_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "player target order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let ability = state.table.take_n(tick, 17, "move ability: order start");
    for &e in ability[11..].iter().rev() {
        state.table.defer(e, "move ability: order start");
    }
    if let Some(e) = entries(state, h) {
        e.order = ability[..11].iter().rev().copied().chain([order]).collect();
    }
    walk_made_at_order(state, h);
}

/// A script made destructable `h` (c-RYP-1: 35 trees after the hall): 7
/// objects, itself, three parts of it and three pathing parts, all kept
/// (the pathing parts to its death).
pub(super) fn destructable_made(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let parts = state.table.take_n(tick, 7, "destructable");
    state
        .table
        .destructable_paths
        .insert(h, parts[4..].to_vec());
}

/// Worker `h` reaches its tree (c-RYP-1: peasant 0 at 997): a part of its
/// harvest order goes 0.0001 s later; three objects of the harvest code
/// (the last two freed 0.0001 s later, the last first; the first at its
/// next update); then its new walk to the tree.
pub(super) fn tree_arrived(state: &mut super::State, h: u32) {
    harvest_arrived(state, h);
    let tick = state.table.now;
    let parts = state.table.take_n(tick, 3, "harvest: at the tree");
    state.table.defer(parts[2], "harvest: at the tree");
    state.table.defer(parts[1], "harvest: at the tree");
    let next = state.ticks + 6;
    if let Some(e) = entries(state, h) {
        e.tree_arrival = Some((parts[0], next));
    }
    walk_made_at_order(state, h);
}

/// Worker `h` turns back from its tree with a full load (c-RYP-1: peasant
/// 0 at 3183): the return order's object and 12 parts and the walk to the
/// hall; the harvest order's kept parts and object go, then the new parts
/// 11 .. 7 (0.0001 s later, from the last); 0 .. 6 and the object are kept
/// to the delivery.
pub(super) fn harvest_turned_back(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "harvest: return order");
    let parts = state.table.take_n(tick, 12, "harvest: return order start");
    walk_made_at_order(state, h);
    let old = entries(state, h)
        .map(|e| std::mem::take(&mut e.order))
        .unwrap_or_default();
    for o in old {
        state.table.defer(o, "harvest: order");
    }
    for &e in parts[7..].iter().rev() {
        state.table.defer(e, "harvest: return order start");
    }
    if let Some(e) = entries(state, h) {
        e.order = parts[..7].iter().rev().copied().chain([order]).collect();
    }
}

/// Worker `h` delivers its gold and goes back to its mine (c-RXI-1:
/// worker 5 at 1849): the new harvest order's object, its 15 parts, the
/// walk, then the return order's kept parts and object going, and parts
/// 10 .. 14 0.0001 s later (8 and 9 at the arrival, 0 .. 7 and the object
/// kept).
pub(super) fn gold_delivered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "harvest: order");
    let parts = state.table.take_n(tick, 15, "harvest: order start");
    walk_made_at_order(state, h);
    let old = entries(state, h)
        .map(|e| std::mem::take(&mut e.order))
        .unwrap_or_default();
    for o in old {
        state.table.defer(o, "harvest: return order");
    }
    for &e in parts[10..].iter().rev() {
        state.table.defer(e, "harvest: order start");
    }
    if let Some(e) = entries(state, h) {
        e.harvest_arrive = vec![parts[9], parts[8]];
        e.order = parts[..8].iter().rev().copied().chain([order]).collect();
    }
}

/// Worker `h` delivers its lumber and goes back to its tree (c-RYP-1:
/// peasant 0 at 3985): the new harvest order's object, the return
/// order's kept parts and object going, its 13 parts (8 .. 12 freed
/// 0.0001 s later, 7 at the arrival, 0 .. 6 and the object kept) and the
/// walk.
pub(super) fn lumber_delivered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "harvest: order");
    let old = entries(state, h)
        .map(|e| std::mem::take(&mut e.order))
        .unwrap_or_default();
    for o in old {
        state.table.defer(o, "harvest: return order");
    }
    let parts = state.table.take_n(tick, 13, "harvest: order start");
    for &e in parts[8..].iter().rev() {
        state.table.defer(e, "harvest: order start");
    }
    if let Some(e) = entries(state, h) {
        e.harvest_arrive = vec![parts[7]];
        e.order = parts[..7].iter().rev().copied().chain([order]).collect();
    }
    walk_made_at_order(state, h);
}

/// Worker `h` drops a tree it cannot reach (c-RYP-1: peasant 4 at 1243):
/// its harvest order's part 7 goes, a stop's 3 parts are made and freed
/// at once (0.0001 s later, from the last), then its part 6, and an object
/// of the wait before another tree is looked for, kept to then.
pub(super) fn tree_dropped(state: &mut super::State, h: u32) {
    harvest_arrived(state, h);
    let tick = state.table.now;
    let stop = state.table.take_n(tick, 3, "harvest: stop");
    for &e in stop.iter().rev() {
        state.table.defer(e, "harvest: stop");
    }
    let six = entries(state, h).and_then(|e| (!e.order.is_empty()).then(|| e.order.remove(0)));
    if let Some(o) = six {
        state.table.defer(o, "harvest: order start");
    }
    let wait = state.table.take(tick, "harvest: wait for a tree");
    if let Some(e) = entries(state, h) {
        e.tree_wait = Some(wait);
    }
}

/// Worker `h` takes another tree after its wait (c-RYP-1: peasant 4 at
/// 1343): the wait's object goes, the new order's object is made, the old
/// order's kept parts go, then its 12 parts (8 .. 11 freed 0.0001 s
/// later, 7 at the arrival, 0 .. 6 and the object kept) and the walk.
pub(super) fn tree_switched(state: &mut super::State, h: u32) {
    let wait = entries(state, h).and_then(|e| e.tree_wait.take());
    if let Some(o) = wait {
        state.table.defer(o, "harvest: wait for a tree");
    }
    let tick = state.table.now;
    let order = state.table.take(tick, "harvest: order");
    let old = entries(state, h)
        .map(|e| std::mem::take(&mut e.order))
        .unwrap_or_default();
    for o in old {
        state.table.defer(o, "harvest: order");
    }
    let parts = state.table.take_n(tick, 12, "harvest: order start");
    for &e in parts[8..].iter().rev() {
        state.table.defer(e, "harvest: order start");
    }
    if let Some(e) = entries(state, h) {
        e.harvest_arrive = vec![parts[7]];
        e.order = parts[..7].iter().rev().copied().chain([order]).collect();
    }
    walk_made_at_order(state, h);
}

/// Worker `h`'s update after reaching its tree: its arrival's kept object
/// goes (c-RYP-1: 1358 at 1003, after the old walk).
pub(super) fn tree_arrival_ended(state: &mut super::State, h: u32) {
    let tick = state.ticks;
    let due =
        entries(state, h).and_then(|e| e.tree_arrival.filter(|&(_, t)| t <= tick).map(|(o, _)| o));
    if let Some(o) = due {
        if let Some(e) = entries(state, h) {
            e.tree_arrival = None;
        }
        state.table.defer(o, "harvest: at the tree");
    }
}

/// Unit `h`'s follow leg ends at its update after it stopped in the follow
/// range (c-RTW-3: footman (825, 828) at 21.419670): 4 of the order's
/// kept parts go 0.0001 s later and a new walk is made at once.
pub(super) fn follow_leg_ended(state: &mut super::State, h: u32) {
    let tick = state.ticks;
    let due = entries(state, h).is_some_and(|e| e.follow_leg_end.is_some_and(|t| t <= tick));
    if !due {
        return;
    }
    let parts: Vec<u32> = entries(state, h)
        .map(|e| {
            e.follow_leg_end = None;
            let n = e.order.len().saturating_sub(1).min(4);
            e.order.drain(..n).collect()
        })
        .unwrap_or_default();
    for p in parts {
        state.table.defer(p, "move ability: order start");
    }
    walk_made_at_order(state, h);
    if let Some(e) = entries(state, h) {
        e.follow_walk = true;
    }
}

/// Unit `h` takes a patrol order (c-RTW-3: the player's at 13.523731): an
/// order object, its record and one more, all three freed 0.0001 s
/// later (the record, the third, the first), the old order ends, then 12
/// objects of the patrol's start (the last 5 freed 0.0001 s later, from
/// the last; the first 7 kept to the order's end) and the walk.
pub(super) fn patrol_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let first = state.table.take(tick, "patrol order");
    let record = state.table.take(tick, "order record");
    let third = state.table.take(tick, "patrol order");
    state.table.defer(record, "order record");
    state.table.defer(third, "patrol order");
    state.table.defer(first, "patrol order");
    order_ended(state, h);
    let parts = state.table.take_n(tick, 12, "patrol: order start");
    for &e in parts[7..].iter().rev() {
        state.table.defer(e, "patrol: order start");
    }
    if let Some(e) = entries(state, h) {
        e.order = parts[..7].iter().rev().copied().collect();
    }
    walk_made_at_order(state, h);
}

/// Unit `h` takes a script's move order to a point (c-SBG-1, ogre 3 at
/// 1.5 s): as an attack order to a point, with 13 objects of the move
/// ability, the last 7 freed 0.0001 s later and the first 6 kept to the
/// order's end.
///
/// PROVISIONAL: read for a creep that had acquired a target.
pub(super) fn move_point_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script point order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    // 13 for a unit that had taken a target, 12 for one that had not
    // (c-SBG-1: ogre 3; c-RTW-3: footman (825, 828) at 10.924168, the
    // first 6 kept either way).
    let targeted =
        entries(state, h).is_some_and(|e| !e.acquisition.is_empty() || e.current.is_some());
    order_ended(state, h);
    let n = if targeted { 13 } else { 12 };
    let ability = state.table.take_n(tick, n, "move ability: order start");
    for &e in ability[6..].iter().rev() {
        state.table.defer(e, "move ability: order start");
    }
    let mut kept: Vec<u32> = ability[..6].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
    }
    walk_made_at_order(state, h);
}

/// The members `units` take a group's move or right click on the ground
/// (c-RTW-3: 7 footmen at 24.021969): two search helpers, then each
/// member's order as [`move_point_ordered`] without its own walk, then one
/// walk for the group; the helpers are freed at the end.
///
/// The group's walk goes with the last member's order (c-RTW-3: freed at
/// 27.686354, the update after the last member started its queued move).
pub(super) fn group_move_ordered(state: &mut super::State, units: &[u32]) {
    let tick = state.table.now;
    let helpers = state.table.take_n(tick, 2, "search: helper");
    for &h in units {
        let order = state.table.take(tick, "script point order");
        let record = state.table.take(tick, "order record");
        state.table.defer(record, "order record");
        let targeted =
            entries(state, h).is_some_and(|e| !e.acquisition.is_empty() || e.current.is_some());
        order_ended(state, h);
        let n = if targeted { 13 } else { 12 };
        let ability = state.table.take_n(tick, n, "move ability: order start");
        for &e in ability[6..].iter().rev() {
            state.table.defer(e, "move ability: order start");
        }
        let mut kept: Vec<u32> = ability[..6].iter().rev().copied().collect();
        kept.push(order);
        if let Some(e) = entries(state, h) {
            e.order = kept;
            e.group_walk = true;
        }
    }
    let cluster = state.table.take(tick, "walk: cluster");
    let path = state.table.take(tick, "walk: path");
    state
        .table
        .group_walks
        .push((units.to_vec(), vec![path, cluster]));
    for &e in &helpers {
        state.table.free(tick, e, "search: helper");
    }
}

/// Unit `h` takes a script's order of a spell on a point (c-RZJ-1: the
/// Archmage's Blizzards of 2.5 and 9.5): the order and its record, the
/// old order ends, then the ability's objects: walking into range 11 (the
/// last 6 freed 0.0001 s later, the fifth when the cast starts, the first
/// 4 kept with the order) and the walk; cast at once 10, without the one
/// of the cast's start and without a walk.
///
/// PROVISIONAL: when the kept ones go is not read.
pub(super) fn spell_point_ordered(state: &mut super::State, h: u32, at_once: bool) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script point order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let n = if at_once { 10 } else { 11 };
    let ability = state
        .table
        .take_n(tick, n, "spell ability: point order start");
    for &e in ability[n - 6..].iter().rev() {
        state.table.defer(e, "spell ability: point order start");
    }
    let mut kept: Vec<u32> = ability[..4].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
        if !at_once {
            e.cast_start = vec![ability[4]];
        }
    }
    if !at_once {
        walk_made_at_order(state, h);
    }
}

/// A creep's spell ordered by its spell timer (SPEC-0047; c-RZJ-1: the
/// Ogre Lord's Shockwave at 3.177711): as a script's spell order, without
/// a walk; the kept objects go with the cast's end (4.249930).
pub(super) fn creep_spell_ordered(state: &mut super::State, h: u32, turned: bool) {
    if turned {
        spell_order_objects(state, h, "creep spell order");
        return;
    }
    // Not facing its target, as a target spell walking into range: 12,
    // the fifth at the cast's start, the sixth at the order's end, and a
    // walk (c-RZJ-1: the Ogre Lord's second, 12.110987).
    let tick = state.table.now;
    let order = state.table.take(tick, "creep spell order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let ability = state.table.take_n(tick, 12, "spell ability: order start");
    for &e in ability[6..].iter().rev() {
        state.table.defer(e, "spell ability: order start");
    }
    let mut kept: Vec<u32> = ability[..4].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
        e.cast_start = vec![ability[4]];
        e.impact = vec![ability[5]];
    }
    walk_made_at_order(state, h);
}

fn spell_order_objects(state: &mut super::State, h: u32, tag: &'static str) {
    let tick = state.table.now;
    let order = state.table.take(tick, tag);
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let ability = state
        .table
        .take_n(tick, 11, "spell ability: point order start");
    for &e in ability[5..].iter().rev() {
        state.table.defer(e, "spell ability: point order start");
    }
    let mut kept: Vec<u32> = ability[..5].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
    }
}

/// Unit `h` gets a new walk (an event `cluster` of the path lists): a
/// search helper freed at once, the cluster and the path. The walk it
/// had is given up (`walk_dropped`).
pub(super) fn walk_made(state: &mut super::State, h: u32) {
    walk_dropped(state, h);
    let tick = state.table.now;
    let helper = state.table.take(tick, "search: helper");
    let cluster = state.table.take(tick, "walk: cluster");
    let path = state.table.take(tick, "walk: path");
    state.table.free(tick, helper, "search: helper");
    if let Some(e) = entries(state, h) {
        e.walk = vec![path, cluster];
        // A goal in reach after a swing is that walk's alone (c-SAK-1: the
        // Mountain King's attack-move walk of 10000 kept through his stop
        // at 10557).
        e.quiet_walk = false;
    }
    // The objects kept to the end of a backswing after its target's death
    // go with a walk it takes instead of standing (c-RZH-1: creep 13 at
    // 5142, after footman 2's death at 5116).
    let lost = entries(state, h)
        .map(|e| std::mem::take(&mut e.lost_later))
        .unwrap_or_default();
    for e in lost {
        state.table.defer(e, "acquisition: attack object");
    }
}
/// Unit `h` made a walk at its order: its first search takes none.
pub(super) fn walk_made_at_order(state: &mut super::State, h: u32) {
    walk_made(state, h);
    if let Some(e) = entries(state, h) {
        e.walk_ready = true;
    }
}

/// Unit `h` searches a new path: a new walk, unless its order made one.
pub(super) fn path_searched(state: &mut super::State, h: u32) {
    match entries(state, h) {
        Some(e) if e.group_walk => {}
        // A follower keeps the walk of its leg's end through its later legs
        // (c-RTW-3: 828's walk of 21.417406 kept to 91.438118).
        Some(e) if e.follow_walk => {}
        Some(e) if e.walk_ready => e.walk_ready = false,
        Some(_) => walk_made(state, h),
        None => {}
    }
}

/// Unit `h` stands in reach of its target, facing it: the walk of its
/// chase ends (c-SAK-1: the image 20 at 48.718559).
pub(super) fn chase_walk_ended(state: &mut super::State, h: u32) {
    if entries(state, h).is_some_and(|e| !e.walk.is_empty()) {
        walk_dropped(state, h);
    }
}

/// `walk_dropped` at the unit's update of this tick, whenever it is
/// handled: freed at its next update (c-RXI-1: workers going in or
/// starting to wait at 871, their walks freed at 877).
pub(super) fn walk_dropped_at_update(state: &mut super::State, h: u32) {
    let started = std::mem::replace(&mut state.updates_started, state.ticks);
    walk_dropped(state, h);
    state.updates_started = started;
}

/// Unit `h`'s walk stops or is replaced: its path and cluster are freed
/// at its first update after this tick (SPEC-0024, "Walks").
pub(super) fn walk_dropped(state: &mut super::State, h: u32) {
    // The walk of a follow leg or of a group stays to the order's end.
    if entries(state, h).is_some_and(|e| e.follow_walk || e.group_walk) {
        return;
    }
    // A walk made early for a chase to come stays through its order.
    // Not once an update of an earlier tick came after it: the walk was
    // given up there (c-RZH-1: the Mountain King's walk of 5573, update
    // 5574, his goal in 5575, freed at the update after, 5580; one in the
    // goal's own tick keeps it, c-SAK-1: 6955).
    let (last_update, now) = (state.last_update_tick, state.ticks);
    if let Some(e) = entries(state, h)
        && std::mem::take(&mut e.early_walk)
        && (last_update <= e.early_walk_at || last_update == now)
    {
        return;
    }
    // Given up before the updates of the tick, the unit's update in this
    // tick frees it (c-SAK-1: the Blademaster's fade at 4620, the Mountain
    // King's check at 6234).
    let before_update = state.updates_started != state.ticks
        && state
            .unit(Some(h))
            .and_then(|u| u.movement)
            .is_some_and(|m| m.next_update <= state.ticks);
    let tick = if !state.table.at_fade && !before_update {
        state.table.now
    } else {
        state.table.now.saturating_sub(1)
    };
    // The unit's next update: walks given up by a unit that stands then
    // still go at it (c-SAK-1: the Blademaster at his point, 1573 -> 1579).
    let next = state
        .unit(Some(h))
        .and_then(|u| u.movement)
        .map(|m| m.next_update);
    if let Some(e) = entries(state, h) {
        let old = std::mem::take(&mut e.walk);
        if !old.is_empty() {
            // Dropped in its update, the stored next update is still this
            // one: the next comes 6 ticks on (SPEC-0018).
            e.stale_update = next.map(|n| if n > tick { n } else { tick + 6 });
        }
        e.stale.extend(old.into_iter().map(|w| (tick, w)));
        e.walk_ready = false;
    }
}

/// Unit `h` updates: the walks it gave up in earlier ticks are freed.
pub(super) fn unit_updated(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let due = entries(state, h).map(|e| {
        let (due, kept) = std::mem::take(&mut e.stale)
            .into_iter()
            .partition::<Vec<_>, _>(|&(t, _)| t < tick);
        e.stale = kept;
        due
    });
    let due: Vec<u32> = due
        .unwrap_or_default()
        .into_iter()
        .map(|(_, e)| e)
        .collect();
    for walk in due.chunks(2) {
        state.table.walks_due.push(walk.to_vec());
    }
}

/// The walks the units gave up are freed after the updates of the tick,
/// the newest walk first, each path before its cluster (c-SAK-1: 709,
/// 931, 1027).
pub(super) fn walks_freed(state: &mut super::State) {
    let tick = state.table.now;
    // Units that stand: their walks go at the update they would have had.
    let standing: Vec<u32> = state
        .units
        .iter()
        .copied()
        .filter(|&h| state.unit(Some(h)).is_some_and(|u| u.movement.is_none()))
        .collect();
    for h in standing {
        let due = entries(state, h).and_then(|e| {
            (e.stale_update.is_some_and(|n| n <= tick) && !e.stale.is_empty()).then(|| {
                e.stale_update = None;
                std::mem::take(&mut e.stale)
            })
        });
        let due: Vec<u32> = due
            .unwrap_or_default()
            .into_iter()
            .map(|(_, e)| e)
            .collect();
        for walk in due.chunks(2) {
            state.table.walks_due.push(walk.to_vec());
        }
    }
    let mut due = std::mem::take(&mut state.table.walks_due);
    due.sort_by_key(|w| std::cmp::Reverse(state.table.creation_of(w[0])));
    for walk in due {
        for e in walk {
            state.table.free(tick, e, "walk");
        }
    }
}

/// The orders of the game that ended at their points in the updates,
/// after the walks freed there (`game_order_arrived`).
pub(super) fn homes_arrived(state: &mut super::State) {
    for h in std::mem::take(&mut state.table.homes) {
        game_order_arrived(state, h);
    }
}

/// Unit `h` takes a script's order of a spell on a target (c-SAK-1: the
/// Storm Bolt at 2.5 s): the order and its record, the old order ends,
/// then 12 objects of the ability, and a walk when it has to walk into
/// range. The record and the ability's last 6 are freed 0.0001 s later
/// (from the last), the ability's fifth when the cast starts, its first
/// four and the order when the order ends.
///
/// PROVISIONAL: read for Storm Bolt only; when the ability's sixth goes
/// is not read.
pub(super) fn spell_ordered(state: &mut super::State, h: u32, walks: bool) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script target order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    // In range and turned, the cast starts at once: 11 objects, the last 6
    // freed 0.0001 s later, the first 5 and the order at the order's end
    // (c-SAK-1: the Storm Bolt of 4400).
    if !walks && state.table.cast_at_once {
        state.table.cast_at_once = false;
        let ability = state.table.take_n(tick, 11, "spell ability: order start");
        for &e in ability[5..].iter().rev() {
            state.table.defer(e, "spell ability: order start");
        }
        let mut kept: Vec<u32> = ability[..5].iter().rev().copied().collect();
        kept.push(order);
        if let Some(e) = entries(state, h) {
            e.order = kept;
        }
        return;
    }
    let ability = state.table.take_n(tick, 12, "spell ability: order start");
    for &e in ability[6..].iter().rev() {
        state.table.defer(e, "spell ability: order start");
    }
    let mut kept: Vec<u32> = ability[..4].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
        e.cast_start = vec![ability[4]];
        e.impact = vec![ability[5]];
    }
    if walks {
        walk_made_at_order(state, h);
    }
}

/// Unit `h` gains the buff of aura `ability`: its object, kept while it
/// has the buff (c-RZJ-1 at 1.505). The bearer's own buff has its object
/// from the aura's learning or the unit's making.
///
/// PROVISIONAL: the order of the gains in a check is the order of the
/// bearers, then of the units.
pub(super) fn aura_buff_gained(state: &mut super::State, h: u32, ability: i32) {
    let tick = state.table.now;
    let own = state.unit(Some(h)).is_some_and(|u| {
        u.skills.iter().any(|&(a, _)| a == ability)
            || state
                .data
                .units
                .get(&u.type_id)
                .is_some_and(|t| t.abilities.contains(&ability))
    });
    if own {
        return;
    }
    let o = state.table.take(tick, "aura buff");
    if let Some(e) = entries(state, h) {
        e.aura_buffs.push((ability, o));
    }
    // Brilliance's buff makes a rate part for its mana regeneration
    // (c-RZJ-1: 1444 after the Paladin's buff 1443 at 1.505), Unholy
    // Aura's for its life's (c-SBN-1: 1527 at 12.189039).
    // PROVISIONAL: kept; what frees it is not read.
    let brilliance = state
        .data
        .abilities
        .get(&ability)
        .is_some_and(|d| [*b"AHab", *b"AUau"].contains(&d.code.to_be_bytes()));
    if brilliance {
        state.table.take(tick, "aura buff: rate part");
    }
}

/// The creation number of unit `h`'s buff of aura `ability`.
pub(super) fn aura_buff_creation(state: &super::State, h: u32, ability: i32) -> u32 {
    state
        .unit(Some(h))
        .and_then(|u| u.entries.aura_buffs.iter().find(|b| b.0 == ability))
        .map_or(u32::MAX, |b| state.table.creation_of(b.1))
}

/// Unit `h` loses the buff of aura `ability`: its object goes 0.0001 s
/// later.
pub(super) fn aura_buff_lost(state: &mut super::State, h: u32, ability: i32) {
    let lost = entries(state, h).and_then(|e| {
        let k = e.aura_buffs.iter().position(|b| b.0 == ability)?;
        Some(e.aura_buffs.remove(k).1)
    });
    if let Some(o) = lost {
        state.table.defer(o, "aura buff");
    }
}

/// Unit `h`'s attack ability is removed (`UnitRemoveAbility` 'Aatk'): its
/// object, made with the unit, is freed 0.0001 s later (c-SBG-1: the
/// Mountain King's 13).
pub(super) fn attack_removed(state: &mut super::State, h: u32) {
    if let Some(e) = entries(state, h).and_then(|e| e.attack_ability.take()) {
        state.table.defer(e, "attack ability");
    }
}

/// Unit `h` takes a script's `holdposition` (c-SBG-1, the map start): the
/// order and its record, the old order ends, then 5 objects of the hold's
/// start; all freed 0.0001 s later, the record first, then the 5 (the
/// last first), then the order.
///
/// PROVISIONAL: read for units whose order before was their start order.
pub(super) fn hold_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script immediate order");
    let record = state.table.take(tick, "order record");
    // A builder leaving its building: the building's rate row and its
    // work's two timers first, its work's parts and order after the record
    // (c-SBZ-1: peasant 0's stop at 19.497728, hub #13).
    let work = entries(state, h)
        .map(|e| std::mem::take(&mut e.work))
        .unwrap_or_default();
    if work.len() == 7 {
        if let Some(b) = state.building_left.take()
            && let Some(o) = entries(state, b).and_then(|e| e.construction_rates.pop())
        {
            state.table.defer(o, "unit state: rate part");
        }
        state.table.defer(work[6], "work order: timer");
        state.table.defer(work[5], "work order: timer");
    }
    state.table.defer(record, "order record");
    if work.len() == 7 {
        for &e in work[..5].iter().rev() {
            state.table.defer(e, "work order");
        }
    }
    order_ended(state, h);
    // A group member's walk, kept through its order, goes now (c-RTW-3:
    // the stop of 27.921314, the last member's walk freed at its update).
    walk_dropped(state, h);
    let start = state.table.take_n(tick, 5, "hold: order start");
    for &e in start.iter().rev() {
        state.table.defer(e, "hold: order start");
    }
    state.table.defer(order, "script immediate order");
}

/// Unit `h` takes a script's order of a spell without a target (c-SAK-1:
/// the Mirror Image at 7101): the order and its record, the old order
/// ends, then 9 objects of the ability. The record and the ability's last
/// 5 are freed 0.0001 s later (from the last), its first 4 and the order
/// when the order ends.
///
/// PROVISIONAL: when the kept ones go is not read.
pub(super) fn immediate_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script immediate order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let ability = state.table.take_n(tick, 9, "spell ability: order start");
    for &e in ability[4..].iter().rev() {
        state.table.defer(e, "spell ability: order start");
    }
    let mut kept: Vec<u32> = ability[..4].iter().rev().copied().collect();
    kept.push(order);
    if let Some(e) = entries(state, h) {
        e.order = kept;
    }
}

/// Unit `h` takes Divine Shield, an order without a target and without a
/// cast time (c-RZJ-1: the Paladin at 24.0): the order and its record;
/// the object its old order kept for an impact goes first, then the
/// record and the old order's objects. The order is given back, freed
/// 0.0001 s later after the effect.
pub(super) fn divine_shield_ordered(state: &mut super::State, h: u32) -> u32 {
    let tick = state.table.now;
    let order = state.table.take(tick, "script immediate order");
    let record = state.table.take(tick, "order record");
    impact_unused(state, h);
    state.table.defer(record, "order record");
    order_ended(state, h);
    order
}

/// Unit `h`'s Divine Shield takes effect at queue time `at` for `seconds`:
/// the effect's event, then the buff's object, freed at the buff's end
/// (c-RZJ-1: 3680, 23.996973 to 38.996883).
pub(super) fn divine_shield_effect(state: &mut super::State, h: u32, at: u32, seconds: u32) {
    spell_effect_event(state, h);
    let tick = state.table.now;
    let buff = state.table.take(tick, "Divine Shield: buff");
    state
        .table
        .free_at_q(ow3_math::add(at, seconds), buff, "Divine Shield: buff");
}

/// Unit `h` is ordered the spell it is casting again before its effect:
/// the order is taken and kept.
///
/// PROVISIONAL: when it is freed is not read (not by 60 s in c-SAK-1).
pub(super) fn spell_ordered_again(state: &mut super::State) {
    let tick = state.table.now;
    state.table.take(tick, "script target order");
}

/// Unit `h`'s cast ends with the object kept for an impact unused (a
/// target spell without a missile): freed 0.0001 s later, before the
/// order's.
pub(super) fn impact_unused(state: &mut super::State, h: u32) {
    let kept = entries(state, h).map(|e| std::mem::take(&mut e.impact));
    for e in kept.unwrap_or_default() {
        state.table.defer(e, "spell ability: order start");
    }
}

/// Unit `h` starts a cast: the object of its order kept to then is freed.
pub(super) fn cast_started(state: &mut super::State, h: u32) {
    // At the tick's end (c-RZJ-1: the Ogre Lord's 2465 at 12.153961,
    // after the walks freed at its update).
    let start = entries(state, h).map(|e| std::mem::take(&mut e.cast_start));
    for e in start.unwrap_or_default() {
        state.table.defer(e, "spell ability: order start");
    }
    // The walk into range ends.
    walk_dropped(state, h);
}

/// Unit `h` takes a target at an acquisition (SPEC-0024, "Acquisition";
/// c-SAK-1 from 3.51 s), after the event's objects:
/// - on attack-move, 7: the last two freed 0.0001 s later (the last
///   first), the fifth at the end of the reaction delay, the fourth at the
///   target's death, the first three at the unit's next acquisition; the
///   attack-move goes on;
/// - without an order, 10: an attack order of its own, the last two freed
///   0.0001 s later, the eighth at the end of the reaction delay, the
///   others with the order;
/// - with an attack order of its own already, 2 and a new walk at once:
///   the first freed at the new target's death, the second 0.0001 s
///   later (c-SAK-1: gnoll 12 at 908).
pub(super) fn acquired(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let at = state.table.next_creation();
    if let Some(e) = entries(state, h) {
        e.taken_at = at;
        e.by_call = false;
    }
    let target = state.unit(Some(h)).and_then(|u| {
        u.movement.and_then(|m| match m.order {
            super::movement::Order::Attack { target } => Some(target),
            _ => None,
        })
    });
    if entries(state, h).is_some_and(|e| e.own_attack) {
        // The object of the old target goes 0.0001 s later; those kept to
        // the target's death follow the new one (c-SAK-1: gnoll 12, 908
        // and 4764).
        // A reaction delay still running goes first (c-RZJ-1: 2903, then
        // the old target's 2902, at 15.708464).
        let old = entries(state, h).map(|e| {
            let mut old = std::mem::take(&mut e.reaction);
            old.extend(e.current.take());
            old
        });
        for c in old.unwrap_or_default() {
            state.table.defer(c, "acquisition: attack object");
        }
        let a = state.table.take_n(tick, 2, "acquisition: extra object");
        state.table.defer(a[1], "acquisition: extra object");
        if let (Some(e), Some(t)) = (entries(state, h), target) {
            // The first is the new target's object (c-SAK-1: freed at the
            // next change at 1127, at the target's death at 4764).
            e.current = Some(a[0]);
            for d in &mut e.target_death {
                d.0 = t;
            }
        }
        walk_made_at_order(state, h);
        return;
    }
    // On attack-move, still attacking an acquired target: the object kept
    // to that target's death goes 0.0001 s later, and the same 2 and walk
    // (c-SAK-1: footman 7 answering a call at 965).
    // So does a creep that took its target walking back to its guard
    // point (c-RZJ-1: the kobold of 24.186941 again at 25.686689).
    let attacking = entries(state, h).is_some_and(|e| !e.target_death.is_empty());
    if attacking {
        // A reaction delay still running goes first (c-SAK-1: ogre 18 at
        // 2927).
        let old = entries(state, h).map(|e| {
            let mut old = std::mem::take(&mut e.reaction);
            old.extend(
                std::mem::take(&mut e.target_death)
                    .into_iter()
                    .map(|(_, o)| o),
            );
            old
        });
        for o in old.unwrap_or_default() {
            state.table.defer(o, "acquisition: attack object");
        }
        let a = state.table.take_n(tick, 2, "acquisition: extra object");
        state.table.defer(a[1], "acquisition: extra object");
        if let (Some(e), Some(t)) = (entries(state, h), target) {
            e.target_death = vec![(t, a[0])];
        }
        walk_made_at_order(state, h);
        return;
    }
    // An engaged unit's answer to a hit lets go only the first of its last
    // acquisition's (c-SCA-1: the turtle's 1147 at 9.35943, its 1145 and
    // 1146 kept).
    // PROVISIONAL: when the kept two go is not read.
    if state.table.answering && entries(state, h).is_some_and(|e| e.engaged) {
        let first = entries(state, h)
            .and_then(|e| (!e.acquisition.is_empty()).then(|| e.acquisition.remove(0)));
        if let Some(o) = first {
            state.table.defer(o, "acquisition: attack object");
        }
    }
    let answering = state.table.answering && entries(state, h).is_some_and(|e| e.engaged);
    if !answering {
        let old = entries(state, h).map(|e| std::mem::take(&mut e.acquisition));
        for e in old.unwrap_or_default() {
            state.table.defer(e, "acquisition: attack object");
        }
    }
    // A unit without any order makes an attack order of its own; one on
    // attack-move or with an order of the game (a creep going home) not
    // (c-SAK-1: ogre 18 at 2915).
    let own = state.unit(Some(h)).is_some_and(|u| {
        u.attack_move.is_none()
                && !u.entries.game_order
                // Nor one walking back to its guard point after its target's
                // death (c-SAK-1: gnoll 12 at 4766).
                && !u.entries.guard_walk
    });
    if let Some(e) = entries(state, h) {
        e.guard_walk = false;
    }
    // One already engaged (a wake's attack-move): four, the fourth freed
    // 0.0001 s later, and a chase walk when its target is out of reach
    // (c-SCA-1: the turtle at 7.009730, hub #14).
    // PROVISIONAL: only a wake marks a unit engaged; when it goes back to
    // idle is not read.
    // Its answer to a hit: two and its walk (c-SCA-1: the turtle answering
    // the archer at 9.35943, hub #14).
    if !own && state.table.answering && entries(state, h).is_some_and(|e| e.engaged) {
        let a = state.table.take_n(tick, 2, "acquisition: extra object");
        state.table.defer(a[1], "acquisition: extra object");
        if let Some(e) = entries(state, h) {
            e.acquisition.push(a[0]);
        }
        walk_made_at_order(state, h);
        if let Some(e) = entries(state, h) {
            e.walk_at_acquire = true;
        }
        return;
    }
    if !own && entries(state, h).is_some_and(|e| e.engaged) {
        let a = state.table.take_n(tick, 4, "acquisition: attack object");
        state.table.defer(a[3], "acquisition: attack object");
        if let Some(e) = entries(state, h) {
            e.acquisition = vec![a[2], a[1], a[0]];
        }
        if target.is_some_and(|t| !super::movement::reaches(state, h, t)) {
            walk_made_at_order(state, h);
            if let Some(e) = entries(state, h) {
                e.walk_at_acquire = true;
            }
        }
        return;
    }
    let n = if own { 10 } else { 7 };
    let a = state.table.take_n(tick, n, "acquisition: attack object");
    state.table.defer(a[n - 1], "acquisition: attack object");
    state.table.defer(a[n - 2], "acquisition: attack object");
    if own {
        // Its old order ends at the end of the tick (c-SAK-1: kobold 10's
        // order back after its stun, freed after the hit of 1745).
        let old = entries(state, h).map(|e| {
            let mut old: Vec<u32> = Vec::new();
            if std::mem::take(&mut e.own_attack) {
                old.extend(e.current.take());
                old.extend(
                    std::mem::take(&mut e.target_death)
                        .into_iter()
                        .map(|(_, o)| o),
                );
            }
            old.extend(std::mem::take(&mut e.order));
            old
        });
        if let Some(e) = entries(state, h) {
            e.old_order = old.unwrap_or_default();
        }
        state.table.ends_late.push(h);
    }
    if let Some(e) = entries(state, h) {
        e.reaction = vec![a[n - 3]];
        if own {
            // The first three go with the order, the next three at the
            // target's death, the seventh when the target changes.
            e.order = vec![a[2], a[1], a[0]];
            e.current = Some(a[6]);
            if let Some(t) = target {
                e.target_death = vec![(t, a[5]), (t, a[4]), (t, a[3])];
            }
            e.own_attack = true;
        } else {
            e.acquisition = vec![a[2], a[1], a[0]];
            if let Some(t) = target {
                e.target_death.push((t, a[n - 4]));
            }
        }
    }
}

/// Unit `h`'s reaction delay ends: its acquisition's object kept to then
/// is freed 0.0001 s later.
pub(super) fn reaction_ended(state: &mut super::State, h: u32) {
    let reaction = entries(state, h).map(|e| std::mem::take(&mut e.reaction));
    for e in reaction.unwrap_or_default() {
        state.table.defer(e, "acquisition: attack object");
    }
}

/// Unit `dead` died: the acquisitions that took it free their objects
/// kept to its death.
pub(super) fn target_died(state: &mut super::State, dead: u32) {
    // The attackers in the order they took their targets, each one's
    // objects from the last (c-SAK-1: footman 2 before the Mountain King
    // at 4700).
    let mut units = state.units.clone();
    units.sort_by_key(|&h| state.unit(Some(h)).map_or(u32::MAX, |u| u.entries.taken_at));
    let resumed = std::mem::take(&mut state.table.resumed);
    for h in units {
        let goes_on = resumed.contains(&h);
        let held = entries(state, h).map(|e| {
            let (gone, kept) = std::mem::take(&mut e.target_death)
                .into_iter()
                .partition::<Vec<_>, _>(|&(t, _)| t == dead);
            e.target_death = kept;
            // The object of the current target goes first.
            let current = if gone.is_empty() {
                None
            } else {
                e.current.take()
            };
            // One going on with its attack-move frees its acquisition too.
            let acquisition = if goes_on {
                std::mem::take(&mut e.acquisition)
            } else {
                Vec::new()
            };
            current
                .into_iter()
                .chain(gone.into_iter().map(|(_, o)| o))
                .chain(acquisition)
                .collect::<Vec<_>>()
        });
        for o in held.unwrap_or_default() {
            state.table.defer(o, "acquisition: attack object");
        }
    }
    state.table.dying = None;
}

/// `EVENT_UNIT_ACQUIRED_TARGET` of common.j.
const EVENT_UNIT_ACQUIRED_TARGET: i32 = 60;

/// Unit `h` answers a call (SPEC-0019): its acquired-target event's
/// objects and those of the attack are taken at the call, before the hit
/// that made it ends (c-SAK-1: footman 7 at 965); its triggers run later
/// without objects (`widgets::acquired_at_call`).
pub(super) fn call_answered(state: &mut super::State, h: u32) {
    let registrations = state
        .events
        .iter()
        .filter(|r| {
            matches!(**r, super::widgets::Registration::Unit { unit, event, .. }
                if unit == h && event == EVENT_UNIT_ACQUIRED_TARGET)
        })
        .count();
    event_without_triggers(state, registrations);
    // In the backswing after a kill the answer only records the target
    // (SPEC-0019, "During such a backswing"; c-SAK-1: gnoll 17 at 4766).
    let now = state.event_time.unwrap_or_else(|| state.timers.time());
    let recording = state.unit(Some(h)).is_some_and(|u| {
        u.attack
            .is_none_or(|a| state.unit(Some(a.target)).is_none_or(|t| !t.alive))
            && f32::from_bits(u.backswing_end) > f32::from_bits(now)
    });
    if recording {
        return;
    }
    state.table.answering = true;
    acquired(state, h);
    state.table.answering = false;
    if let Some(e) = entries(state, h) {
        e.by_call = true;
    }
}

/// Unit `h`'s mana crossed the cost of an ability it learned: the watch
/// on it is made anew (for the other direction) and the old one freed
/// 0.0001 s later (c-SAK-1: the Mountain King's third Storm Bolt at 4480
/// took him below 75).
///
/// PROVISIONAL: the watch's limit taken to be the cost; the crossing back
/// up is checked at the end of each tick.
pub(super) fn mana_watches(state: &mut super::State, h: u32, spent: i32) {
    mana_watches_crossed(state, h, spent);
    // A cost paid re-bases the mana (c-SAK-1: 39.896290).
    if spent > 0 {
        let clock = state.move_clock;
        watches_rebased(state, h, clock);
    }
}

fn mana_watches_crossed(state: &mut super::State, h: u32, spent: i32) {
    let tick = state.table.now;
    let Some(mana) = state
        .unit(Some(h))
        .map(|u| f32::from_bits(u.mana.value()) - spent as f32)
    else {
        return;
    };
    let watches = entries(state, h)
        .map(|e| e.mana_watches.clone())
        .unwrap_or_default();
    // The old watches go 0.0001 s later, the newest first (c-RZH-1: the
    // Mountain King's Thunder Clap at 26.40, 2947 then 1167; c-RZJ-1: the
    // Archmage's 2814 then 1230 at 22.797274).
    let mut old = Vec::new();
    for (k, (cost, entry, above)) in watches.into_iter().enumerate() {
        let now = mana >= cost as f32;
        if now == above {
            continue;
        }
        // Back above the cost, the watch fires at its next check
        // (`watches_checked`).
        if now {
            continue;
        }
        let new = state.table.take(tick, "unit state: limit watch");
        old.push(entry);
        if let Some(e) = entries(state, h) {
            e.mana_watches[k] = (cost, new, now);
            e.watch_checks.retain(|&(w, _)| w != entry);
        }
        watch_scheduled(state, h, new);
    }
    old.sort_by_key(|&e| std::cmp::Reverse(state.table.creation(e)));
    for entry in old {
        state.table.defer(entry, "unit state: limit watch");
    }
}

/// The step of a watch's checks while its cost is far (c-SAK-1: the
/// Mountain King's 3914, made at 22.397242, checked at 26.39724).
const WATCH_STEP: f32 = 4.0;

/// A watch `entry` of unit `h` below its cost is made: its first check
/// comes 4.0 s after the last re-base of the mana, by the world clock
/// (c-SAK-1: the watch learned at the level-up of 30.255922 counts from
/// the rate change of 30.256021).
pub(super) fn watch_scheduled(state: &mut super::State, h: u32, entry: u32) {
    let clock = state.move_clock;
    if let Some(e) = entries(state, h) {
        let base = if f32::from_bits(e.watch_base) > f32::from_bits(clock) {
            e.watch_base
        } else {
            clock
        };
        e.watch_checks
            .push((entry, ow3_math::add(base, WATCH_STEP.to_bits())));
    }
}

/// Unit `h`'s mana is re-based at world clock `base`: every watch below
/// its cost is checked next 4.0 s from it (c-SAK-1: the Mountain King's
/// watches at the fourth bolt's cost, 39.896290 -> 43.896290).
pub(super) fn watches_rebased(state: &mut super::State, h: u32, base: u32) {
    let next = ow3_math::add(base, WATCH_STEP.to_bits());
    if let Some(e) = entries(state, h) {
        e.watch_base = base;
        for c in &mut e.watch_checks {
            c.1 = next;
        }
    }
}

/// Unit `h`'s mana rate changed (c-SAK-1: the Mountain King's level-up,
/// rate changed at 30.256021, next checks 34.25602).
pub(super) fn watches_rescheduled(state: &mut super::State, h: u32) {
    // The change is stamped 0.0001 s after the clock of the tick
    // (30.255922 + 0.0001).
    let change = ow3_math::add(state.move_clock, crate::timers::MIN_TIMEOUT);
    watches_rebased(state, h, change);
}

/// The checks of unit `h`'s watches below their costs, on the world
/// clock at the start of the next tick: one whose cost is passed fires,
/// made anew above it and the old freed 0.0001 s later; else the next
/// check comes where the mana reaches the cost + 0.1, or 4.0 s on when
/// that is later (c-SAK-1: the Mountain King's at 38.25602, next
/// 39.508526, fired 7903; the Blademaster's at 52.478168, next
/// 55.605343).
pub(super) fn watches_checked(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let clock = state.move_clock;
    let due: Vec<(u32, u32)> = entries(state, h)
        .map(|e| {
            e.watch_checks
                .iter()
                .filter(|&&(_, n)| f32::from_bits(n) <= f32::from_bits(clock))
                .copied()
                .collect()
        })
        .unwrap_or_default();
    let Some((mana, rate)) = state.unit(Some(h)).map(|u| {
        (
            f32::from_bits(u.mana.value()),
            f32::from_bits(u.mana.rate()),
        )
    }) else {
        return;
    };
    for (entry, at) in due {
        let cost = entries(state, h)
            .and_then(|e| e.mana_watches.iter().find(|w| w.1 == entry).map(|w| w.0));
        let Some(cost) = cost else {
            if let Some(e) = entries(state, h) {
                e.watch_checks.retain(|&(w, _)| w != entry);
            }
            continue;
        };
        // The value at the check's own time.
        let value = state
            .unit(Some(h))
            .map_or(mana, |u| f32::from_bits(u.mana.value_at(at)));
        if value >= cost as f32 {
            let new = state.table.take(tick, "unit state: limit watch");
            state.table.defer(entry, "unit state: limit watch");
            if let Some(e) = entries(state, h) {
                e.watch_checks.retain(|&(w, _)| w != entry);
                if let Some(w) = e.mana_watches.iter_mut().find(|w| w.1 == entry) {
                    w.1 = new;
                    w.2 = true;
                }
            }
            continue;
        }
        let near = if rate > 0.0 {
            (cost as f32 + 0.1 - value) / rate
        } else {
            f32::INFINITY
        };
        let step = if near < WATCH_STEP { near } else { WATCH_STEP };
        // Counted from the check's own time (26.397 -> 30.397).
        let next = ow3_math::add(at, step.to_bits());
        if let Some(e) = entries(state, h)
            && let Some(c) = e.watch_checks.iter_mut().find(|c| c.0 == entry)
        {
            c.1 = next;
        }
    }
}

/// Unit `h`'s attack ends at its Wind Walk's fade (c-SAK-1: the
/// Blademaster at 4620): one object kept with its order, and the objects
/// kept to its target's death freed at once.
pub(super) fn attack_ended_at_fade(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let o = state.table.take(tick, "attack ends");
    // Its objects kept to the target's death go now; the new one goes
    // first at its next order (4800).
    let gone = entries(state, h).map(|e| {
        let gone: Vec<u32> = e
            .current
            .replace(o)
            .into_iter()
            .chain(
                std::mem::take(&mut e.target_death)
                    .into_iter()
                    .map(|(_, t)| t),
            )
            .collect();
        gone
    });
    // They go after the fade's state parts (`wind_walk_faded`).
    let gone = gone.unwrap_or_default();
    if let Some(e) = entries(state, h) {
        e.fade.extend(gone);
    }
}

/// Unit `h`'s attack on a unit that died ends: one object, freed 0.0001 s
/// later, or kept with the move it takes up instead (`kept`; a creep
/// whose return waited behind the attack) (SPEC-0024, "Unit death";
/// c-SAK-1: 4764).
pub(super) fn attack_ended(state: &mut super::State, h: u32, kept: bool) {
    let tick = state.table.now;
    let o = state.table.take(tick, "attack ends at the target death");
    if kept {
        // Kept with the move it goes on to (`queue_taken_up`).
        if let Some(e) = entries(state, h) {
            match e.current {
                None => e.current = Some(o),
                Some(_) => e.order.insert(0, o),
            }
        }
    } else {
        state.table.defer(o, "attack ends at the target death");
    }
}

/// Unit `h`'s script attack order lost its target: the sixth of its start
/// objects goes, 0.0001 s later (c-SBL-1: 1275 at kobold 3's death).
/// Returned, for 0.0001 s later after the attack's end, the rest of a unit
/// not in a backswing at queue time `now` (c-SBM-1: the footman's at the
/// Far Seer's kill of 7.144721, 1384, then 1405, 1383 .. 1379, 1377;
/// c-SBL-1: the Mountain King's at the footman's kill of 11.569).
pub(super) fn script_attack_lost(state: &mut super::State, h: u32, now: u32) -> Vec<u32> {
    let end = state.unit(Some(h)).map_or(0, |u| u.backswing_end);
    let swinging = f32::from_bits(end) > f32::from_bits(now);
    let first = entries(state, h).and_then(|e| (!e.order.is_empty()).then(|| e.order.remove(0)));
    if let Some(o) = first {
        state.table.defer(o, "attack: order start");
    }
    // The rest, with the order, at the end of the backswing of the kill: the
    // order ends there (c-SBL-1: 1274 .. 1270, 1268 at 2.884976).
    let rest = entries(state, h)
        .map(|e| {
            e.script_attack = false;
            std::mem::take(&mut e.order)
        })
        .unwrap_or_default();
    if !swinging {
        return rest;
    }
    for &o in &rest {
        state.table.free_at_q(end, o, "attack: order end");
    }
    if let Some(e) = entries(state, h) {
        e.order_ending = rest.into_iter().map(|o| (end, o)).collect();
    }
    Vec::new()
}

/// Unit `h`'s backswing ending at queue time `old` ends at `new` instead:
/// the objects of its order lost with its target go then.
pub(super) fn backswing_moved(state: &mut super::State, h: u32, old: u32, new: u32) {
    let Some(e) = entries(state, h) else {
        return;
    };
    let mut moved = Vec::new();
    for p in &mut e.order_ending {
        if p.0 == old {
            p.0 = new;
            moved.push(p.1);
        }
    }
    for t in &mut state.table.timed_q {
        if t.0 == old && t.2 == "attack: order end" && moved.contains(&t.1) {
            t.0 = new;
        }
    }
}

/// Attacker `h` of unit `dead` is handled at the death: the object of its
/// current target is freed 0.0001 s later now, the others it kept to the
/// death at the end of its handling (`attacker_done`), after its attack's
/// end (c-SAK-1: kobold 19 at 4764: 3593, "attack ends", 3533 .. 3531).
pub(super) fn attacker_lost(state: &mut super::State, h: u32, dead: u32) {
    let current = entries(state, h).map(|e| {
        let (gone, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut e.target_death)
            .into_iter()
            .partition::<Vec<_>, _>(|&(t, _)| t == dead);
        e.target_death = kept;
        let mut gone: Vec<u32> = gone.into_iter().map(|(_, o)| o).collect();
        // The current target's object, or the first kept to the death.
        let first = if gone.is_empty() {
            None
        } else {
            e.current.take().or_else(|| Some(gone.remove(0)))
        };
        e.lost = gone;
        // A reaction delay still running goes first (c-SAK-1: footman 2,
        // 4910 at gnoll 16's death in 6130).
        let reaction = if first.is_some() {
            std::mem::take(&mut e.reaction)
        } else {
            Vec::new()
        };
        (reaction, first)
    });
    if let Some((reaction, first)) = current {
        for c in reaction.into_iter().chain(first) {
            state.table.defer(c, "acquisition: attack object");
        }
    }
}

/// The end of attacker `h`'s handling at a death (`attacker_lost`).
pub(super) fn attacker_done(state: &mut super::State, h: u32) {
    // Its attack on the dead unit is over.
    if let Some(e) = entries(state, h) {
        e.own_attack = false;
    }
    let goes_on = state.table.resumed.contains(&h);
    let rest = entries(state, h).map(|e| {
        if std::mem::take(&mut e.keep_lost) {
            let lost = std::mem::take(&mut e.lost);
            e.lost_later.extend(lost);
        }
        let mut rest = std::mem::take(&mut e.lost);
        // One walking back frees its acquisition's too (c-SAK-1: kobold 19
        // at 4764, 3533 .. 3531).
        if goes_on || std::mem::take(&mut e.lose_acquisition) {
            rest.extend(std::mem::take(&mut e.acquisition));
        }
        if std::mem::take(&mut e.lose_order) {
            rest.extend(std::mem::take(&mut e.order));
            e.own_attack = false;
        }
        rest
    });
    for o in rest.unwrap_or_default() {
        state.table.defer(o, "acquisition: attack object");
    }
}

/// Attacker `h`, in its backswing at its target's death, keeps the
/// objects it held to the death to its backswing's end
/// (`backswing_ended_idle`).
pub(super) fn lost_at_backswing_end(state: &mut super::State, h: u32) {
    if let Some(e) = entries(state, h) {
        e.keep_lost = true;
    }
}

/// Unit `h`'s backswing after its target's death ended and it stays: the
/// objects it kept to the death go, then its order ends (c-SAK-1: the
/// image 20, 6455 .. 6453 and its order 6452 .. 6450 at 10950).
pub(super) fn backswing_ended_idle(state: &mut super::State, h: u32) {
    let lost = entries(state, h)
        .map(|e| std::mem::take(&mut e.lost_later))
        .unwrap_or_default();
    if lost.is_empty() {
        return;
    }
    for e in lost {
        state.table.defer(e, "acquisition: attack object");
    }
    order_ended(state, h);
}

/// Attacker `h`'s order ends with its attack at a death (`attacker_done`).
/// Attacker `h`'s acquisition ends with its attack at a death.
pub(super) fn acquisition_lost_with(state: &mut super::State, h: u32) {
    if let Some(e) = entries(state, h) {
        e.lose_acquisition = true;
        e.guard_walk = true;
    }
}

pub(super) fn order_lost_with(state: &mut super::State, h: u32) {
    if let Some(e) = entries(state, h) {
        e.lose_order = true;
    }
}

/// `EVENT_UNIT_SPELL_EFFECT` of common.j.
const EVENT_UNIT_SPELL_EFFECT: i32 = 291;

/// The objects of a unit event with `registrations` registrations that
/// fire (SPEC-0024, "Event objects"), for events whose triggers are not
/// run yet: none without one, else the event's and one per registration,
/// all freed 0.0001 s later (the registrations' first).
fn event_without_triggers(state: &mut super::State, registrations: usize) {
    if registrations == 0 {
        return;
    }
    let tick = state.table.now;
    let event = state.table.take(tick, "event");
    for _ in 0..registrations {
        let e = state.table.take(tick, "event object");
        state.table.defer(e, "event object");
    }
    state.table.defer(event, "event");
}

/// The objects of the spell effect event of unit `h` (SPEC-0024, "Event
/// objects").
pub(super) fn spell_effect_event(state: &mut super::State, h: u32) {
    let registrations = state
        .events
        .iter()
        .filter(|r| {
            matches!(**r, super::widgets::Registration::Unit { unit, event, .. }
                if unit == h && event == EVENT_UNIT_SPELL_EFFECT)
        })
        .count();
    event_without_triggers(state, registrations);
}

/// Hero `h`'s Mirror Image takes effect, its `cost` not spent yet
/// (c-SAK-1: the Blademaster at 7161): the effect's object and its
/// event's, a new mana watch for each costed ability whose cost the mana
/// crosses, and the effect's own object, kept. Then, 0.0001 s later: the
/// effect's two, the old watches (the last first), the first new watch,
/// the order's objects (the order ends with the effect), the other new
/// watches and the parts of its level: hidden, the hero has no rate, and
/// its watches go with it.
///
/// PROVISIONAL: what the hero takes at its return is not read.
pub(super) fn mirror_effect(state: &mut super::State, h: u32, cost: i32) {
    let tick = state.table.now;
    spell_effect_event(state, h);
    let Some(mana) = state
        .unit(Some(h))
        .map(|u| f32::from_bits(u.mana.value()) - cost as f32)
    else {
        return;
    };
    let watches = entries(state, h)
        .map(|e| std::mem::take(&mut e.mana_watches))
        .unwrap_or_default();
    let crossed: Vec<u32> = watches
        .iter()
        .filter(|&&(cost, _, above)| (mana >= cost as f32) != above)
        .map(|&(_, entry, _)| entry)
        .collect();
    let new = state
        .table
        .take_n(tick, crossed.len(), "unit state: limit watch");
    let effect = state.table.take(tick, "Mirror Image effect");
    for &e in crossed.iter().rev() {
        state.table.defer(e, "unit state: limit watch");
    }
    if let Some(&first) = new.first() {
        state.table.defer(first, "unit state: limit watch");
    }
    order_ended(state, h);
    for &e in new.iter().skip(1) {
        state.table.defer(e, "unit state: limit watch");
    }
    let level = entries(state, h).map(|e| {
        e.mirror_effect = Some(effect);
        e.hidden_watches = watches
            .iter()
            .filter(|w| crossed.contains(&w.1))
            .map(|w| w.0)
            .collect();
        // The watches that did not cross stay with the hero.
        e.mana_watches = watches
            .into_iter()
            .filter(|w| !crossed.contains(&w.1))
            .collect();
        std::mem::take(&mut e.level)
    });
    for e in level.unwrap_or_default() {
        state.table.defer(e, "hero level");
    }
}

/// A copy `unit` of a hero is made by its Mirror Image (c-SAK-1: the
/// image and the stand-in at 7261): 36 objects, 12 of them, parts of its
/// state, freed 0.0001 s later, after the 0 s timers of the copies' enter
/// events (`copies_freed`); the others are kept to its removal.
///
/// PROVISIONAL: what each of the 36 holds is not read.
pub(super) fn mirror_copy(state: &mut super::State, unit: u32) {
    let tick = state.table.now;
    let o = state.table.take_n(tick, 36, "Mirror Image: copy");
    for k in [17, 18, 23, 22, 19, 20, 24, 25, 26, 27, 32, 29] {
        state.table.copy_parts.push(o[k]);
    }
    // In the order they go at the stand-in's removal (c-SAK-1: 7297).
    let kept = [
        35, 33, 34, 10, 11, 31, 30, 28, 21, 16, 15, 14, 13, 12, 8, 7, 6, 5, 4, 9, 2, 3, 1, 0,
    ]
    .map(|k| o[k])
    .to_vec();
    state.table.copies.insert(unit, kept);
}

/// Unit `h` died of the hit that stunned it: the stun's missile objects
/// go at the tick its stun would have ended (c-SAK-1: gnoll 17, killed by
/// the bolt of 7981, its objects freed at 8981).
pub(super) fn stunned_dead(state: &mut super::State, h: u32, end_tick: u64) {
    for e in state.table.dead_missiles.remove(&h).unwrap_or_default() {
        state.table.free_at(end_tick, e, "missile");
    }
}

/// The parts of the copies' states made by a Mirror Image go, before
/// the objects of their enter events (c-SAK-1: 7261).
pub(super) fn copies_freed(state: &mut super::State) {
    for e in std::mem::take(&mut state.table.copy_parts) {
        state.table.defer(e, "Mirror Image: copy");
    }
}

/// The missile of a Mirror Image to the place of copy `unit`: 5 objects,
/// freed at its arrival after the copy's summon event (`mirror_arrived`).
pub(super) fn mirror_missile(state: &mut super::State, unit: u32) {
    let tick = state.table.now;
    let o = state.table.take_n(tick, 5, "Mirror Image: missile");
    state
        .table
        .missiles
        .insert(unit, vec![o[4], o[2], o[3], o[1], o[0]]);
}

/// The missile to copy `unit`'s place arrived: its objects go 0.0001 s
/// later, after the summon event's (c-SAK-1: 7288, 7297).
pub(super) fn mirror_arrived(state: &mut super::State, unit: u32) {
    copies_freed(state);
    for e in state.table.missiles.remove(&unit).unwrap_or_default() {
        state.table.defer(e, "Mirror Image: missile");
    }
}

/// Hero `h` comes back at its stand-in's place (c-SAK-1: 7297): an order
/// record and 5 objects of the placing, its mana watches and the parts of
/// its level made anew, one more of the placing, and the record and 5
/// objects of its order after the return. Freed 0.0001 s later: the
/// record, 3 of the placing, the stand-in's objects with one more of the
/// placing among them, the first of the placing, the order's, the last of
/// the placing and the effect's object.
pub(super) fn mirror_hero_back(state: &mut super::State, h: u32, stand_in: u32) {
    let tick = state.table.now;
    let record = state.table.take(tick, "order record");
    let p = state.table.take_n(tick, 5, "Mirror Image: hero placed");
    let costs = entries(state, h)
        .map(|e| std::mem::take(&mut e.hidden_watches))
        .unwrap_or_default();
    let watches = state
        .table
        .take_n(tick, costs.len(), "unit state: limit watch");
    let level = state.table.take_n(tick, 2, "hero level");
    let last = state.table.take(tick, "Mirror Image: hero placed");
    let record2 = state.table.take(tick, "order record");
    let order = state.table.take_n(tick, 5, "order objects");
    let mana = state
        .unit(Some(h))
        .map_or(0.0, |u| f32::from_bits(u.mana.value()));
    let effect = entries(state, h).and_then(|e| {
        e.level = level;
        // Made anew the last first (c-SAK-1: the Blademaster's Wind Walk
        // watch is 5608, made after his Mirror Image's 5607).
        for (&cost, &w) in costs.iter().rev().zip(&watches) {
            e.mana_watches.push((cost, w, mana >= cost as f32));
        }
        e.mirror_effect.take()
    });
    for (&cost, &w) in costs.iter().rev().zip(&watches) {
        if mana < cost as f32 {
            watch_scheduled(state, h, w);
        }
    }
    state.table.defer(record, "order record");
    for &e in [p[4], p[3], p[2]].iter() {
        state.table.defer(e, "Mirror Image: hero placed");
    }
    let copy = state.table.copies.remove(&stand_in).unwrap_or_default();
    let (a, b) = copy.split_at(copy.len().min(5));
    for &e in a {
        state.table.defer(e, "Mirror Image: copy");
    }
    state.table.defer(p[1], "Mirror Image: hero placed");
    for &e in b {
        state.table.defer(e, "Mirror Image: copy");
    }
    state.table.defer(p[0], "Mirror Image: hero placed");
    state.table.defer(record2, "order record");
    for &e in order.iter().rev() {
        state.table.defer(e, "order objects");
    }
    state.table.defer(last, "Mirror Image: hero placed");
    if let Some(e) = effect {
        state.table.defer(e, "Mirror Image effect");
    }
}

/// Unit `h`'s Thunder Clap takes effect (c-RZH-1, 26.40 s): the spell
/// effect event, then the watches its cost crosses.
pub(super) fn clap_effect(state: &mut super::State, h: u32, cost: i32) {
    spell_effect_event(state, h);
    mana_watches(state, h, cost);
}

/// An area spell of `source` passed over units at queue time `at` (a
/// thunder clap's hits): the source's timed fog modifier, made at its
/// first pass, lasts `FOG_TIME` from its last restart; any later pass of
/// the source, or a target the source acquires, restarts it (c-SBG-1:
/// 1064, the clap at 2.9, freed at 5.9; c-RZH-1: 3059, the clap at 26.40,
/// restarted by the Mountain King's acquisition at 27.341698, freed at
/// 30.34).
pub(super) fn clap_area(state: &mut super::State, at: u32, source: Option<u32>) {
    if let Some(h) = source {
        fog_pass(state, h, at, true);
    }
}

/// The source `h`'s fog modifier at queue time `at`: restarted if it runs,
/// else made when `make` (an area pass).
pub(super) fn fog_pass(state: &mut super::State, h: u32, at: u32, make: bool) {
    let tick = state.table.now;
    let end = ow3_math::add(at, FOG_TIME.to_bits());
    let running = entries(state, h)
        .and_then(|e| e.fog)
        .filter(|&(_, until)| f32::from_bits(until) >= f32::from_bits(at));
    let o = match running {
        Some((o, _)) => {
            state.table.timed_q.retain(|t| t.1 != o);
            o
        }
        None if make => state.table.take(tick, "fog modifier"),
        None => return,
    };
    state.table.free_at_q(end, o, "fog modifier");
    if let Some(e) = entries(state, h) {
        e.fog = Some((o, end));
    }
}

/// How long a timed fog modifier lasts from its last restart (3.0 s).
const FOG_TIME: f32 = 3.0;

/// Unit `h` is slowed by a thunder clap and lives, to queue time `end`:
/// the slow's object, kept, and its walk made again for the new speed
/// (c-RZH-1: tunneler 10 at 26.40 s; c-SBG-1: ogre 3).
///
/// PROVISIONAL: the walk taken to be made again only for a unit that has
/// one; a slow on a unit slowed already is not read.
pub(super) fn clap_slowed(state: &mut super::State, h: u32, end: u32) {
    let tick = state.table.now;
    let buff = state.table.take(tick, "Thunder Clap effect");
    let walking = entries(state, h).is_some_and(|e| {
        e.slow_buff = Some(buff);
        e.slow_end_q = Some(end);
        !e.walk.is_empty()
    });
    if walking {
        // Its search comes at its next update but one and takes none
        // (hub #10).
        walk_made_at_order(state, h);
        super::movement::slowed_now(state, h);
        super::movement::chase_renewed(state, h, true);
    }
}

/// Unit `h` is rooted by Entangling Roots: the roots' object, a walk made
/// for it (it stands now), one more object, both kept; then its order ends
/// and the walk is given up (c-SBK-1: the ogre at 7.239974: 1413, walk
/// 1415/1416 freed at its next update, 1417, its acquired attack's
/// objects 1145 .. 1139).
///
/// PROVISIONAL: read for one unit with an acquired attack; when the two
/// objects go and what the unit does after are not read.
pub(super) fn rooted(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    state.table.take(tick, "roots");
    walk_made(state, h);
    state.table.take(tick, "roots: other");
    order_ended(state, h);
    walk_dropped(state, h);
    if let Some(super::Object::Unit(u)) = state.objects.get_mut(&h) {
        u.attack = None;
    }
}

/// The thunder clap slows that end by time in this tick (c-SBG-1: the
/// paladin at q 5.900000, the ogres at 7.900000), in the order the clap
/// hit them. A creep is given the order that ends the buff (its buff end
/// object): standing, it is carried out at once with a second object,
/// both freed 0.0001 s later; walking its move, it waits to the move's
/// end (`buff_end_carried_out`). Then the slow's object goes, and with
/// the first unit's the clap's area object.
///
/// PROVISIONAL: a player's unit is taken to have no buff end, as at a
/// stun's end.
pub(super) fn slow_ends(state: &mut super::State, limit: u32) {
    let tick = state.table.now;
    // Equal ends in the order of the hits: by their slows' objects.
    let mut ending: Vec<(u32, u32, u32)> = state
        .units
        .iter()
        .filter_map(|&h| {
            let e = &state.unit(Some(h))?.entries;
            let end = e.slow_end_q?;
            let made = e.slow_buff.map_or(0, |b| state.table.creation_of(b));
            (f32::from_bits(end) <= f32::from_bits(limit)).then_some((h, end, made))
        })
        .collect();
    ending.sort_by(|a, b| {
        f32::from_bits(a.1)
            .total_cmp(&f32::from_bits(b.1))
            .then(a.2.cmp(&b.2))
    });
    for (h, end, _) in ending {
        let Some(u) = state.unit(Some(h)) else {
            continue;
        };
        let creep = u.owner == super::creeps::CREEPS;
        let moving = super::movement::on_move(state, h);
        // A creep stunned or attacking holds it: freed when its stun's
        // order comes back, or kept (c-SBJ-1: the frost on gnoll 4, under
        // Impale, freed at 9.914337; kobolds 5 and 6 attacking).
        let busy = state
            .unit(Some(h))
            .is_some_and(|u| u.entries.stun_ticks.is_some() || u.attack.is_some());
        if creep && busy && !moving {
            let buff_end = state.table.take(tick, "buff end");
            if let Some(e) = entries(state, h) {
                e.buff_end = Some(buff_end);
            }
        } else if creep {
            let buff_end = state.table.take(tick, "buff end");
            if moving {
                if let Some(e) = entries(state, h) {
                    e.buff_end_waiting = Some(buff_end);
                }
            } else {
                let second = state.table.take(tick, "buff end: second object");
                state.table.defer(second, "buff end: second object");
                state.table.defer(buff_end, "buff end");
            }
        }
        // Its speed comes back at its next update (SPEC-0037), its attack
        // times at once (c-SBJ-1: kobold 6 at 8.96).
        let cooldown = super::attack::swing_cooldown(state, h);
        if let Some(super::Object::Unit(u)) = state.objects.get_mut(&h) {
            u.move_slow = None;
            if let Some((_, speed)) = u.slowed_until.take() {
                super::attack::unslow_cooldown(u, end, speed, cooldown);
            }
        }
        // A backswing that ends later resumes in the tick its new end falls
        // in (c-SBJ-1: kobold 5's at 9.059998, after kobold 6's hit at
        // 9.059997).
        let back = state.unit(Some(h)).map_or(0, |u| u.backswing_end);
        if f32::from_bits(back) > f32::from_bits(end) {
            let resume = super::cast::tick_of(state, back) + 1;
            if let Some(super::Object::Unit(u)) = state.objects.get_mut(&h)
                && let Some(a) = u.attack.as_mut()
                && a.resume.is_some()
            {
                a.resume = Some(resume);
            }
        }
        let buff = entries(state, h)
            .map(|e| {
                e.slow_end_q = None;
                e.slow_buff.take()
            })
            .unwrap_or_default();
        if let Some(b) = buff {
            state.table.defer(b, "Thunder Clap effect");
        }
    }
}

/// Unit `h`'s move ended with a buff end order waiting behind it: it is
/// carried out now, a second object made, both freed 0.0001 s later
/// (c-SBG-1: ogre 3 at 8.819954).
pub(super) fn buff_end_carried_out(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let Some(buff_end) = entries(state, h).and_then(|e| e.buff_end_waiting.take()) else {
        return;
    };
    let second = state.table.take(tick, "buff end: second object");
    state.table.defer(second, "buff end: second object");
    state.table.defer(buff_end, "buff end");
}

/// A creep's Shockwave takes effect (c-RZJ-1, the Ogre Lord at 3.74):
/// the wave's object and 4 of its missile (position, two regions, the
/// flight), freed as a missile's 5.0 s after the wave's last step
/// (`missile_hit`; 9.504406).
pub(super) fn shockwave_effect(state: &mut super::State) -> Vec<u32> {
    let tick = state.table.now;
    let m = state.table.take_n(tick, 5, "Shockwave: wave");
    vec![m[4], m[2], m[3], m[1], m[0]]
}

/// Unit `h` is hit by a Shockwave of `caster` at the step of queue time
/// `at`: the wave's hit buff before its damage, ending `SHOCKWAVE_HIT`
/// after the last step that covers the unit (`shockwave_covered`), then
/// freed at that tick's end (c-RZJ-1: the Paladin's 1690, hit at
/// 3.937711, last covered at 4.237710, off at 4.357710).
pub(super) fn shockwave_hit(state: &mut super::State, caster: u32, h: u32, at: u32) {
    let tick = state.table.now;
    let o = state.table.take(tick, "Shockwave: hit buff");
    state.table.free_at_q(
        ow3_math::add(at, SHOCKWAVE_HIT.to_bits()),
        o,
        "Shockwave: hit buff",
    );
    state.table.shock_buffs.push((caster, h, o));
}

/// A later step of `caster`'s Shockwave covers unit `h` again at queue
/// time `at`: its hit buff starts again.
pub(super) fn shockwave_covered(state: &mut super::State, caster: u32, h: u32, at: u32) {
    // The latest (an earlier wave's buff on the unit may be gone).
    let Some(&(_, _, o)) = state
        .table
        .shock_buffs
        .iter()
        .rev()
        .find(|b| b.0 == caster && b.1 == h)
    else {
        return;
    };
    if !state.table.timed_q.iter().any(|t| t.1 == o) {
        return;
    }
    state.table.timed_q.retain(|t| t.1 != o);
    state.table.free_at_q(
        ow3_math::add(at, SHOCKWAVE_HIT.to_bits()),
        o,
        "Shockwave: hit buff",
    );
}

/// How long a Shockwave's hit buff lasts from its last start (0.12 s).
const SHOCKWAVE_HIT: f32 = 0.12;

/// A Blizzard wave hits unit `h`: its short buff, taken off at queue
/// time `end` (the wave's plus 0.99 s) and freed at that tick's end
/// (c-RZJ-1: 1751 at 4.139997, off at 5.129997).
///
/// PROVISIONAL: a unit's death before that is not read.
pub(super) fn blizzard_hit(state: &mut super::State, h: u32, end: u32) {
    let tick = state.table.now;
    let o = state.table.take(tick, "Blizzard: buff");
    state.table.free_at_q(end, o, "Blizzard: buff");
    if let Some(e) = entries(state, h) {
        e.blizzard_buff = Some((o, end));
    }
}

/// A missile of an attack is launched (c-RZJ-1: the water elemental's at
/// 4.839993): its own object, its position, two regions and its flight.
/// Given in the order they are freed then: the flight, the regions, the
/// position, its own (c-RZJ-1 at 10.099306, as the Storm Bolt's).
pub(super) fn missile_launched(state: &mut super::State) -> Vec<u32> {
    let tick = state.table.now;
    let m = state.table.take_n(tick, 5, "missile");
    vec![m[4], m[2], m[3], m[1], m[0]]
}

/// A missile hits at queue time `at`: its objects are freed by a 5.0 s
/// timer from there, at that tick's end (c-RZJ-1: 5.094869 -> 10.099306;
/// c-SAK-1: the Storm Bolt's, 3.699956 -> 8.704540).
///
/// PROVISIONAL: a missile lost with its target keeps its objects.
pub(super) fn missile_hit(state: &mut super::State, objects: &[u32], at: u32) {
    let end = ow3_math::add(at, MISSILE_AFTER.to_bits());
    for &e in objects {
        state.table.free_at_q(end, e, "missile");
    }
}

/// The time from a missile's impact to the free of its objects (5.0 s).
const MISSILE_AFTER: f32 = 5.0;

/// Unit `h`'s Blizzard takes effect (c-RZJ-1 at 3.24): the spell effect
/// event, then the channel's object, kept to the channel's end.
pub(super) fn blizzard_effect(state: &mut super::State, h: u32, cost: i32) {
    spell_effect_event(state, h);
    // The watches its cost crosses, before the channel's object (c-RZJ-1:
    // 2249, then 2250, at the second Blizzard's 9.80).
    mana_watches(state, h, cost);
    let tick = state.table.now;
    let channel = state.table.take(tick, "Blizzard: channel");
    if let Some(e) = entries(state, h) {
        e.channel = Some(channel);
    }
}

/// Unit `h`'s cast ends: a channel's object goes 0.0001 s later (c-RZJ-1:
/// 1621 at 9.144466).
pub(super) fn channel_ended(state: &mut super::State, h: u32) {
    if let Some(c) = entries(state, h).and_then(|e| e.channel.take()) {
        state.table.defer(c, "Blizzard: channel");
    }
}

/// Unit `h`'s Storm Bolt takes effect (c-SAK-1, 3.04 s): the spell effect
/// event, then the 5 objects of the missile, kept to the end of the stun
/// of its hit; gives them in the order they are freed then.
///
/// PROVISIONAL: read for Storm Bolt in c-SAK-1 only.
pub(super) fn bolt_effect(state: &mut super::State, h: u32, cost: i32) -> Vec<u32> {
    spell_effect_event(state, h);
    // Its cost, spent now, may cross a watch (c-SAK-1: 4480).
    mana_watches(state, h, cost);
    let tick = state.table.now;
    let m = state.table.take_n(tick, 5, "missile");
    vec![m[4], m[2], m[3], m[1], m[0]]
}

/// A spell's missile of `caster` stuns `target` (c-SAK-1: the bolt of
/// 3.7 s), between the two damage events of its hit: the stun object,
/// kept to the stun's end with the missile's `objects`; the stun order and
/// its record, the target's order ending; 10 objects of the stun order, 4
/// of them freed 0.0001 s later, the others and the order when the order
/// comes back; and the caster's order object kept to the impact is freed
/// 0.0001 s later.
///
/// PROVISIONAL: read for Storm Bolt in c-SAK-1 only, on a target not
/// stunned before.
pub(super) fn stun_objects(state: &mut super::State, caster: u32, target: u32, objects: Vec<u32>) {
    let tick = state.table.now;
    let stun = state.table.take(tick, "stun");
    // A paused unit keeps its pause as its order: the stun object alone
    // (c-SBL-1: the paused ogre at the bolt of 5.28).
    if state.unit(Some(target)).is_some_and(|u| u.paused) {
        if let Some(e) = entries(state, target) {
            e.stun_end = std::iter::once(stun).chain(objects).collect();
        }
        impact_freed(state, caster);
        return;
    }
    let order = state.table.take(tick, "stun: order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    // An order of the game the target had comes back after the stun.
    let had = entries(state, target).is_some_and(|e| e.game_order);
    if let Some(e) = entries(state, target) {
        e.stun_game_order = had;
    }
    // An attack-move keeps its order's own object through the stun; its
    // order comes back after it (c-RZH-1: the footman stunned by the
    // bash of 8.03, his script order 1252 kept to 25.58). So does a
    // harvest order, the parts kept to its arrival going first (c-RXD-1:
    // the peasant bashed at 21.677362, his order 1243 kept to 25.946646).
    let harvesting = state
        .unit(Some(target))
        .is_some_and(|u| u.harvest.is_some());
    if harvesting {
        harvest_arrived(state, target);
    }
    if let Some(e) = entries(state, target) {
        e.stun_harvest = harvesting;
    }
    let kept = state
        .unit(Some(target))
        .is_some_and(|u| u.attack_move.is_some() || harvesting)
        .then(|| entries(state, target).and_then(|e| e.order.pop()))
        .flatten();
    order_ended(state, target);
    if let (Some(o), Some(e)) = (kept, entries(state, target)) {
        e.order = vec![o];
    }
    let parts = state.table.take_n(tick, 10, "stun: order objects");
    for &e in parts[6..].iter().rev() {
        state.table.defer(e, "stun: order objects");
    }
    impact_freed(state, caster);
    if let Some(e) = entries(state, target) {
        e.stun_end = std::iter::once(stun).chain(objects).collect();
        e.stun_order = parts[..6].iter().rev().copied().chain([order]).collect();
    }
}

/// The caster's object kept to the impact: freed now if its order ended,
/// else with the order (c-SAK-1: the bolts of 741 and 2616).
fn impact_freed(state: &mut super::State, caster: u32) {
    let impact = entries(state, caster).map(|e| {
        let impact = std::mem::take(&mut e.impact);
        if e.order.is_empty() {
            impact
        } else {
            e.order.splice(0..0, impact);
            Vec::new()
        }
    });
    for e in impact.unwrap_or_default() {
        state.table.defer(e, "spell ability: order start");
    }
}

/// Unit `h` is stunned to the end tick `end`, its order coming back in
/// tick `back`.
pub(super) fn stun_ticks(state: &mut super::State, h: u32, end: u64, back: u64, end_q: u32) {
    if let Some(e) = entries(state, h) {
        e.stun_ticks = Some((end, back));
        e.stun_end_q = end_q;
    }
}

/// The earliest queue time of a stun ending in this tick still to come.
pub(super) fn next_stun_end(state: &super::State) -> Option<u32> {
    let tick = state.ticks;
    state
        .units
        .iter()
        .filter_map(|&h| state.unit(Some(h)))
        .filter(|u| u.entries.stun_ticks.is_some_and(|(end, _)| end == tick))
        .map(|u| u.entries.stun_end_q)
        .min_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)))
}

/// The stuns that end and the orders that come back in this tick (c-SAK-1:
/// 8.70 s and 8.71 s): at the end the buff's end takes an object, and the
/// stun and missile objects are freed 0.0001 s later; when the order comes
/// back it takes an object kept to the next order and one freed 0.0001 s
/// later, and the stun order and the buff's end object go 0.0001 s later.
pub(super) fn stuns(state: &mut super::State) {
    stuns_with(state, true);
}

/// The ends of stuns in this tick only, not the orders coming back.
/// The ends of Impale's stuns in this tick, in the order of the stuns:
/// per unit a 0.00 hit from the caster, then its stun's objects go
/// (c-SBJ-1: 9.9, the three units thrown at 6.9).
pub(super) fn impale_ends(
    vm: &mut ow3_jass::vm::Vm,
    state: &std::rc::Rc<std::cell::RefCell<super::State>>,
) {
    let ended = std::mem::take(&mut state.borrow_mut().impale_ends);
    for h in ended {
        let source = state.borrow().unit(Some(h)).and_then(|u| u.stunner);
        super::widgets::damage_widget(vm, state, source, Some(h), 0, 0, 0);
        let mut s = state.borrow_mut();
        let objects = entries(&mut s, h)
            .map(|e| {
                e.impale = false;
                std::mem::take(&mut e.stun_end)
            })
            .unwrap_or_default();
        for e in objects {
            s.table.defer(e, "stun end");
        }
    }
}

pub(super) fn stun_ends(state: &mut super::State) {
    stuns_with(state, false);
}

fn stuns_with(state: &mut super::State, backs: bool) {
    let tick = state.table.now;
    let mut units: Vec<u32> = state.units.clone();
    // In the order the stuns were made (c-SBI-1: War Stomp's four).
    let made = |s: &super::State, h: u32| {
        s.unit(Some(h))
            .and_then(|u| {
                u.entries
                    .stun_end
                    .first()
                    .or(u.entries.stun_order.last())
                    .copied()
            })
            .map_or(u32::MAX, |o| s.table.creation_of(o))
    };
    units.sort_by_key(|&h| made(state, h));
    for h in units {
        let Some((end, back)) = entries(state, h).and_then(|e| e.stun_ticks) else {
            continue;
        };
        if end == tick && !backs && entries(state, h).is_some_and(|e| e.impale) {
            if let Some(e) = entries(state, h) {
                e.stun_ticks = Some((u64::MAX, back));
            }
            state.impale_ends.push(h);
        } else if end == tick {
            // Only a creep is given the order that ends the stun, the buff
            // end object (c-RZH-1: none at the ends of footman 2's bash
            // stuns, 2007, 2817 and 4976).
            let buff = state
                .unit(Some(h))
                .is_some_and(|u| u.owner == super::creeps::CREEPS)
                .then(|| state.table.take(tick, "buff end"));
            let ended = entries(state, h).map(|e| {
                e.buff_end = buff;
                e.stun_ticks = Some((u64::MAX, back));
                std::mem::take(&mut e.stun_end)
            });
            for e in ended.unwrap_or_default() {
                state.table.defer(e, "stun end");
            }
        }
        if backs && back == tick && entries(state, h).is_some_and(|e| e.stun_harvest) {
            // A harvest order comes back: the object of its coming back, 13
            // parts (the last 4 freed 0.0001 s later, after the stun order's)
            // and a walk (c-RXD-1: the peasant bashed at 21.677362, back at
            // 23.687025).
            //
            // PROVISIONAL: when the kept parts go is not read; taken as for a
            // harvest order's start, 7 and 8 at the arrival.
            let order = state.table.take(tick, "order back after a stun");
            let parts = state.table.take_n(tick, 13, "harvest: order start");
            walk_made_at_order(state, h);
            let (stun, buff) = entries(state, h)
                .map(|e| {
                    e.stun_ticks = None;
                    e.stun_harvest = false;
                    let kept = std::mem::take(&mut e.order);
                    e.harvest_arrive = vec![parts[8], parts[7]];
                    e.order = parts[..7]
                        .iter()
                        .rev()
                        .copied()
                        .chain([order])
                        .chain(kept)
                        .collect();
                    (std::mem::take(&mut e.stun_order), e.buff_end.take())
                })
                .unwrap_or_default();
            for e in stun {
                state.table.defer(e, "stun: order objects");
            }
            for &e in parts[9..].iter().rev() {
                state.table.defer(e, "harvest: order start");
            }
            if let Some(b) = buff {
                state.table.defer(b, "buff end");
            }
            continue;
        }
        if backs && back == tick && entries(state, h).is_some_and(|e| e.stun_game_order) {
            // Its order of the game comes back: 13 objects of the move
            // ability (the last 7 freed 0.0001 s later, after the stun
            // order's, the first 6 kept with the order) and a walk; the
            // object of the order's coming back goes 2 ticks on (c-SAK-1:
            // gnoll 16 at 3618).
            let back_object = state.table.take(tick, "order back after a stun");
            state
                .table
                .free_at(tick + 2, back_object, "order back after a stun");
            let ability = state.table.take_n(tick, 13, "move ability: order start");
            walk_made_at_order(state, h);
            let parts = entries(state, h)
                .map(|e| {
                    e.stun_ticks = None;
                    e.stun_game_order = false;
                    e.game_order = true;
                    e.order = ability[..6].iter().rev().copied().collect();
                    std::mem::take(&mut e.stun_order)
                })
                .unwrap_or_default();
            for e in parts {
                state.table.defer(e, "stun: order objects");
            }
            for &e in ability[6..].iter().rev() {
                state.table.defer(e, "move ability: order start");
            }
            continue;
        }
        // An attack-move comes back as a new order: the 11 objects of the
        // attack ability and a walk, the stun order's objects and the last
        // 6 of the ability freed 0.0001 s later, the first 5 kept with the
        // order's own object (c-RZH-1: footman 2 at 2009, after the bash
        // of 8.03).
        if backs && back == tick && state.unit(Some(h)).is_some_and(|u| u.attack_move.is_some()) {
            let ability = state.table.take_n(tick, 11, "attack ability: order start");
            walk_made_at_order(state, h);
            let (parts, buff) = entries(state, h)
                .map(|e| {
                    let mut kept: Vec<u32> = ability[..5].iter().rev().copied().collect();
                    kept.append(&mut e.order);
                    e.order = kept;
                    e.stun_ticks = None;
                    (std::mem::take(&mut e.stun_order), e.buff_end.take())
                })
                .unwrap_or_default();
            for e in parts {
                state.table.defer(e, "stun: order objects");
            }
            for &e in ability[5..].iter().rev() {
                state.table.defer(e, "attack ability: order start");
            }
            if let Some(b) = buff {
                state.table.defer(b, "buff end");
            }
            continue;
        }
        if backs && back == tick {
            let order = state.table.take(tick, "order back after a stun");
            // The buff end's order carried out, for a unit that holds one
            // (c-SBJ-1: the gnoll's of the frost at 9.909338, none for the
            // kobolds out of Impale's stun).
            let holds = entries(state, h).is_some_and(|e| e.buff_end.is_some());
            let other = holds.then(|| state.table.take(tick, "order back: other"));
            let (parts, buff) = entries(state, h)
                .map(|e| {
                    // Freed 0.01 s later by the stun's own timer, after its 0.00
                    // hit (`order_back_freed`; c-SBI-1: 1434 at 1599; c-SAK-1:
                    // kobold 10's at 8.719954).
                    e.order_back = Some((order, tick + 2));
                    e.stun_ticks = None;
                    (std::mem::take(&mut e.stun_order), e.buff_end.take())
                })
                .unwrap_or_default();
            for e in parts {
                state.table.defer(e, "stun: order objects");
            }
            if let Some(o) = other {
                state.table.defer(o, "order back: other");
            }
            if let Some(b) = buff {
                state.table.defer(b, "buff end");
            }
        }
    }
}

/// Unit `h` casts Wind Walk at a script's immediate order (c-SAK-1, 2.0
/// s): the order and its record, the old order ends, the spell effect
/// event, then two objects kept to the end of the Wind Walk and a rate
/// part and a limit watch of the unit's state kept to its fade; the order
/// is freed 0.0001 s later, after the event.
///
/// PROVISIONAL: read in c-SAK-1 only; the triggers of the spell effect
/// event are not run yet, only their objects are taken.
pub(super) fn wind_walk_cast(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "script immediate order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    spell_effect_event(state, h);
    let lasting = state.table.take_n(tick, 2, "wind walk");
    let fading = state.table.take_n(tick, 2, "wind walk: state parts");
    state.table.defer(order, "script immediate order");
    if let Some(e) = entries(state, h) {
        e.fade = fading;
        e.lasting = lasting.into_iter().rev().collect();
    }
}

/// Unit `h`'s Wind Walk fades: its state parts are freed.
pub(super) fn wind_walk_faded(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let fade = entries(state, h).map(|e| std::mem::take(&mut e.fade));
    for e in fade.unwrap_or_default() {
        state.table.free(tick, e, "wind walk: state parts");
    }
}

/// Unit `h`'s Wind Walk ends: its objects are freed 0.0001 s later.
pub(super) fn wind_walk_ended(state: &mut super::State, h: u32) {
    let lasting = entries(state, h).map(|e| std::mem::take(&mut e.lasting));
    for e in lasting.unwrap_or_default() {
        state.table.defer(e, "wind walk");
    }
}

/// Unit `h` died: the objects it holds until its death are freed.
pub(super) fn unit_died(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let Some(owner) = state.unit(Some(h)).map(|u| u.owner) else {
        return;
    };
    super::cast::stream_target_died(state, h);
    // A stunned unit's order comes back first; that object goes 2 ticks
    // on (c-SAK-1: gnoll 13 at 3509, freed at 3511).
    // Also one whose stun came with the hit that kills it (c-SAK-1: gnoll
    // 17 at the bolt of 7981).
    // Not a paused one: its order is the pause (c-SBL-1: the ogre killed by
    // Storm Bolt at 5.28).
    let paused = state.unit(Some(h)).is_some_and(|u| u.paused);
    let stunned =
        entries(state, h).is_some_and(|e| e.stun_ticks.is_some() || !e.stun_order.is_empty());
    if stunned && !paused {
        let back = state.table.take(tick, "order back after a stun");
        state
            .table
            .free_at(tick + 2, back, "order back after a stun");
    }
    // A summon whose time ran out: a record and 7 queue objects; freed at
    // once the record, the last 3 of the queue, its buffs, the objects it
    // kept to its death and the fourth of the queue; at the end of its
    // death time the third and second of the queue, its corpse's objects
    // and the first (c-RZJ-1: the first Water Elemental at 62.298977 and
    // 63.798252).
    if state.table.expiring == Some(h) && state.unit(Some(h)).is_some_and(|u| u.illusion.is_none())
    {
        let record = state.table.take(tick, "death order record");
        let queue = state.table.take_n(tick, 7, "death order queue");
        state.table.defer(record, "death order record");
        for &e in queue[4..].iter().rev() {
            state.table.defer(e, "death order queue");
        }
        let (buffs, death, corpse) = entries(state, h)
            .map(|e| {
                (
                    std::mem::take(&mut e.aura_buffs),
                    std::mem::take(&mut e.death),
                    std::mem::take(&mut e.corpse),
                )
            })
            .unwrap_or_default();
        for (_, b) in buffs {
            state.table.defer(b, "aura buff");
        }
        for e in death {
            state.table.defer(e, "unit death");
        }
        // The fourth after the death's events (c-SBX-1: 1517 at 43.449436,
        // after the event object of the raised kobold's death).
        state.table.rest_after.push(queue[3]);
        let at = state.event_time.unwrap_or_else(|| state.timers.time());
        let time = state
            .unit(Some(h))
            .and_then(|u| state.data.units.get(&u.type_id))
            .map_or(3.0f32.to_bits(), |t| t.death_time);
        let later = ow3_math::add(at, time);
        for &e in [queue[2], queue[1]]
            .iter()
            .chain(&corpse)
            .chain([&queue[0]])
        {
            state.table.free_at_q(later, e, "summon: corpse");
        }
        return;
    }
    // A Mirror Image copy: a record and 7 queue objects; freed at once
    // the record, the last 3 of the queue, the first 5 objects it kept
    // from its making and the fourth of the queue; the first 3 of the
    // queue at the end of its death time (c-SAK-1: the image 20 at 96.3 s,
    // 6986 .. 6993, the 3 before 98.25 s).
    // PROVISIONAL: what the second stage frees beyond those 3 is not read.
    if state.unit(Some(h)).is_some_and(|u| u.illusion.is_some()) {
        let record = state.table.take(tick, "death order record");
        let queue = state.table.take_n(tick, 7, "death order queue");
        state.table.defer(record, "death order record");
        for &e in queue[4..].iter().rev() {
            state.table.defer(e, "death order queue");
        }
        let copy = state.table.copies.remove(&h).unwrap_or_default();
        for &e in copy.iter().take(5) {
            state.table.defer(e, "Mirror Image: copy");
        }
        state.table.defer(queue[3], "death order queue");
        // At the end of its type's death time (the Blademaster's 1.9 s).
        let at = state.event_time.unwrap_or_else(|| state.timers.time());
        let death = state
            .unit(Some(h))
            .and_then(|u| state.data.units.get(&u.type_id))
            .map_or(3.0f32.to_bits(), |t| t.death_time);
        let later = ow3_math::add(at, death);
        for &e in queue[..3].iter().rev() {
            state.table.free_at_q(later, e, "death order queue");
        }
        state
            .table
            .copies
            .insert(h, copy.into_iter().skip(5).collect());
        return;
    }
    // Its death order: a record and 9 queue objects, and for a unit not
    // of the creeps 1 more (SPEC-0024, "Unit death").
    // A hero's: 3 more before them, freed with the record (c-SBP-1: the
    // paladins at 6.2 and 6.95).
    // PROVISIONAL: read for heroes on hold position killed by KillUnit.
    // PROVISIONAL: a hero of the creeps is not read.
    let hero = owner != super::creeps::CREEPS
        && state
            .unit(Some(h))
            .is_some_and(|u| crate::data::is_hero(u.type_id));
    let hero_order = hero.then(|| state.table.take_n(tick, 3, "hero death order"));
    // A paused unit, not a hero, with a costed ability, whose watches the
    // pause freed: a watch on its mana, freed first of its own (c-SBL-1,
    // c-SBN-1: the ogre lord with Shockwave, 1503 at 11.569, 1502 at
    // 11.689; none for c-RZJ-1's Ogre Lord, not paused, at 37.137806).
    // PROVISIONAL: what the watch is.
    let costed = state
        .unit(Some(h))
        .filter(|u| u.paused && !crate::data::is_hero(u.type_id))
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| {
            t.abilities.iter().any(|a| {
                state
                    .data
                    .abilities
                    .get(a)
                    .and_then(|d| d.per_level.first())
                    .is_some_and(|l| l.cost > 0)
            })
        });
    let watch = costed.then(|| state.table.take(tick, "death mana watch"));
    let record = state.table.take(tick, "death order record");
    let queue = state.table.take_n(tick, 9, "death order queue");
    // (Defend's order at the death: only a unit with Defend; c-RXR-1:
    // none for the grunt dead at 12.888838.)
    let defends = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| {
            t.abilities.iter().any(|&a| {
                state
                    .data
                    .abilities
                    .get(&a)
                    .is_some_and(|d| d.code == i32::from_be_bytes(*b"Adef"))
            })
        });
    let other =
        (owner != super::creeps::CREEPS && defends).then(|| state.table.take(tick, "death order"));
    let held = entries(state, h).map(|e| {
        let death = std::mem::take(&mut e.death);
        // The last two of a creep's (its pair) before its order, the
        // first two after all (c-SAK-1: kobold 9 at 2753).
        // A peasant's three all after (c-RXD-1: 972, 963, 964 at 33.315006).
        let n = if death.len() == 3 {
            3
        } else {
            death.len().min(2)
        };
        let (core, pair) = death.split_at(n);
        let mut core = core.to_vec();
        // A hero's: its level's parts, then the two (c-SBP-1: the paladin
        // at 6.2, 1211, 1212, 1202, 1203).
        if hero {
            core.splice(0..0, std::mem::take(&mut e.level));
        }
        if let Some(m) = e.death_mana.take() {
            core.insert(core.len().min(1), m);
        }
        if let Some(w) = watch {
            core.insert(0, w);
        }
        // The objects of an order of the game and a stun's buff end go
        // before its stun's order, the rest of its own after the record
        // (c-SAK-1: gnoll 16 at 6130, 2625, 2643, 3439, then 4929 ..).
        let mut game = std::mem::take(&mut e.game_objects);
        game.extend(e.buff_end.take());
        // So does the order object a stunned unit keeps (c-RXD-1: the peasant
        // bashed and killed at 33.314907, his return order's 1649).
        let mut order = std::mem::take(&mut e.order);
        if !e.stun_order.is_empty() {
            game.append(&mut order);
        }
        let own: Vec<u32> = std::mem::take(&mut e.reaction)
            .into_iter()
            .chain(e.current.take())
            .chain(
                std::mem::take(&mut e.target_death)
                    .into_iter()
                    .map(|(_, o)| o),
            )
            .chain(std::mem::take(&mut e.acquisition))
            .chain(order)
            .collect();
        e.own_attack = false;
        // A stun's objects: its order's after the pair, its own after the
        // queue's (c-SAK-1: 3509).
        // A missile's objects stay to the stun's own end (c-SAK-1: kobold
        // 14 dead at 4700, the bolt's objects freed at 5513).
        let end_tick = e.stun_ticks.take().map(|(end, _)| end);
        let stun_order = std::mem::take(&mut e.stun_order);
        let mut stun_end = std::mem::take(&mut e.stun_end);
        let missile: Vec<u32> = if stun_end.len() > 1 {
            stun_end.split_off(1)
        } else {
            Vec::new()
        };
        (
            game,
            pair.to_vec(),
            own,
            core,
            stun_order,
            stun_end,
            missile,
            end_tick,
        )
    });
    let Some((game, pair, own, core, stun_order, mut stun_end, missile, end_tick)) = held else {
        return;
    };
    match end_tick {
        Some(end) if end != u64::MAX => {
            for e in missile {
                state.table.free_at(end, e, "missile");
            }
        }
        // Killed by the hit that stuns it: they wait for the stun's end
        // (`stunned_dead`).
        None if !missile.is_empty() => {
            state.table.dead_missiles.insert(h, missile);
        }
        _ => {}
    }
    // A creep's ability parts going first (`death_first`).
    let (first, late) = entries(state, h)
        .map(|e| {
            (
                std::mem::take(&mut e.death_first),
                std::mem::take(&mut e.death_late),
            )
        })
        .unwrap_or_default();
    for e in first {
        state.table.defer(e, "unit death");
    }
    for e in pair.into_iter().chain(game).chain(stun_order) {
        state.table.defer(e, "unit death");
    }
    // A hero's own objects go after the first of its three, before the
    // record (c-SBQ-1: the Paladin's acquisition objects at 9.329535).
    let mut own = own;
    if let Some(o) = &hero_order {
        state.table.defer(o[0], "hero death order");
        for e in std::mem::take(&mut own) {
            state.table.defer(e, "unit death");
        }
    }
    state.table.defer(record, "death order record");
    if let Some(o) = &hero_order {
        state.table.defer(o[2], "hero death order");
        state.table.defer(o[1], "hero death order");
    }
    for e in own {
        state.table.defer(e, "unit death");
    }
    for &e in queue[6..].iter().rev() {
        state.table.defer(e, "death order queue");
    }
    // A paused unit's stun object goes with them (c-SBL-1: the ogre killed
    // by Storm Bolt, 1333 at 5.279955).
    if paused {
        for e in std::mem::take(&mut stun_end) {
            state.table.defer(e, "unit death");
        }
    }
    // Its buffs, then the Defend order, with the rest after the levels its
    // death gives (c-RZJ-1: footman 5 at 15.70, its Blizzard buff 2877,
    // its Devotion buff 1446, then 2890, then its own).
    // (Only one still on: its timer not run yet.)
    if let Some((b, end)) = entries(state, h).and_then(|e| e.blizzard_buff.take())
        && state.table.timed_q.iter().any(|t| t.1 == b && t.0 == end)
    {
        state.table.timed_q.retain(|t| !(t.1 == b && t.0 == end));
        state.table.rest.push((b, "Blizzard: buff"));
    }
    // A Shockwave's hit buff still on goes there too (c-RZJ-1: footman
    // 4409 at 32.610394, before his Devotion buff).
    let shocks: Vec<u32> = state
        .table
        .shock_buffs
        .iter()
        .filter(|b| b.1 == h)
        .map(|b| b.2)
        .filter(|o| {
            state
                .table
                .timed_q
                .iter()
                .any(|t| t.1 == *o && t.2 == "Shockwave: hit buff")
        })
        .collect();
    for o in shocks {
        state
            .table
            .timed_q
            .retain(|t| !(t.1 == o && t.2 == "Shockwave: hit buff"));
        state.table.shock_buffs.retain(|b| b.2 != o);
        state.table.rest.push((o, "Shockwave: hit buff"));
    }
    // The newest first (c-SBL-1: kobold 3's two, 1253 then 1250).
    let auras = entries(state, h).map(|e| std::mem::take(&mut e.aura_buffs));
    for (_, b) in auras.unwrap_or_default().into_iter().rev() {
        state.table.rest.push((b, "aura buff"));
    }
    for e in late {
        state.table.rest.push((e, "unit death"));
    }
    if let Some(o) = other {
        state.table.rest.push((o, "death order"));
    }
    // The rest go after the levels its experience gives (`death_rest`;
    // c-SAK-1: ogre 13 at 6052, both heroes' levels before 1143, 1144).
    for e in stun_end {
        state.table.rest.push((e, "unit death"));
    }
    // A thunder clap's slow on it goes with its death, before its own
    // objects (c-RZH-1: tunneler 10's 3040 at 28.49).
    if let Some(b) = entries(state, h).and_then(|e| e.slow_buff.take()) {
        state.table.rest.push((b, "Thunder Clap effect"));
    }
    if let Some(e) = entries(state, h) {
        e.slow_end_q = None;
    }
    for e in core {
        state.table.rest.push((e, "unit death"));
    }
    // The fourth from the last after them (c-SAK-1: 2851 at 2753), and after
    // the death's events (c-SBL-1: 1295 after 1306, 1305 at 2.354989).
    state.table.rest_after.push(queue[5]);
    // Of the first 5 queue objects 2 go at the end of the unit's death
    // time (`death` of its type, from the death), 1 2.0 s after that, 2
    // stay (c-SAK-1: 2753 -> 3353, 3353, 3753; footman 7's death time
    // 3.04 at 4764 -> 5372, 5772).
    // PROVISIONAL: which of the five.
    let at = state.event_time.unwrap_or_else(|| state.timers.time());
    let death = state
        .unit(Some(h))
        .and_then(|u| state.data.units.get(&u.type_id))
        .map_or(3.0f32.to_bits(), |t| t.death_time);
    let three = ow3_math::add(at, death);
    let five = ow3_math::add(three, 2.0f32.to_bits());
    if hero {
        hero_dead(state, h, &queue, three);
        return;
    }
    state.table.rest_q.push((three, queue[4]));
    state.table.rest_q.push((three, queue[3]));
    state.table.rest_q.push((five, queue[2]));
    // Its corpse goes at the end of its decay, DecayTime and BoneDecayTime
    // on (2 and 88 s), a timer of the world clock: the second of the queue,
    // the objects of its making it still has, the first of the queue
    // (c-SAK-1: kobold 9 at 106.758; footman 5's at 120.2495 after the
    // turn checksum of 120.15, the queue time ahead of the world clock).
    // A hero leaves no corpse that decays.
    let corpse = entries(state, h)
        .map(|e| std::mem::take(&mut e.corpse))
        .unwrap_or_default();
    if !state
        .unit(Some(h))
        .is_some_and(|u| crate::data::is_hero(u.type_id))
    {
        let gone = ow3_math::add(five, (CORPSE_DECAY - 2.0).to_bits());
        // A unit with Defend: the ability, taken away with the unit, orders
        // its end, an object freed after the ability's (c-SAK-1: footman 7
        // at 116.859).
        let defend = state
            .unit(Some(h))
            .and_then(|u| state.data.units.get(&u.type_id))
            .is_some_and(|t| {
                t.abilities.iter().any(|a| {
                    state.data.abilities.get(a).map_or(*a, |d| d.code)
                        == i32::from_be_bytes(*b"Adef")
                })
            });
        if let Some(e) = entries(state, h) {
            e.dead_queue = queue[..5].to_vec();
            e.dead_corpse = Some((gone, corpse.clone()));
        }
        state.table.rest_w.push((gone, queue[1]));
        for (k, e) in corpse.into_iter().enumerate() {
            state.table.rest_w.push((gone, e));
            if k == 0 && defend {
                state.table.rest_w.push((gone, TAKE_AT));
            }
        }
        state.table.rest_w.push((gone, queue[0]));
    }
}

/// In `ObjectTable::flush_q`, an object to be made at the timer and freed
/// in its place.
const TAKE_AT: u32 = u32::MAX;
/// The keys of `ObjectTable::take_at_q`, and its mark for the making.
const MADE_KEY: u32 = 0x2000_0000;
const MADE_NOW: u32 = 0x4000_0000;

/// DecayTime + BoneDecayTime of the game's constants, from the end of the
/// death time (c-SAK-1: 2 + 88).
/// PROVISIONAL: read for the creeps of c-SAK-1 only.
const CORPSE_DECAY: f32 = 90.0;

/// The last frees of a death (`unit_died`), after the levels its
/// experience gave.
/// The death queue objects left for after the death's events.
pub(super) fn death_after(state: &mut super::State) {
    for e in std::mem::take(&mut state.table.rest_after) {
        state.table.defer(e, "death order queue");
    }
}

pub(super) fn death_rest(state: &mut super::State) {
    for (e, tag) in std::mem::take(&mut state.table.rest) {
        state.table.defer(e, tag);
    }
    for (q, e) in std::mem::take(&mut state.table.rest_q) {
        state.table.free_at_q(q, e, "death order queue");
    }
    for (c, e) in std::mem::take(&mut state.table.rest_w) {
        state.table.free_at_clock(c, e, "corpse");
    }
}

/// The number of objects a native of the script takes when called
/// (SPEC-0024, the analyst's reading of c-SAK-1's map start), for the
/// natives whose objects do not depend on their arguments.
///
/// PROVISIONAL: read at the map start of c-SAK-1 only.
pub(super) fn native_objects(name: &str) -> usize {
    match name {
        "CreateTimer"
        | "TimerStart"
        | "TriggerRegisterUnitEvent"
        | "TriggerRegisterPlayerUnitEvent"
        | "TriggerRegisterEnterRegion"
        | "TriggerRegisterTimerEvent" => 1,
        "TriggerAddAction" | "CreateRegion" => 2,
        "CreateTrigger" => 3,
        _ => 0,
    }
}

/// Unit `h` is paused by a script (c-SBL-1: the creeps at the map start):
/// an order of its own, as a stun's order without the stun object. The
/// order and its record; 9 parts, the last 4 freed 0.0001 s later from the
/// last, the first 5 kept with the order (freed from the last, then the
/// order, when the order ends; c-SBL-1: at the creep's death, 2.359989).
///
/// PROVISIONAL: an unpause is not read.
pub(super) fn paused(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    // Its mana watches go after them: a paused unit casts nothing (c-SBL-1: the
    // ogre lord's Shockwave watch, 1038).
    let watches = entries(state, h)
        .map(|e| {
            let w: Vec<u32> = std::mem::take(&mut e.mana_watches)
                .into_iter()
                .map(|(_, o, _)| o)
                .collect();
            e.death_late.retain(|o| !w.contains(o));
            w
        })
        .unwrap_or_default();
    let order = state.table.take(tick, "pause: order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    // The order it had ends (c-SBQ-1: the ogre paused in the Paladin's death
    // event, its script attack's 1130 .. 1126), but the order's own object
    // stays (1125, kept to the game's end).
    // PROVISIONAL: what keeps it is not read.
    // (Also when the order lost its target and waits for its backswing's end.)
    let kept_order = entries(state, h).and_then(|e| {
        if e.order.is_empty() {
            e.order_ending.pop().map(|(q, o)| (Some(q), o))
        } else {
            e.order.pop().map(|o| (None, o))
        }
    });
    if let Some((Some(q), o)) = kept_order {
        state
            .table
            .timed_q
            .retain(|t| *t != (q, o, "attack: order end"));
    }
    let kept_order = kept_order.map(|(_, o)| o);
    order_ended(state, h);
    if let (Some(o), Some(e)) = (kept_order, entries(state, h)) {
        e.game_objects.push(o);
    }
    let parts = state.table.take_n(tick, 9, "pause: order objects");
    for &e in parts[5..].iter().rev() {
        state.table.defer(e, "pause: order objects");
    }
    // A unit not of the creeps with Defend takes one more, kept, as at its
    // death (c-SBU-1: the footman paused at the map start, 1049).
    // PROVISIONAL: when it goes is not read.
    let defends = state
        .unit(Some(h))
        .filter(|u| u.owner != super::creeps::CREEPS)
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| {
            t.abilities.iter().any(|&a| {
                state
                    .data
                    .abilities
                    .get(&a)
                    .is_some_and(|d| d.code == i32::from_be_bytes(*b"Adef"))
            })
        });
    if defends {
        state.table.take(tick, "pause: Defend's order");
    }
    if let Some(e) = entries(state, h) {
        let mut kept: Vec<u32> = parts[..5].iter().rev().copied().collect();
        kept.push(order);
        // Kept as an order of the game's: at the death before the record
        // (c-SBL-1: kobold 3 at 2.354989).
        kept.append(&mut e.game_objects);
        e.game_objects = kept;
    }
    for o in watches {
        state.table.defer(o, "mana watch");
    }
}

/// Unit `h` is made invulnerable by a script, or no longer (`on`): one
/// object while it lasts, freed 0.0001 s later at its end (c-SBL-1: 1145,
/// from the map start to 2.0 s).
pub(super) fn invulnerable_set(state: &mut super::State, h: u32, on: bool) {
    let tick = state.table.now;
    if on {
        let o = state.table.take(tick, "invulnerability");
        if let Some(e) = entries(state, h) {
            e.invulnerable = Some(o);
        }
        // The units attacking it lose it: their acquisition's objects go
        // (c-SBQ-1: the Archmage on the ogre made invulnerable in the
        // Paladin's death event, 1323 .. 1317).
        // PROVISIONAL: read for one attacker standing on hold position.
        let attackers: Vec<u32> = state
            .units
            .iter()
            .copied()
            .filter(|&a| {
                a != h
                    && state.unit(Some(a)).is_some_and(|u| {
                        u.alive
                            && (u.attack.is_some_and(|t| t.target == h)
                                || u.movement.is_some_and(|m| {
                                    m.order == super::movement::Order::Attack { target: h }
                                }))
                    })
            })
            .collect();
        // One past its swing's damage point keeps the swing to the end of its
        // backswing, its acquisition check 0.01 s after (hub #6).
        let now = state.event_time.unwrap_or_else(|| state.timers.time());
        for a in attackers {
            // (In its backswing: past the damage point.)
            let swing_end = state
                .unit(Some(a))
                .filter(|u| f32::from_bits(u.backswing_end) > f32::from_bits(now))
                .map(|u| u.backswing_end);
            order_ended_with(state, a, swing_end);
            if let Some(super::Object::Unit(u)) = state.objects.get_mut(&a) {
                u.attack = None;
                u.movement = None;
                if let Some(end) = swing_end {
                    u.next_acquire = ow3_math::add(end, super::movement::AFTER_DEATH);
                }
            }
        }
    } else if let Some(o) = entries(state, h).and_then(|e| e.invulnerable.take()) {
        state.table.defer(o, "invulnerability");
    }
}

/// A player's hero dead, at the end of its death time `end`: 5 objects
/// of its state are made (a state change, a path agent's, a rate part
/// and two limit watches), then the third to fifth of its death queue go;
/// the last watch 0.5 s later; the first two of the queue 3.0 s after the
/// end, then the other four (c-SBP-1: the paladin dead at 6.199680, 1650
/// .. 1654 at 7.699680, 1654 at 8.19978, the rest at 10.6997).
///
/// PROVISIONAL: what the 3.0 s is, and what a revival does with them.
fn hero_dead(state: &mut super::State, h: u32, queue: &[u32], end: u32) {
    // It joins its player's list of dead heroes then (c-SBP-1: G changed
    // at 7.69968).
    if let Some(e) = entries(state, h) {
        e.dead_listed = Some(end);
    }
    let made: Vec<u32> = (0..5)
        .map(|_| state.table.take_at_q(end, "dead hero state"))
        .collect();
    for k in [4, 3, 2] {
        state.table.rest_q.push((end, queue[k]));
    }
    let half = ow3_math::add(end, 0.5f32.to_bits());
    state.table.rest_q.push((half, made[4]));
    let later = ow3_math::add(end, 3.0f32.to_bits());
    for e in [queue[1], queue[0], made[3], made[1], made[0], made[2]] {
        state.table.rest_q.push((later, e));
    }
}
/// A unit is put somewhere by `SetUnitPosition`: an object, an order's
/// record and five order objects, freed 0.0001 s later, the record first,
/// the five from the last, the object last (c-SBU-1: the revived Paladin
/// at 51.75, 1010, 906, 844 .. 932).
///
/// PROVISIONAL: read once, on a hero standing without an order.
pub(super) fn position_set(state: &mut super::State) {
    let tick = state.table.now;
    let first = state.table.take(tick, "position: order");
    let record = state.table.take(tick, "order record");
    let parts = state.table.take_n(tick, 5, "position: order objects");
    state.table.defer(record, "order record");
    for &o in parts.iter().rev() {
        state.table.defer(o, "position: order objects");
    }
    state.table.defer(first, "position: order");
}

/// Dead unit `h` is brought back by a Resurrection (c-SBV-1 at 3.5): a limit
/// watch and a rate part of its state, a Defend order for a unit with
/// Defend, its start order's five; then, 0.0001 s later, the Defend order,
/// the five of its death order's queue still held (the last first) and
/// the start order's (the last first). Its corpse no longer decays: the
/// objects of its making stay with it.
///
/// PROVISIONAL: what the watch and the rate part are, and when they go,
/// is not read.
pub(super) fn resurrected(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let (queue, corpse) = held_dead(state, h);
    state.table.take_n(tick, 2, "resurrection: unit state");
    let defends = state
        .unit(Some(h))
        .filter(|u| u.owner != super::creeps::CREEPS)
        .and_then(|u| state.data.units.get(&u.type_id))
        .is_some_and(|t| {
            t.abilities.iter().any(|a| {
                state.data.abilities.get(a).map_or(*a, |d| d.code) == i32::from_be_bytes(*b"Adef")
            })
        });
    let defend = defends.then(|| state.table.take(tick, "resurrection: Defend's order"));
    let order = state.table.take_n(tick, 5, "resurrection: start order");
    if let Some(d) = defend {
        state.table.defer(d, "resurrection: Defend's order");
    }
    for &o in queue.iter().rev() {
        state.table.defer(o, "death order queue");
    }
    for &o in order.iter().rev() {
        state.table.defer(o, "resurrection: start order");
    }
    if let (Some((_, list)), Some(e)) = (corpse, entries(state, h)) {
        e.corpse = list;
    }
}

/// Hero `h`'s Avatar takes effect at `q` (c-SBV-1 at 4.5): its effect's
/// event, an object kept to its end, a rate part of the hero's state freed
/// 1.0 s later, a limit watch kept to its end. The watch on the cost of
/// `ability` goes 0.0001 s later, after the effect's event.
///
/// PROVISIONAL: what the kept object and the watch are is not read.
pub(super) fn avatar_effect(state: &mut super::State, h: u32, ability: i32, q: u32) {
    let tick = state.table.now;
    spell_effect_event(state, h);
    let kept = state.table.take(tick, "Avatar: buff");
    let rate = state.table.take(tick, "Avatar: rate part");
    let watch = state.table.take(tick, "Avatar: limit watch");
    state.table.free_at_q(
        ow3_math::add(q, 1.0f32.to_bits()),
        rate,
        "Avatar: rate part",
    );
    let cost = state
        .data
        .abilities
        .get(&ability)
        .and_then(|d| d.per_level.first())
        .map_or(0, |l| l.cost);
    let old = entries(state, h).and_then(|e| {
        let k = e.mana_watches.iter().position(|w| w.0 == cost)?;
        let w = e.mana_watches.remove(k).1;
        e.watch_checks.retain(|&(x, _)| x != w);
        Some(w)
    });
    if let Some(w) = old {
        state.table.defer(w, "unit state: limit watch");
    }
    if let Some(e) = entries(state, h) {
        e.avatar = vec![watch, kept];
    }
}

/// Hero `h`'s Avatar ends (c-SBV-1 at 64.5): a rate part and two limit
/// watches, the second the watch on the cost of `ability` again; its kept
/// two go 0.0001 s later.
///
/// PROVISIONAL: what the first watch is and when it goes is not read.
pub(super) fn avatar_ended(state: &mut super::State, h: u32, ability: i32) {
    let tick = state.table.now;
    // Its rate part goes 1.0 s later, as at the start (c-SBV-1: 2095 at
    // 65.49786).
    let rate = state.table.take(tick, "Avatar end: rate part");
    // (By the world clock: 1.0001 s after the clock of the tick.)
    let at = ow3_math::add(state.move_clock, 1.0001f32.to_bits());
    state.table.free_at_clock(at, rate, "Avatar end: rate part");
    state.table.take(tick, "Avatar end: limit watch");
    let watch = state.table.take(tick, "unit state: limit watch");
    let cost = state
        .data
        .abilities
        .get(&ability)
        .and_then(|d| d.per_level.first())
        .map_or(0, |l| l.cost);
    let mana = state
        .unit(Some(h))
        .map_or(0.0, |u| f32::from_bits(u.mana.value()));
    let kept = entries(state, h).map(|e| {
        e.mana_watches.push((cost, watch, mana >= cost as f32));
        std::mem::take(&mut e.avatar)
    });
    if mana < cost as f32 {
        watch_scheduled(state, h, watch);
    }
    for o in kept.unwrap_or_default() {
        state.table.defer(o, "Avatar: end");
    }
}

/// An Earthquake's area acts the first time (c-SBW-1 at 3.8, hub #10): its
/// five objects, kept to its last action.
pub(super) fn quake_area(state: &mut super::State) -> Vec<u32> {
    let tick = state.table.now;
    state.table.take_n(tick, 5, "Earthquake: area")
}

/// Unit `h` comes into an Earthquake: its object, kept while it stays, then,
/// walking, its walk made again at the slowed speed (c-SBW-1 at 3.8 and
/// 4.8). It goes with the unit at its death.
pub(super) fn quake_entered(state: &mut super::State, h: u32) -> u32 {
    let tick = state.table.now;
    let o = state.table.take(tick, "Earthquake: unit");
    quake_renewed(state, h, true);
    if let Some(e) = entries(state, h) {
        e.death.push(o);
    }
    o
}

/// An Earthquake's action finds unit `h` in it: its slow is set anew and,
/// walking, its walk is made again (c-SBW-1: kobold 5 at 4.799891, 1269 ..
/// 1271, hub #10). Its search comes at its next update and takes none.
pub(super) fn quake_renewed(state: &mut super::State, h: u32, fresh: bool) {
    let walking = entries(state, h).is_some_and(|e| !e.walk.is_empty());
    if walking {
        walk_made_at_order(state, h);
        // (One already slowed keeps its speed.)
        if fresh {
            super::movement::slowed_now(state, h);
        }
        super::movement::chase_renewed(state, h, fresh);
    }
}

/// Unit `h` left an Earthquake: its object goes (c-SBW-1: 1215 at 10.8).
pub(super) fn quake_left(state: &mut super::State, h: u32, o: u32) {
    if let Some(e) = entries(state, h) {
        e.death.retain(|&d| d != o);
    }
    state.table.defer(o, "Earthquake: unit");
}

/// An Earthquake's last action (c-SBW-1 at 27.8): its objects go, the
/// first, the fourth, the fifth, the third, the second, then its units'.
pub(super) fn quake_ended(state: &mut super::State, objects: &[u32], inside: &[(u32, u32)]) {
    for k in [0, 3, 4, 2, 1] {
        if let Some(&o) = objects.get(k) {
            state.table.defer(o, "Earthquake: area");
        }
    }
    for &(h, o) in inside {
        quake_left(state, h, o);
    }
}

/// Dead unit `h` is raised by Animate Dead (c-SBX-1 at 3.449962): two rate
/// parts and a limit watch of its state, its start order's five, and one
/// object kept while it lives; then, 0.0001 s later, the five of its
/// death order's queue still held (the last first) and the start order's
/// (the last first). Its corpse no longer decays.
///
/// PROVISIONAL: what the state's three and the kept object are is not
/// read; the kept one is taken to go at its death.
pub(super) fn animated(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let (queue, corpse) = held_dead(state, h);
    // The first goes 3.0001 s on by the world clock (c-SBX-1: 1152, 1161, 1170
    // at 6.450062).
    let parts = state.table.take_n(tick, 3, "Animate Dead: unit state");
    let at = ow3_math::add(state.move_clock, 3.0001f32.to_bits());
    state
        .table
        .free_at_clock(at, parts[0], "Animate Dead: unit state");
    let order = state.table.take_n(tick, 5, "Animate Dead: start order");
    let kept = state.table.take(tick, "Animate Dead: unit");
    for &o in queue.iter().rev() {
        state.table.defer(o, "death order queue");
    }
    for &o in order.iter().rev() {
        state.table.defer(o, "Animate Dead: start order");
    }
    if let Some(e) = entries(state, h) {
        if let Some((_, list)) = corpse {
            e.corpse = list;
        }
        e.death.push(kept);
        // Then its state's third and second (c-SBX-1: 1160, 1154, 1153 at
        // 43.449436).
        e.death.push(parts[2]);
        e.death.push(parts[1]);
    }
}

/// The death-queue objects dead unit `h` still holds and its corpse's,
/// their timers cancelled, for a unit brought back.
fn held_dead(state: &mut super::State, h: u32) -> (Vec<u32>, Option<(u32, Vec<u32>)>) {
    let (queue, corpse) = entries(state, h)
        .map(|e| (std::mem::take(&mut e.dead_queue), e.dead_corpse.take()))
        .unwrap_or_default();
    state.table.timed_q.retain(|t| !queue.contains(&t.1));
    if let Some((gone, list)) = &corpse {
        state.table.timed_w.retain(|t| {
            !(t.0 == *gone && (t.1 == TAKE_AT || list.contains(&t.1) || queue.contains(&t.1)))
        });
    }
    (queue, corpse)
}

/// An Earthquake has units in it again: an object it holds while they are
/// (c-SBW-1: 1215 at 3.8).
pub(super) fn quake_occupied(state: &mut super::State) -> u32 {
    let tick = state.table.now;
    state.table.take(tick, "Earthquake: occupied")
}

/// A Death and Decay takes effect (c-SBX-1 at 4.9, hub #12): its area's
/// eight objects, the others freed 5.0 s after its end; the last, returned,
/// goes at its end (`cast::decays`), before the channel's order.
pub(super) fn decay_area(state: &mut super::State, end: u32) -> u32 {
    let tick = state.table.now;
    let objects = state.table.take_n(tick, 8, "Death and Decay: area");
    let later = ow3_math::add(end, 5.0f32.to_bits());
    // In this order (c-SBX-1: 1285, 1286, 1284, 1282, 1283, 1281, 1280 at
    // 44.998585).
    for k in [5, 6, 4, 2, 3, 1, 0] {
        state
            .table
            .free_at_q(later, objects[k], "Death and Decay: area");
    }
    objects[7]
}

/// A Tranquility heals a unit (c-SBY-1: the Priestess's 1386, 1387, 1388 at
/// 10.209288, hub #11): its heal's object, a limit watch on its life and
/// a rate part, all kept to the channel's end.
pub(super) fn tranquility_heal(state: &mut super::State) -> (u32, u32, u32) {
    let tick = state.table.now;
    let object = state.table.take(tick, "Tranquility: heal");
    let watch = state.table.take(tick, "unit state: limit watch");
    let rate = state.table.take(tick, "unit state: rate part");
    (object, watch, rate)
}

/// A unit's life reached its maximum: its limit watch `old` is made anew
/// for the other direction, the old freed 0.0001 s later (c-SBY-1: 1487
/// for 1387 at 38.425983, hub #11). The new watch.
pub(super) fn life_watch_renewed(state: &mut super::State, old: u32) -> u32 {
    let tick = state.table.now;
    let new = state.table.take(tick, "unit state: limit watch");
    state.table.defer(old, "unit state: limit watch");
    new
}

/// A Starfall's star streams start (c-SBY-1: 1198 .. 1201 at 3.509961, hub
/// #11): an object each, freed at its target's death (`cast::stream_target_died`).
pub(super) fn star_streams(state: &mut super::State, n: usize) -> Vec<u32> {
    let tick = state.table.now;
    state.table.take_n(tick, n, "Starfall: stream")
}

/// A Death and Decay's step 0.01 s after its effect: an object, freed at
/// `end` (c-SBX-1: 1288, 4.909883 to 5.399983).
/// PROVISIONAL: what it is is not read.
pub(super) fn decay_step(state: &mut super::State, end: u32) {
    let tick = state.table.now;
    let o = state.table.take(tick, "Death and Decay: step");
    state.table.free_at_q(end, o, "Death and Decay: step");
}

/// Worker `h` takes a build order (c-SBZ-1 at 1.999998, hub #13): the
/// order, its record (freed 0.0001 s later), twelve parts (the last six
/// freed 0.0001 s later, the first six kept with the order to the
/// construction's start), then its walk to the site.
pub(super) fn build_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "build order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let parts = state.table.take_n(tick, 12, "build order: order start");
    for &e in parts[6..].iter().rev() {
        state.table.defer(e, "build order: order start");
    }
    walk_made_at_order(state, h);
    if let Some(e) = entries(state, h) {
        let mut kept = vec![parts[5]];
        kept.extend(parts[..5].iter().rev());
        kept.push(order);
        e.build_order = kept;
    }
}

/// Worker `h` starts building `building` (c-SBZ-1 at 2.759979, hub #13):
/// the building's objects as a unit made by a script (its dispel waiting
/// for the completion), then the start's: one freed 0.0001 s later, the
/// construction-start event's, the construction's progress pair (kept to
/// the completion), three freed 0.0001 s later, the worker's work order
/// with eleven parts (four kept with it, one kept by the building to the
/// completion, six freed 0.0001 s later) and three more (two kept with
/// the order, one kept to the first helper). The build order's objects go
/// then: its sixth part first, before the building's.
///
/// This first part takes the building and the start's first two; the
/// construction-start event's triggers run after it, before the rest
/// (`construction_rest`; c-SCA-1: the script's timer 1101, 1102 after the
/// event's 1100, hub #14).
///
/// PROVISIONAL: the construction-start event's object is taken whatever
/// triggers are registered.
pub(super) fn construction_begun(state: &mut super::State, h: u32, building: u32) {
    let tick = state.table.now;
    if let Some(e) = entries(state, h)
        && !e.build_order.is_empty()
    {
        let first = e.build_order.remove(0);
        state.table.defer(first, "build order");
    }
    state.table.constructing = true;
    unit_made(state, building);
    state.table.constructing = false;
    let one = state.table.take(tick, "construction: start");
    let event = state.table.take(tick, "construction: event");
    state.table.defer(event, "construction: event");
    state.table.defer(one, "construction: start");
}

/// The rest of the start: the progress pair, three timers, the worker's
/// work order with its parts, and the construction's life rows. A worker
/// not yet facing the building (`turning`) takes a twelfth part (its
/// fifth, let go when it faces it) and a walk that turns it, and the life
/// rows come only then (`turned_to_work`; c-SCA-1: 1109 .. 1123, the rows
/// at 6.959734, hub #14).
pub(super) fn construction_rest(state: &mut super::State, h: u32, building: u32, turning: bool) {
    let tick = state.table.now;
    let order = entries(state, h)
        .map(|e| std::mem::take(&mut e.build_order))
        .unwrap_or_default();
    let progress = state.table.take_n(tick, 2, "construction: progress");
    let timer = state.table.take_n(tick, 3, "construction: timer");
    let work = state.table.take(tick, "work order");
    let mut parts = state.table.take_n(
        tick,
        if turning { 12 } else { 11 },
        "work order: order start",
    );
    let turn_part = turning.then(|| parts.remove(4));
    let more = if turning {
        Vec::new()
    } else {
        state.table.take_n(tick, 3, "work order: timer")
    };
    for &e in timer.iter().rev() {
        state.table.defer(e, "construction: timer");
    }
    for &e in &order {
        state.table.defer(e, "build order");
    }
    for &e in parts[5..].iter().rev() {
        state.table.defer(e, "work order: order start");
    }
    if let Some(e) = entries(state, h) {
        let mut kept = vec![work];
        kept.extend(&parts[..4]);
        kept.extend(more.iter().take(2));
        e.work = kept;
        e.turn_part = turn_part;
    }
    if let Some(e) = entries(state, building) {
        e.construction = progress;
        e.construction_work = Some(parts[4]);
        e.first_helper = more.get(2).copied();
    }
    if turning {
        walk_made(state, h);
    }
}

/// Worker `h` faces `building` and its work begins (c-SCA-1: 6.959734, two
/// updates after the start, hub #14): the construction's life rows, the
/// fifth part of its order let go, its turning walk freed at its next
/// update.
pub(super) fn turned_to_work(state: &mut super::State, h: u32, building: u32) {
    let tick = state.table.now;
    let more = state.table.take_n(tick, 3, "work order: timer");
    if let Some(e) = entries(state, h) {
        e.work.extend(&more[..2]);
        if let Some(o) = e.turn_part.take() {
            state.table.defer(o, "work order: order start");
        }
    }
    if let Some(e) = entries(state, building) {
        e.first_helper = Some(more[2]);
    }
    walk_dropped_at_update(state, h);
}

/// Building `building` under construction is cancelled (c-SCA-1 at
/// 7.399702, hub #14): the cancel order's eight objects, three of the
/// building's freed at once, the cancel event's two; then, 0.0001 s later
/// in this order, the cancel's record, the worker's life rate rows and
/// the first helper's part, the progress pair, the cancel order's own,
/// the building's watch, the event's, the work's part kept by the
/// building, the building's dispel and the rest of its making; the
/// worker's kept parts go with its next order. The event's two, for the
/// triggers between.
pub(super) fn construction_cancelled(
    state: &mut super::State,
    worker: u32,
    building: u32,
) -> [u32; 2] {
    let tick = state.table.now;
    let order = state.table.take(tick, "cancel order");
    let record = state.table.take(tick, "order record");
    let record2 = state.table.take(tick, "order record");
    let parts = state.table.take_n(tick, 5, "cancel order: parts");
    let made = entries(state, building)
        .map(|e| std::mem::take(&mut e.made))
        .unwrap_or_default();
    for k in [11, 12, 13] {
        if let Some(&e) = made.get(k) {
            state.table.free(tick, e, "construction: cancelled");
        }
    }
    let one = state.table.take(tick, "construction: cancel");
    let event = state.table.take(tick, "construction: event");
    state.table.defer(record, "order record");
    // The worker's life rate rows (two with its work, one kept for the
    // first helper) and the progress pair.
    let rows = entries(state, worker).map_or(Vec::new(), |e| {
        if e.work.len() == 7 {
            e.work.split_off(5)
        } else {
            Vec::new()
        }
    });
    for &e in rows.iter().rev() {
        state.table.defer(e, "unit state: rate part");
    }
    let (progress, first, kept, dispel) =
        entries(state, building).map_or((Vec::new(), None, None, Vec::new()), |e| {
            (
                std::mem::take(&mut e.construction),
                e.first_helper.take(),
                e.construction_work.take(),
                std::mem::take(&mut e.dispel),
            )
        });
    if let Some(f) = first {
        state.table.defer(f, "unit state: rate part");
    }
    for &e in progress.iter().rev() {
        state.table.defer(e, "construction: progress");
    }
    state.table.defer(record2, "order record");
    state.table.defer(order, "cancel order");
    state.table.defer(parts[4], "cancel order: parts");
    state.table.defer(parts[3], "cancel order: parts");
    if let Some(&e) = made.get(10) {
        state.table.defer(e, "construction: cancelled");
    }
    state.table.defer(event, "construction: event");
    state.table.defer(one, "construction: cancel");
    state.table.defer(parts[2], "cancel order: parts");
    state.table.defer(parts[1], "cancel order: parts");
    if let Some(k) = kept {
        state.table.defer(k, "work order");
    }
    for k in [20, 19] {
        if let Some(&e) = made.get(k) {
            state.table.defer(e, "construction: cancelled");
        }
    }
    for e in dispel {
        state.table.defer(e, "construction: cancelled");
    }
    for k in [8, 7, 6, 5, 4, 9, 2, 3, 1, 0] {
        if let Some(&e) = made.get(k) {
            state.table.defer(e, "construction: cancelled");
        }
    }
    state.table.defer(parts[0], "cancel order: parts");
    // The worker's kept parts of its work go with its next order, the
    // last first (1112 .. 1108).
    if let Some(e) = entries(state, worker) {
        let work = std::mem::take(&mut e.work);
        e.order = work.into_iter().rev().collect();
    }
    [one, event]
}

/// Creep `h`'s chase walk ended out of reach of a target it cannot act
/// against: its guard check gives the chase up (c-SCA-1: the turtle under
/// the archer's cliff at 10.649214, hub #14). The target's object goes,
/// three objects, for an attack-move a walk to its place, an object and the new
/// standing order's two records; 0.0001 s later in the game's order the
/// last acquisition's, the wake's part, a record, the old order's kept
/// parts and the order, the other record and the object.
pub(super) fn guard_given_up(state: &mut super::State, h: u32, moving_order: bool) {
    let tick = state.table.now;
    let acquisition = entries(state, h)
        .map(|e| {
            // An attack of its own acquisition: its target's three and its
            // order's three go in the place of the acquisition's, its
            // current target's first (c-SCA-1: 1280, then 1279 .. 1274 at
            // 11.549162, hub #14).
            if std::mem::take(&mut e.own_attack) {
                let mut all: Vec<u32> = std::mem::take(&mut e.target_death)
                    .into_iter()
                    .map(|(_, o)| o)
                    .collect();
                all.extend(std::mem::take(&mut e.order));
                all.extend(e.current.take());
                all
            } else {
                std::mem::take(&mut e.acquisition)
            }
        })
        .unwrap_or_default();
    if let Some(&last) = acquisition.last() {
        state.table.defer(last, "acquisition: extra object");
    }
    let three = state.table.take_n(tick, 3, "guard: given up");
    for &e in three.iter().rev() {
        state.table.defer(e, "guard: given up");
    }
    // An attack-move (the wake's) is ended with a walk made; a plain attack
    // (from an idle acquisition) without one (c-SCA-1: 10.649214 and
    // 11.549063, hub #14).
    // PROVISIONAL: the walk's cause is read from these two only.
    if moving_order {
        walk_made(state, h);
    }
    // It stands: the walk goes at its next update, before the older one
    // (1268, 1267, then 1226, 1225 at 10.679209).
    walk_dropped(state, h);
    let one = state.table.take(tick, "guard: given up");
    let record = state.table.take(tick, "order record");
    let record2 = state.table.take(tick, "order record");
    for &e in acquisition.iter().take(acquisition.len().saturating_sub(1)) {
        state.table.defer(e, "acquisition: attack object");
    }
    if let Some(p) = entries(state, h).and_then(|e| e.wake_part.take()) {
        state.table.defer(p, "creep: woken");
    }
    state.table.defer(record, "order record");
    let order = entries(state, h)
        .map(|e| std::mem::take(&mut e.order))
        .unwrap_or_default();
    for e in order {
        state.table.defer(e, "order: given up");
    }
    state.table.defer(record2, "order record");
    state.table.defer(one, "guard: given up");
}

/// Building `building` is finished at queue time `q` (c-SBZ-1: the
/// barracks at 44.043449, hub #13): an object and the construction-finish
/// event's, both freed at once after the construction's progress pair;
/// its dispel runs from 0.01 s after the completion.
pub(super) fn construction_finished(state: &mut super::State, building: u32, builder: u32, q: u32) {
    let tick = state.table.now;
    let type_id = state.unit(Some(building)).map_or(0, |u| u.type_id);
    let after = building_layout(state, type_id).map_or(0, |b| b.2.to_bits());
    let one = state.table.take(tick, "construction: finished");
    let event = state.table.take(tick, "construction: event");
    let (progress, dispel, rates) = entries(state, building)
        .map(|e| {
            (
                std::mem::take(&mut e.construction),
                std::mem::take(&mut e.dispel),
                std::mem::take(&mut e.construction_rates),
            )
        })
        .unwrap_or_default();
    // The builder's two work timers first (c-SBZ-1: the barracks' 1145,
    // 1144), its rate rows (1180; a takeover's three as its limit watch,
    // then its rate parts: the farm's 1234, 1233, 1235), then its progress
    // pair (hub #13).
    // PROVISIONAL: the order of the rows read on these two buildings.
    let timers = entries(state, builder)
        .filter(|e| e.work.len() == 7)
        .map(|e| e.work.split_off(5))
        .unwrap_or_default();
    for e in timers.into_iter().rev() {
        state.table.defer(e, "work order: timer");
    }
    let rates = match rates[..] {
        [a, b, c] => vec![b, a, c],
        _ => rates.into_iter().rev().collect(),
    };
    for e in rates {
        state.table.defer(e, "unit state: rate part");
    }
    for &e in progress.iter().rev() {
        state.table.defer(e, "construction: progress");
    }
    state.table.defer(event, "construction: event");
    state.table.defer(one, "construction: finished");
    let end = ow3_math::add(ow3_math::add(q, crate::timers::TICK), after);
    for e in dispel {
        state.table.free_at_q(end, e, "building: dispel");
    }
}

/// The work on building `building` ends two ticks after its completion
/// (c-SBZ-1: 44.054104): each worker's work order and kept parts in the
/// order of `workers`, the construction's kept part where `building`
/// stands among them.
pub(super) fn work_ended(state: &mut super::State, workers: &[u32], building: u32) {
    for &w in workers {
        if w == building {
            if let Some(o) = entries(state, building).and_then(|e| e.construction_work.take()) {
                state.table.defer(o, "work order");
            }
            continue;
        }
        let work = entries(state, w)
            .map(|e| std::mem::take(&mut e.work))
            .unwrap_or_default();
        for e in work.into_iter().rev() {
            state.table.defer(e, "work order");
        }
    }
}

/// The work that ends in this tick (`work_ended`).
pub(super) fn work_ends_due(state: &mut super::State) {
    let now = state.ticks;
    let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut state.table.work_ends)
        .into_iter()
        .partition(|w| w.0 <= now);
    state.table.work_ends = later;
    for (_, building, workers) in due {
        work_ended(state, &workers, building);
    }
}

/// Worker `h` takes a repair order on a building under construction
/// (c-SBZ-1: peasant 1 at 7.499695, hub #13): the order, its record (freed
/// 0.0001 s later), eleven parts (the last six freed 0.0001 s later, the
/// fifth at the start of its help, the first four kept with the order),
/// then its walk to the building.
pub(super) fn repair_ordered(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let order = state.table.take(tick, "repair order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let parts = state.table.take_n(tick, 11, "repair order: order start");
    for &e in parts[5..].iter().rev() {
        state.table.defer(e, "repair order: order start");
    }
    walk_made_at_order(state, h);
    if let Some(e) = entries(state, h) {
        let mut kept = vec![order];
        kept.extend(&parts[..4]);
        e.work = kept;
        e.repair_part = Some(parts[4]);
    }
}

/// Worker `h` starts helping on `building` (c-SBZ-1: peasant 1 at 8.489576,
/// hub #13): the building's life rate is set anew (a rate part, kept to
/// the completion), then the fifth part of the worker's order and, for the
/// first helper, the building's object kept for it go.
pub(super) fn helper_started(state: &mut super::State, h: u32, building: u32) {
    let tick = state.table.now;
    let rate = state.table.take(tick, "unit state: rate part");
    // It replaces the row of the last change, freed then (c-SBZ-1: 1163
    // by 1213 at 14.758524).
    if let Some(o) = entries(state, h).and_then(|e| e.repair_part.take()) {
        state.table.defer(o, "repair order: order start");
    }
    let old = entries(state, building).and_then(|e| e.construction_rates.pop());
    if let Some(o) = old {
        state.table.defer(o, "unit state: rate part");
    }
    if let Some(e) = entries(state, building) {
        e.construction_rates.push(rate);
    }
    if let Some(o) = entries(state, building).and_then(|e| e.first_helper.take()) {
        state.table.defer(o, "work order: timer");
    }
}

/// A helper takes over a building whose builder left, at its 1.0 s timer
/// (c-SBZ-1: peasant 2 at 19.757685, hub #13): its new work order. The
/// order.
pub(super) fn takeover_started(state: &mut super::State) -> u32 {
    let tick = state.table.now;
    state.table.take(tick, "work order")
}

/// The takeover of `building` by helper `h` ends one update later
/// (c-SBZ-1: 19.767683, hub #13): ten parts of its new work order (the last
/// six freed 0.0001 s later), the building's rate rows (a rate part, a
/// limit watch, a rate part, kept to the completion); its repair order's
/// kept parts and order go, then the new parts.
pub(super) fn takeover_done(state: &mut super::State, h: u32, building: u32, order: u32) {
    let tick = state.table.now;
    let parts = state.table.take_n(tick, 10, "work order: order start");
    let rates = state.table.take_n(tick, 3, "unit state: rate part");
    let repair = entries(state, h)
        .map(|e| std::mem::take(&mut e.work))
        .unwrap_or_default();
    for e in repair.into_iter().rev() {
        state.table.defer(e, "repair order");
    }
    for &e in parts[4..].iter().rev() {
        state.table.defer(e, "work order: order start");
    }
    if let Some(e) = entries(state, h) {
        let mut kept = vec![order];
        kept.extend(&parts[..4]);
        e.work = kept;
    }
    if let Some(e) = entries(state, building) {
        e.construction_rates.extend(rates);
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_freed_first() {
        let mut t = ObjectTable::new(821, vec![818, 805, 806], 821);
        assert_eq!(t.next_entry(), 806);
        assert_eq!(t.take(201, "a"), 806);
        assert_eq!(t.take(201, "a"), 805);
        assert_eq!(t.take(201, "a"), 818);
        assert_eq!(t.take(201, "a"), 821);
        assert_eq!(t.next_creation(), 825);
        t.free(202, 805, "a");
        t.free(202, 821, "a");
        assert_eq!(t.take(203, "a"), 821);
        assert_eq!(t.take(203, "a"), 805);
        assert_eq!(t.next_entry(), 822);
    }
}
