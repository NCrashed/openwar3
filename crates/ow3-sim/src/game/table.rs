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
}

#[derive(Clone, Debug, Default)]
pub struct ObjectTable {
    /// The number of entries the table has.
    size: u32,
    /// Free entries; the last one is taken next.
    free: Vec<u32>,
    /// The next creation number.
    next: u32,
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
    /// The spell order being taken starts its cast at once.
    pub cast_at_once: bool,
    /// A Wind Walk's fade is being handled (`walk_dropped`).
    pub at_fade: bool,
    /// The unit whose death is being handled, and the objects of its
    /// attackers going on with their attack-moves (`attack_move_resumed`).
    pub dying: Option<u32>,
    pub resumed: Vec<u32>,
    /// Objects freed in a later tick's batch (`free_at`).
    timed: Vec<(u64, u32, &'static str)>,
    timed_q: Vec<(u32, u32, &'static str)>,
    /// Units whose order ends at the end of the tick.
    pub ends_late: Vec<u32>,
    /// The takings and freeings, when kept.
    pub log: Option<Vec<TableEvent>>,
    /// The world clock at the start of each tick, kept with the log.
    pub clocks: Option<Vec<(u64, u32)>>,
}

impl ObjectTable {
    /// A table of `size` entries whose free list is `free` (taken from the
    /// end) and whose next creation number is `next`.
    pub fn new(size: u32, free: Vec<u32>, next: u32) -> ObjectTable {
        ObjectTable {
            size,
            free,
            next,
            live: Default::default(),
            deferred: Vec::new(),
            now: 0,
            day: Vec::new(),
            walks_due: Vec::new(),
            cast_at_once: false,
            at_fade: false,
            dying: None,
            resumed: Vec::new(),
            timed: Vec::new(),
            timed_q: Vec::new(),
            ends_late: Vec::new(),
            log: None,
            clocks: None,
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
            });
        }
    }

    /// `n` new objects take entries, in order; gives the entries.
    pub fn take_n(&mut self, tick: u64, n: usize, tag: &'static str) -> Vec<u32> {
        (0..n).map(|_| self.take(tick, tag)).collect()
    }

    /// The object holding `entry` is freed by a 0.0001 s timer of the
    /// world clock (`flush`).
    pub fn defer(&mut self, entry: u32, tag: &'static str) {
        self.deferred.push((self.now, entry, tag));
    }

    /// The object holding `entry` is freed by a timer of the queue expiring
    /// at `q` (`flush_q`).
    pub fn free_at_q(&mut self, q: u32, entry: u32, tag: &'static str) {
        self.timed_q.push((q, entry, tag));
    }

    /// The queue's timers of this tick, up to `limit`, free their objects.
    pub fn flush_q(&mut self, tick: u64, limit: u32) {
        let (due, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.timed_q)
            .into_iter()
            .partition(|&(q, _, _)| f32::from_bits(q) <= f32::from_bits(limit));
        self.timed_q = later;
        for (_, entry, tag) in due {
            self.free(tick, entry, tag);
        }
    }

    /// The object holding `entry` is freed in the 0.0001 s batch of tick
    /// `tick`.
    pub fn free_at(&mut self, tick: u64, entry: u32, tag: &'static str) {
        self.timed.push((tick, entry, tag));
    }

