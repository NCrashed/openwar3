//! A dead hero revived at an altar (c-SBQ-1, hub #3): the player's revive
//! order on the dead hero, its cost at once, the hero standing up again by
//! the altar at the end of its revive time.
//!
//! PROVISIONAL: read from one revive at level 1; the place by the altar,
//! the rule of the revive time and the gold factor are read once.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::vm::Vm;
use ow3_math::add;

use super::training::{EngineTimer, start_timer};
use super::widgets::Registration;
use super::{Context, Object, State, fire_trigger, test_boolexpr};

/// The order of a revive on a dead hero (c-SBQ-1: the player's TargetOrder
/// 852027 on the altar).
pub const ORDER_REVIVE: i32 = 852_027;

/// `EVENT_PLAYER_HERO_REVIVE_START` and `_FINISH` of common.j.
const EVENT_PLAYER_HERO_REVIVE_START: i32 = 44;
const EVENT_PLAYER_HERO_REVIVE_FINISH: i32 = 46;

/// The share of a hero's gold cost a revive costs at level 1 (c-SBQ-1:
/// 170 of the Paladin's 425).
const GOLD_FACTOR: f32 = 0.40;
/// The share more a level above the first.
const LEVEL_FACTOR: f32 = 0.10;
/// ReviveTimeFactor: the revive takes the hero's build time times this, cut
/// to whole seconds, times its level (c-SBQ-1: 55 x 0.65 = 35.75, revived
/// 35.000759 s later; c-SBR-1 at level 2: 70 s; hub #4).
const TIME_FACTOR: f32 = 0.65;
/// Where the revived hero stands from the altar's centre (c-SBQ-1: the
/// Paladin at (-1264, 176), the altar at (-1120, 352)).
const PLACE: [f32; 2] = [-144.0, -176.0];

/// The player states of gold and food used (`PLAYER_STATE_RESOURCE_*`).
const GOLD: i32 = 1;
const FOOD_USED: i32 = 5;

/// `altar` takes a revive order on `hero`: the cost at once, the start's
/// objects and event, the end's timer.
pub(super) fn start(vm: &mut Vm, state: &Rc<RefCell<State>>, altar: u32, hero: u32) -> bool {
    let (owner, end) = {
        let mut s = state.borrow_mut();
        let Some(h) = s.unit(Some(hero)) else {
            return false;
        };
        let Some(a) = s.unit(Some(altar)) else {
            return false;
        };
        if h.alive || h.owner != a.owner || !crate::data::is_hero(h.type_id) {
            return false;
        }
        let (owner, type_id, level) = (h.owner, h.type_id, h.level);
        let (gold, food, build) = s.data.units.get(&type_id).map_or((0, 0, 0.0), |t| {
            (t.gold_cost, t.food_used, f32::from_bits(t.build_time))
        });
        // 0.40 of the gold cost and 0.10 more a level above the first, cut
        // to whole tens (c-SBQ-1: 170 at level 1; c-SBR-1: 210 at level 2).
        // PROVISIONAL: two points; the cut to tens is a guess that fits them.
        let share = GOLD_FACTOR + LEVEL_FACTOR * (level.max(1) - 1) as f32;
        let cost = ((gold as f32 * share) as i32 / 10) * 10;
        *s.setup.player_states.entry((owner, GOLD)).or_default() -= cost;
        *s.setup.player_states.entry((owner, FOOD_USED)).or_default() += food;
        let seconds = (build * TIME_FACTOR).floor() * level.max(1) as f32;
        let end = add(s.move_clock, seconds.to_bits());
        // An object freed after the start event, then a rate part and a limit
        // watch kept to the end (c-SBQ-1: 1497, 1498, 1499).
        let tick = s.table.now;
        let first = s.table.take(tick, "revive");
        let rate = s.table.take(tick, "revive: rate part");
        let watch = s.table.take(tick, "revive: limit watch");
        s.reviving.insert(hero, (altar, vec![watch, rate], first));
        // It leaves its player's list of dead heroes at the start (c-SBQ-1: G
        // changed at turn 389, the turn of the click).
        if let Some(Object::Unit(u)) = s.objects.get_mut(&hero) {
            u.entries.dead_listed = None;
        }
        // Its type record counts it in production (q3) from the start (c-SBQ-1:
        // G at turn 389).
        super::widgets::type_record(&mut s, owner, type_id, |q| q[3] = q[3].wrapping_add(1));
        start_timer(&mut s, end, EngineTimer::Revive(hero));
        (owner, end)
    };
    let _ = end;
    fire(vm, state, EVENT_PLAYER_HERO_REVIVE_START, owner, hero);
    let mut s = state.borrow_mut();
    if let Some(first) = s.reviving.get(&hero).map(|r| r.2) {
        s.table.defer(first, "revive");
    }
    true
}

