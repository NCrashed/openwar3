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
    deferred: Vec<(u32, &'static str)>,
    /// The parts of the time of day (its rate and the watches of the hours
    /// where something happens), replaced when the time is set.
    pub day: Vec<u32>,
    /// The takings and freeings, when kept.
    pub log: Option<Vec<TableEvent>>,
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
            day: Vec::new(),
            log: None,
        }
    }

    /// The next entry (`C` of SPEC-0024): the head of the free list, or
    /// the size of the table when it is empty.
    pub fn next_entry(&self) -> u32 {
        self.free.last().copied().unwrap_or(self.size)
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
        self.deferred.push((entry, tag));
    }

    /// The 0.0001 s timers of the tick expire: their objects are freed in
    /// the order they were given up.
    pub fn flush(&mut self, tick: u64) {
        for (entry, tag) in std::mem::take(&mut self.deferred) {
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
    /// Its walk was made at its order; the first search of the order
    /// takes no new one.
    pub walk_ready: bool,
    /// The objects of its Wind Walk freed at the fade and at the end.
    pub fade: Vec<u32>,
    pub lasting: Vec<u32>,
    /// The object of its order freed when its cast starts.
    pub cast_start: Vec<u32>,
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
    let tick = state.ticks;
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
    let tick = state.ticks;
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
    let old = entries(state, h).map(|e| std::mem::take(&mut e.order));
    for e in old.unwrap_or_default() {
        state.table.defer(e, "order end");
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
    let tick = state.ticks;
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
    let tick = state.ticks;
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
    let tick = state.ticks;
    if let Some(e) = entries(state, h) {
        let old = std::mem::take(&mut e.walk);
        e.stale.extend(old.into_iter().map(|w| (tick, w)));
        e.walk_ready = false;
    }
}

/// Unit `h` updates: the walks it gave up in earlier ticks are freed.
pub(super) fn unit_updated(state: &mut super::State, h: u32) {
    let tick = state.ticks;
    let due = entries(state, h).map(|e| {
        let (due, kept) = std::mem::take(&mut e.stale)
            .into_iter()
            .partition::<Vec<_>, _>(|&(t, _)| t < tick);
        e.stale = kept;
        due
    });
    for (_, e) in due.unwrap_or_default() {
        state.table.free(tick, e, "walk");
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
    let tick = state.ticks;
    let order = state.table.take(tick, "script target order");
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
    let tick = state.ticks;
    state.table.take(tick, "script target order");
}

/// Unit `h` starts a cast: the object of its order kept to then is freed.
pub(super) fn cast_started(state: &mut super::State, h: u32) {
    let tick = state.ticks;
    let start = entries(state, h).map(|e| std::mem::take(&mut e.cast_start));
    for e in start.unwrap_or_default() {
        state.table.free(tick, e, "spell ability: order start");
    }
    // The walk into range ends.
    walk_dropped(state, h);
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
    let tick = state.ticks;
    let event = state.table.take(tick, "event");
    for _ in 0..registrations {
        let e = state.table.take(tick, "event object");
        state.table.defer(e, "event object");
    }
    state.table.defer(event, "event");
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
    let tick = state.ticks;
    let order = state.table.take(tick, "script immediate order");
    let record = state.table.take(tick, "order record");
    state.table.defer(record, "order record");
    order_ended(state, h);
    let registrations = state
        .events
        .iter()
        .filter(|r| {
            matches!(**r, super::widgets::Registration::Unit { unit, event, .. }
                if unit == h && event == EVENT_UNIT_SPELL_EFFECT)
        })
        .count();
    event_without_triggers(state, registrations);
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
    let tick = state.ticks;
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
    let tick = state.ticks;
    let Some(super::Object::Unit(u)) = state.objects.get_mut(&h) else {
        return;
    };
    for e in std::mem::take(&mut u.entries.death) {
        state.table.free(tick, e, "unit death");
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