    /// The 0.0001 s timers of the tick expire: their objects are freed in
    /// the order they were given up.
    pub fn flush(&mut self, tick: u64) {
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
    /// Its walk was made at a backswing's end for the chase of the next
    /// tick, whose order keeps it.
    pub early_walk: bool,
    /// The goal after its swing was taken at the backswing's end; the
    /// chase takes no other.
    pub early_goal: bool,
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
    /// At a death being handled: the objects it kept to it, and whether
    /// its order ends too.
    pub lost: Vec<u32>,
    pub lose_order: bool,
    pub lose_acquisition: bool,
    /// It walks back to its guard point after its target's death.
    pub guard_walk: bool,
    /// Its order is one of the game (a creep's return home).
    pub game_order: bool,
    /// The order and the extra object of its order of the game.
    pub game_objects: Vec<u32>,
    /// Watches on its mana, one per ability with a cost: the cost, the
    /// entry, and whether the mana is at or above it.
    pub mana_watches: Vec<(i32, u32, bool)>,
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
}

const HERO: Making = Making {
    count: 24,
    deferred: &[17, 18, 23, 22],
    death: &[],
    level: &[19, 20],
};
const UNIT: Making = Making {
    count: 18,
    deferred: &[17, 16],
    death: &[10, 11],
    level: &[],
};
const CREEP: Making = Making {
    count: 28,
    deferred: &[16, 15, 20, 25, 24, 23, 22, 21, 19, 27, 26],
    death: &[10, 11, 17, 18],
    level: &[],
};
const CAMP_CREEP: Making = Making {
    count: 29,
    deferred: &[16, 15, 21, 26, 25, 24, 23, 22, 20, 28, 27],
    death: &[10, 11, 18, 19],
    level: &[],
};

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
    let making = if crate::data::is_hero(type_id) {
        &HERO
    } else if owner == super::creeps::CREEPS {
        if super::creeps::group_of(state, h) == [h] {
            &CAMP_CREEP
        } else {
            &CREEP
        }
    } else {
        &UNIT
    };
    let tick = state.table.now;
    let entries = state.table.take_n(tick, making.count, "map start: unit");
    for &k in making.deferred {
        state.table.defer(entries[k], "map start: unit");
    }
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.entries = UnitEntries {
            death: making.death.iter().map(|&k| entries[k]).collect(),
            level: making.level.iter().map(|&k| entries[k]).collect(),
            camp: std::ptr::eq(making, &CAMP_CREEP),
            ..UnitEntries::default()
        };
    }
}