/// The end of `hero`'s revive: its state's objects and start order made
/// again, the revive's own objects gone, the hero standing by the altar
/// with full life and its starting mana, the finish event.
pub(super) fn finish(vm: &mut Vm, state: &Rc<RefCell<State>>, hero: u32) {
    let owner = {
        let mut s = state.borrow_mut();
        let Some((altar, kept, _)) = s.reviving.remove(&hero) else {
            return;
        };
        let tick = s.table.now;
        // A limit watch and four rate parts, as at a unit's making, then
        // its start order's five objects (c-SBQ-1: 1502 .. 1511).
        s.table.take_n(tick, 5, "revive: unit state");
        let order = s.table.take_n(tick, 5, "revive: start order");
        for o in kept {
            s.table.defer(o, "revive");
        }
        for &o in order.iter().rev() {
            s.table.defer(o, "revive: start order");
        }
        let at = s.unit(Some(altar)).map(|a| [a.x, a.y]);
        let start_mana = s
            .unit(Some(hero))
            .and_then(|u| s.data.units.get(&u.type_id))
            .map_or(0, |t| t.start_mana);
        let owner = s.unit(Some(hero)).map_or(0, |u| u.owner);
        let type_id = s.unit(Some(hero)).map_or(0, |u| u.type_id);
        // At the end its type record counts it alive again and no more in
        // production (c-SBQ-1: G at turn 738).
        super::widgets::type_record(&mut s, owner, type_id, |q| {
            q[3] = q[3].wrapping_sub(1);
            q[4] = q[4].wrapping_add(1);
        });
        if let Some(Object::Unit(u)) = s.objects.get_mut(&hero) {
            u.alive = true;
            u.died_at = None;
            u.attack = None;
            u.movement = None;
            let max = u.life.max;
            u.life.set(max);
            u.mana.set(start_mana);
            if let Some([x, y]) = at {
                u.x = (f32::from_bits(x) + PLACE[0]).to_bits();
                u.y = (f32::from_bits(y) + PLACE[1]).to_bits();
            }
            u.entries.dead_listed = None;
        }
        owner
    };
    fire(vm, state, EVENT_PLAYER_HERO_REVIVE_FINISH, owner, hero);
}

/// Runs the triggers of player unit event `event` of `owner` for `hero`,
/// as one event with its objects.
fn fire(vm: &mut Vm, state: &Rc<RefCell<State>>, event: i32, owner: i32, hero: u32) {
    let (registrations, owner_handle) = {
        let mut s = state.borrow_mut();
        (s.events.clone(), s.player_handle(owner))
    };
    super::begin_event(state);
    for registration in &registrations {
        if let Registration::PlayerUnit {
            trigger,
            player,
            event: e,
            filter,
        } = *registration
            && e == event
            && player == owner
        {
            let filter_context = Context {
                filter_unit: Some(hero),
                triggering_player: Some(owner_handle),
                event_id: Some(event),
                ..Context::default()
            };
            if test_boolexpr(vm, state, filter, filter_context) {
                let context = Context {
                    event_id: Some(event),
                    triggering_player: Some(owner_handle),
                    triggering_unit: Some(hero),
                    triggering_widget: Some(hero),
                    ..Context::default()
                };
                fire_trigger(vm, state, trigger, context);
            }
        }
    }
    super::end_event(state);
}