/// Hero `h` changed its level: the parts of its state that hold the level
/// are made anew and the old ones freed 0.0001 s later (c-SAK-1: the
/// `SetHeroXP` of the map start replaced the parts each hero was made
/// with, the level-up at 30.26 s those of `SetHeroXP`).
pub(super) fn level_changed(state: &mut super::State, h: u32) {
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

/// The time of day is set (SPEC-0017): its event takes an object for
/// 0.0001 s, it gets 3 new watches and its old parts are freed 0.0001 s
/// later (c-SAK-1: SetTimeOfDay at the map start, the day standing
/// still, so no new rate part).
///
/// PROVISIONAL: read with the clock suspended right after; a running
/// clock may make a rate part as well.
pub(super) fn time_of_day_set(table: &mut ObjectTable, tick: u64) {
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
    let old = entries(state, h).map(|e| {
        // The objects of its attack on a target go first, then those of
        // its acquisition and of the order (c-SAK-1: the Mountain King's
        // new order at 2501).
        e.game_order = false;
        e.own_attack = false;
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
    for e in old.unwrap_or_default() {
        state.table.defer(e, "order end");
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
    let ability = state.table.take_n(tick, 13, "move ability: order start");
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

/// Unit `h` takes up its attack-move again after its target's death: a
/// new goal with its walk, and its last acquisition's objects are freed
/// 0.0001 s later.
pub(super) fn attack_move_resumed(state: &mut super::State, h: u32) {
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

/// Unit `h`'s move to the point of a Wind Walk's fade ends and its
/// attack-move is taken up again from its order queue, in its update: a
/// new goal with its walk, then the "attack ends" kept from the fade and
/// its last acquisition's objects are freed 0.0001 s later, the latter
/// from the last (c-SAK-1: the Blademaster at 4800).
pub(super) fn attack_move_taken_up(state: &mut super::State, h: u32) {
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
    let tick = state.table.now;
    let order = state.table.take(tick, "script point order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let ability = state.table.take_n(tick, 11, "attack ability: order start");
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
        Some(e) if e.walk_ready => e.walk_ready = false,
        Some(_) => walk_made(state, h),
        None => {}
    }
}

/// Unit `h`'s walk stops or is replaced: its path and cluster are freed
/// at its first update after this tick (SPEC-0024, "Walks").
pub(super) fn walk_dropped(state: &mut super::State, h: u32) {
    // A walk made early for a chase to come stays through its order.
    if let Some(e) = entries(state, h)
        && std::mem::take(&mut e.early_walk)
    {
        return;
    }
    // Given up at a fade, before the updates of the tick, the unit's update
    // in this tick frees it (c-SAK-1: the Blademaster's fade at 4620).
    // PROVISIONAL: read for a fade only.
    let tick = if !state.table.at_fade {
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

/// Unit `h` is ordered the spell it is casting again before its effect:
/// the order is taken and kept.
///
/// PROVISIONAL: when it is freed is not read (not by 60 s in c-SAK-1).
pub(super) fn spell_ordered_again(state: &mut super::State) {
    let tick = state.table.now;
    state.table.take(tick, "script target order");
}

/// Unit `h` starts a cast: the object of its order kept to then is freed.
pub(super) fn cast_started(state: &mut super::State, h: u32) {
    let tick = state.table.now;
    let start = entries(state, h).map(|e| std::mem::take(&mut e.cast_start));
    for e in start.unwrap_or_default() {
        state.table.free(tick, e, "spell ability: order start");
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
        let current = entries(state, h).and_then(|e| e.current.take());
        if let Some(c) = current {
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
    let attacking = state
        .unit(Some(h))
        .is_some_and(|u| u.attack_move.is_some() || u.entries.game_order)
        && entries(state, h).is_some_and(|e| !e.target_death.is_empty());
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
    let old = entries(state, h).map(|e| std::mem::take(&mut e.acquisition));
    for e in old.unwrap_or_default() {
        state.table.defer(e, "acquisition: attack object");
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
        u.attack.is_none() && f32::from_bits(u.backswing_end) > f32::from_bits(now)
    });
    if recording {
        return;
    }
    acquired(state, h);
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
    for (k, (cost, entry, above)) in watches.into_iter().enumerate() {
        let now = mana >= cost as f32;
        if now == above {
            continue;
        }
        let new = state.table.take(tick, "unit state: limit watch");
        state.table.defer(entry, "unit state: limit watch");
        if let Some(e) = entries(state, h) {
            e.mana_watches[k] = (cost, new, now);
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
        if let Some(e) = entries(state, h) {
            e.order.insert(0, o);
        }
    } else {
        state.table.defer(o, "attack ends at the target death");
    }
}

/// Attacker `h` of unit `dead` is handled at the death: the object of its
/// current target is freed 0.0001 s later now, the others it kept to the
/// death at the end of its handling (`attacker_done`), after its attack's
/// end (c-SAK-1: kobold 19 at 4764: 3593, "attack ends", 3533 .. 3531).
pub(super) fn attacker_lost(state: &mut super::State, h: u32, dead: u32) {
    let current = entries(state, h).and_then(|e| {
        let (gone, kept) = std::mem::take(&mut e.target_death)
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
        first
    });
    if let Some(c) = current {
        state.table.defer(c, "acquisition: attack object");
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
fn spell_effect_event(state: &mut super::State, h: u32) {
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
    let order = state.table.take(tick, "stun: order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    // An order of the game the target had comes back after the stun.
    let had = entries(state, target).is_some_and(|e| e.game_order);
    if let Some(e) = entries(state, target) {
        e.stun_game_order = had;
    }
    order_ended(state, target);
    let parts = state.table.take_n(tick, 10, "stun: order objects");
    for &e in parts[6..].iter().rev() {
        state.table.defer(e, "stun: order objects");
    }
    // The caster's object kept to the impact: freed now if its order ended,
    // else with the order (c-SAK-1: the bolts of 741 and 2616).
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
    if let Some(e) = entries(state, target) {
        e.stun_end = std::iter::once(stun).chain(objects).collect();
        e.stun_order = parts[..6].iter().rev().copied().chain([order]).collect();
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
pub(super) fn stun_ends(state: &mut super::State) {
    stuns_with(state, false);
}

fn stuns_with(state: &mut super::State, backs: bool) {
    let tick = state.table.now;
    let units: Vec<u32> = state.units.clone();
    for h in units {
        let Some((end, back)) = entries(state, h).and_then(|e| e.stun_ticks) else {
            continue;
        };
        if end == tick {
            let buff = state.table.take(tick, "buff end");
            let ended = entries(state, h).map(|e| {
                e.buff_end = Some(buff);
                e.stun_ticks = Some((u64::MAX, back));
                std::mem::take(&mut e.stun_end)
            });
            for e in ended.unwrap_or_default() {
                state.table.defer(e, "stun end");
            }
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
        if backs && back == tick {
            let order = state.table.take(tick, "order back after a stun");
            let other = state.table.take(tick, "order back: other");
            let (parts, buff) = entries(state, h)
                .map(|e| {
                    e.order = vec![order];
                    e.stun_ticks = None;
                    (std::mem::take(&mut e.stun_order), e.buff_end.take())
                })
                .unwrap_or_default();
            for e in parts {
                state.table.defer(e, "stun: order objects");
            }
            state.table.defer(other, "order back: other");
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
    // A stunned unit's order comes back first; that object goes 2 ticks
    // on (c-SAK-1: gnoll 13 at 3509, freed at 3511).
    let stunned = entries(state, h).is_some_and(|e| e.stun_ticks.is_some());
    if stunned {
        let back = state.table.take(tick, "order back after a stun");
        state
            .table
            .free_at(tick + 2, back, "order back after a stun");
    }
    // Its death order: a record and 9 queue objects, and for a unit not
    // of the creeps 1 more (SPEC-0024, "Unit death").
    let record = state.table.take(tick, "death order record");
    let queue = state.table.take_n(tick, 9, "death order queue");
    let other = (owner != super::creeps::CREEPS).then(|| state.table.take(tick, "death order"));
    let held = entries(state, h).map(|e| {
        let death = std::mem::take(&mut e.death);
        // The last two of a creep's (its pair) before its order, the
        // first two after all (c-SAK-1: kobold 9 at 2753).
        let (core, pair) = death.split_at(death.len().min(2));
        let own: Vec<u32> = std::mem::take(&mut e.game_objects)
            .into_iter()
            .chain(std::mem::take(&mut e.reaction))
            .chain(e.current.take())
            .chain(
                std::mem::take(&mut e.target_death)
                    .into_iter()
                    .map(|(_, o)| o),
            )
            .chain(std::mem::take(&mut e.acquisition))
            .chain(std::mem::take(&mut e.order))
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
        stun_end.extend(e.buff_end.take());
        (
            pair.to_vec(),
            own,
            core.to_vec(),
            stun_order,
            stun_end,
            missile,
            end_tick,
        )
    });
    let Some((pair, own, core, stun_order, stun_end, missile, end_tick)) = held else {
        return;
    };
    if let Some(end) = end_tick.filter(|&t| t != u64::MAX) {
        for e in missile {
            state.table.free_at(end, e, "missile");
        }
    }
    for e in pair.into_iter().chain(stun_order) {
        state.table.defer(e, "unit death");
    }
    state.table.defer(record, "death order record");
    for e in own {
        state.table.defer(e, "unit death");
    }
    for &e in queue[6..].iter().rev() {
        state.table.defer(e, "death order queue");
    }
    if let Some(o) = other {
        state.table.defer(o, "death order");
    }
    for e in stun_end {
        state.table.defer(e, "unit death");
    }
    for e in core {
        state.table.defer(e, "unit death");
    }
    // The fourth from the last after them (c-SAK-1: 2851 at 2753).
    state.table.defer(queue[5], "death order queue");
    // Of the first 5 queue objects 2 go 3.0 s on, 1 5.0 s on, 2 stay.
    // PROVISIONAL: which of them, and 1000 ticks for "about 5.0 s"
    // (c-SAK-1: 2753 -> 3353, 3353, 3753).
    let at = state.event_time.unwrap_or_else(|| state.timers.time());
    let (three, five) = (
        ow3_math::add(at, 3.0f32.to_bits()),
        ow3_math::add(at, 5.0f32.to_bits()),
    );
    state.table.free_at_q(three, queue[4], "death order queue");
    state.table.free_at_q(three, queue[3], "death order queue");
    state.table.free_at_q(five, queue[2], "death order queue");
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
