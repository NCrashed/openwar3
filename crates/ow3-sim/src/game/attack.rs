//! Hits of attacking units (SPEC-0016).
//!
//! An attack begins when the unit stands in range of its target and faces
//! it (SPEC-0019, `game::movement`); then each hit does the damage of
//! SPEC-0015 for the amount of [`crate::combat::hit_amount`], with the dice
//! from game generator 2, the first one `dmgpt1` after the start and the
//! next ones `cool1` apart, until the target dies or the unit gets another
//! order.
//!
//! One tick after the end of a swing (`backSw1` after the damage) an
//! attacker whose target has left its reach chases it again (SPEC-0019);
//! its next hit comes no earlier than `cool1` after the last one.
//!
//! PROVISIONAL: hits come after the timers of their tick.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::vm::Vm;

use super::{Object, State};
use crate::combat::{DAMAGE_TYPE_NORMAL, hit_amount};
use crate::data::{Area, Weapon, WeaponKind, is_hero};
use crate::random::ATTACK_DICE;
use ow3_math::natives::{cos, sin, sqrt};
use ow3_math::{add, div, floor, mul, sub};

/// A unit's attack on a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attacking {
    pub(super) target: u32,
    /// The tick of the next hit.
    next: u64,
    /// The order id `GetUnitCurrentOrder` gives: 0 for an attack the unit
    /// began on its own (SPEC-0019).
    pub(super) order_id: i32,
    /// The tick after the end of the last swing, when a target out of
    /// reach is chased again (SPEC-0019).
    pub(super) resume: Option<u64>,
    /// The attacker swings again, or takes a new goal and stands, at its
    /// first update from this tick on: the cooldown less the damage point
    /// after the last hit (SPEC-0019, "After the swing").
    pub(super) renew_at: Option<u64>,
    /// The expiry of the damage-point timer of a swing started at an
    /// update: its queue time plus `dmgpt1` (SPEC-0016). The hit comes in
    /// the tick that holds it.
    pub(super) hit_at: Option<u32>,
    /// The queue time at which that swing started.
    pub(super) swing_q: u32,
    /// The expiry of the cooldown timer started with that swing, once a
    /// slow rescaled it (c-SBJ-1: kobold 6's, 1.8 s slowed at its swing,
    /// sped up at the frost's end): the next swing starts there.
    pub(super) cool_end: Option<u32>,
}

impl Attacking {
    /// The attack as a viewer sees it, for a unit of type `type_id`.
    pub(super) fn view(&self, state: &State, type_id: i32) -> super::AttackView {
        let weapon = weapon_for(state, type_id, Some(self.target)).unwrap_or_default();
        super::AttackView {
            target: self.target,
            next_hit: self.next,
            cooldown: ticks(weapon.cooldown),
            damage_point: ticks(weapon.damage_point),
        }
    }
}

/// Seconds as a whole number of ticks of 0.005 s, at least one.
pub(super) fn ticks(seconds: u32) -> u64 {
    (f64::from(f32::from_bits(seconds)) * 200.0)
        .round()
        .max(1.0) as u64
}

/// Whether `target` can still be hit.
pub(super) fn standing(state: &State, target: u32) -> bool {
    match state.objects.get(&target) {
        // An invulnerable unit cannot be hit (SPEC-0045).
        Some(Object::Unit(u)) => u.alive && !super::cast::invulnerable(state, target),
        Some(Object::Destructable(d)) => d.alive,
        Some(Object::Item(i)) => f32::from_bits(i.life) > 0.0,
        _ => false,
    }
}

/// Unit `h` starts hitting `target` at tick `tick`: the first hit comes its
/// weapon's damage point later (SPEC-0019).
pub(super) fn begin(state: &mut State, h: u32, target: u32, tick: u64, order_id: i32) {
    let point = match state.objects.get(&h) {
        Some(Object::Unit(u)) => weapon_for(state, u.type_id, Some(target)).map_or(0, |w| {
            slowed(state, h, hero_cooldown(state, h, w.damage_point))
        }),
        _ => return,
    };
    let damage_point = ticks(point);
    let now = state.timers.time();
    // A cooldown expiring in this tick, after the start of the tick, also
    // times the swing (`movement::cooldown_expiries`).
    let start = state.move_clock;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        // A swing waits for the cooldown of the last one, also on another
        // target (c-RXR-1: grunt 15, whose target died, swung at the
        // expiry of that cooldown).
        let swing_q = if f32::from_bits(u.cooldown_end) > f32::from_bits(start) {
            u.cooldown_end
        } else {
            now
        };
        let hit_at = add(swing_q, point);
        let wait = ticks(sub(swing_q, now));
        u.attack = Some(Attacking {
            target,
            next: (tick + wait + damage_point).max(u.weapon_ready),
            order_id,
            resume: None,
            renew_at: None,
            hit_at: Some(hit_at),
            swing_q,
            cool_end: None,
        });
    }
    if !state.attacking.contains(&h) {
        state.attacking.push(h);
    }
    // The walk to the target stops (SPEC-0024, "Walks").
    super::table::walk_dropped(state, h);
}

/// The hits due at this tick, in the order the attacks began.
pub(super) fn hits(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    fly(vm, state);
    hits_before(vm, state, None);
}

/// The hits due in this tick that expire before `limit` (all with `None`):
/// the timers of the main queue before the acquisition checks of the tick
/// (c-RXR-1: rifleman 7's hit and the call for help it made came before
/// grunt 14's check in 5489).
pub(super) fn hits_before(vm: &mut Vm, state: &Rc<RefCell<State>>, limit: Option<u32>) {
    // The hits are timers of the queue: those due in this tick come in the
    // order of their expiries, before the rest (c-RXR-1, 1979: footman 1
    // before rifleman 6).
    let mut attackers = state.borrow().attacking.clone();
    {
        let s = state.borrow();
        // Equal expiries go by the start of their timers, the swings, and
        // equal swing starts by the cooldowns that timed them (c-SAK-1:
        // footmen 4 and 6 at 6323, both swinging at 31.109863).
        let key = |h: &u32| {
            s.unit(Some(*h))
                .and_then(|u| u.attack.map(|a| (a, u.cooldown_seq)))
                .filter(|(a, _)| due(&s, a))
                .and_then(|(a, seq)| {
                    a.hit_at
                        .map(|e| (f32::from_bits(e), f32::from_bits(a.swing_q), seq))
                })
                .unwrap_or((f32::INFINITY, 0.0, 0))
        };
        attackers.sort_by(|a, b| {
            let (a, b) = (key(a), key(b));
            a.0.total_cmp(&b.0)
                .then(a.1.total_cmp(&b.1))
                .then(a.2.cmp(&b.2))
        });
    }
    for h in attackers {
        if let Some(limit) = limit {
            let s = state.borrow();
            let early = s
                .unit(Some(h))
                .and_then(|u| u.attack)
                .filter(|a| due(&s, a))
                .and_then(|a| a.hit_at)
                .is_some_and(|e| f32::from_bits(e) < f32::from_bits(limit));
            if !early {
                continue;
            }
        }
        // The goals of backswings that end before this hit come first.
        {
            let mut s = state.borrow_mut();
            let at = s
                .unit(Some(h))
                .and_then(|u| u.attack)
                .filter(|a| due(&s, a))
                .and_then(|a| a.hit_at);
            if at.is_some() {
                after_swing_walks(&mut s, at);
            }
        }
        // A stunned unit does not attack: its hit waits (SPEC-0037).
        // PROVISIONAL: what a stun does to a swing under way is not
        // measured.
        {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            if super::cast::stunned(s, h) {
                let tick = s.ticks;
                if let Some(Object::Unit(u)) = s.objects.get_mut(&h)
                    && let Some(a) = u.attack.as_mut()
                    && a.next <= tick
                {
                    a.next = tick + 1;
                }
                continue;
            }
        }
        // A swing after the first starts only with the target in reach;
        // else the attacker chases it (SPEC-0019, "Start of the attack":
        // a started swing always hits, but a new one needs the range).
        {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            let tick = s.ticks;
            let next_swing = match s.objects.get(&h) {
                Some(Object::Unit(u)) if u.alive => {
                    u.attack.filter(|a| a.resume.is_some()).map(|a| {
                        let point = weapon_for(s, u.type_id, Some(a.target)).map_or(1, |w| {
                            ticks(slowed(s, h, hero_cooldown(s, h, w.damage_point)))
                        });
                        (a, a.next.saturating_sub(point))
                    })
                }
                _ => None,
            };
            // The cooldown expires in this tick with the target out of
            // reach: the attack start chases it at once, searching at the
            // next update (c-SAK-1: image 20 of the Blademaster, cooldown
            // end 9584, search at 9585).
            let clock = s.move_clock;
            let expiring = s.unit(Some(h)).is_some_and(|u| {
                let e = f32::from_bits(u.cooldown_end);
                e > f32::from_bits(clock) && e <= f32::from_bits(add(clock, crate::timers::TICK))
            });
            if let Some((a, start)) = next_swing
                && expiring
                && tick < start
                && standing(s, a.target)
                && !super::movement::reaches(s, h, a.target)
            {
                if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                    u.attack = None;
                }
                super::movement::chase(s, h, a.target, a.order_id, None, false);
                super::movement::no_renewal(s, h);
                continue;
            }
            // A target out of reach when the next swing would start, or at
            // the new goal after the swing: chased from the new goal on,
            // the attacker standing until then (SPEC-0019).
            if let Some((a, start)) = next_swing
                // A swing already started goes on to its hit (c-SAK-1:
                // footman 3, swinging at 8937, hit ogre 18 at 9037 although
                // it walked out of reach at 9014).
                && (start == tick
                    || (s.update_now && tick < start && a.renew_at.is_some_and(|r| tick >= r)))
                && a.renew_at.is_none_or(|r| tick >= r)
                && standing(s, a.target)
                && !super::movement::reaches(s, h, a.target)
            {
                if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                    u.attack = None;
                }
                // A chase that took its new goal at this update searches
                // it at the next one (SPEC-0019, "After the swing").
                if !super::movement::chasing(s, h, a.target) {
                    super::movement::chase(s, h, a.target, a.order_id, None, false);
                }
                continue;
            }
        }
        // A swing starts: the roll of a chance-on-attack ability
        // (SPEC-0036).
        {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            let tick = s.ticks;
            let starts = s.unit(Some(h)).and_then(|u| u.attack).is_some_and(|a| {
                let point = s
                    .unit(Some(h))
                    .and_then(|u| weapon_for(s, u.type_id, Some(a.target)))
                    .map_or(1, |w| {
                        ticks(slowed(s, h, hero_cooldown(s, h, w.damage_point)))
                    });
                a.next.saturating_sub(point) == tick
            });
            // Not facing the target at the expiry: no swing, a face movement
            // to where the target stands now, and the swing at its end
            // (SPEC-0019, "Cooldown expiry"; c-SAK-1: the Blademaster at
            // 9662, swing at 9675).
            let clock = s.move_clock;
            let expires = s.unit(Some(h)).and_then(|u| u.attack).is_some_and(|a| {
                let q = f32::from_bits(a.swing_q);
                q > f32::from_bits(clock) && q <= f32::from_bits(add(clock, crate::timers::TICK))
            });
            if expires && super::movement::face_at_expiry(s, h) {
                continue;
            }
            // The swing starts at the expiry: the walk of the goal taken
            // after the last swing ends (SPEC-0024).
            if expires {
                let q = s.unit(Some(h)).and_then(|u| u.attack).map(|a| a.swing_q);
                if let Some(q) = q {
                    s.table_at(q);
                }
                super::table::walk_dropped(s, h);
                s.table.now = s.ticks;
            }
            // At the swing's queue time when it has one (c-SAK-1: the
            // rolls before the turn checksums of 23.225, 34.65, 40.75 and
            // 48.375 s).
            let timed = s
                .unit(Some(h))
                .and_then(|u| u.attack)
                .is_some_and(|a| a.swing_q != 0);
            if (timed && expires) || (!timed && starts) {
                roll_proc(s, h);
            }
        }
        // The hit, if one is due: target, amount and attack type.
        let hit = {
            let mut s = state.borrow_mut();
            let s = &mut *s;
            let tick = s.ticks;
            let (type_id, attack) = match s.objects.get(&h) {
                Some(Object::Unit(u)) if u.alive => (u.type_id, u.attack),
                _ => (0, None),
            };
            match attack {
                Some(a) if due(s, &a) && standing(s, a.target) => {
                    let weapon = weapon_for(s, type_id, Some(a.target)).unwrap_or_default();
                    let primary = hero_primary(s, h);
                    // A thunder clap slows the attacks (SPEC-0037).
                    let cooldown = slowed(s, h, hero_cooldown(s, h, weapon.cooldown));
                    let backswing = slowed(s, h, hero_cooldown(s, h, weapon.backswing));
                    let amount = hit_amount(&weapon, primary, &mut s.game_random[ATTACK_DICE]);
                    // A launch draws one more output for the missile
                    // (SPEC-0016).
                    if weapon.kind != WeaponKind::Normal {
                        s.game_random[ATTACK_DICE].fraction();
                    }
                    let (amount, bash) = proc_effect(s, h, amount);
                    let point = slowed(s, h, hero_cooldown(s, h, weapon.damage_point));
                    s.cooldown_starts += 1;
                    let seq = s.cooldown_starts;
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        u.cooldown_seq = seq;
                        // The next swing starts at the expiry of this one's
                        // cooldown (SPEC-0016), a timer a slow may have
                        // rescaled.
                        let swing_q = a.cool_end.unwrap_or_else(|| add(a.swing_q, cooldown));
                        let cool = ticks(sub(swing_q, a.swing_q));
                        let next = a.next + cool;
                        let end = tick + ticks(backswing);
                        let resume = Some(end + 1);
                        // The next swing, and with it the new goal of a
                        // chase, at the first update from `D + cool -
                        // dmgpt` on (SPEC-0019).
                        let renew_at = Some((tick + cool).saturating_sub(ticks(point)));
                        u.cooldown_end = swing_q;
                        u.cooldown_target = a.target;
                        u.backswing_end = add(a.hit_at.unwrap_or(0), backswing);
                        u.attack = Some(Attacking {
                            next,
                            resume,
                            renew_at,
                            hit_at: Some(add(swing_q, point)),
                            swing_q,
                            cool_end: None,
                            ..a
                        });
                        u.weapon_ready = next;
                    }
                    // A missile weapon launches at the damage point; the
                    // damage comes when the missile arrives (SPEC-0016).
                    if weapon.kind != WeaponKind::Normal
                        && let Some(missile) = launch(
                            s,
                            h,
                            a.target,
                            amount,
                            weapon.attack_type,
                            weapon.area.clone(),
                            weapon.kind == WeaponKind::Artillery,
                            // The flight counts from the start of the tick
                            // (c-RXR-1: troll 16, launch 4131).
                            s.move_clock,
                        )
                    {
                        let mut missile = missile;
                        // Its objects of the engine's table (SPEC-0024).
                        missile.objects = super::table::missile_launched(s);
                        s.missiles.push(missile);
                        // So does the launch restart the fog modifier (c-RZJ-1:
                        // the Archmage at 21.227, 1757 freed at 24.227034).
                        if let Some(at) = a.hit_at {
                            super::table::fog_pass(s, h, at, false);
                        }
                        None
                    } else {
                        if let (Some(trace), Some(at)) = (s.hit_trace.as_mut(), a.hit_at) {
                            trace.push((tick, at, s.timers.time(), a.target));
                        }
                        // Evasion rolls after the dice; an evaded hit does
                        // nothing (SPEC-0036).
                        // PROVISIONAL: measured for melee hits only.
                        if evades(s, a.target) {
                            None
                        } else {
                            s.event_time = a.hit_at;
                            // The queue's timers up to it free first (c-RZJ-1: a
                            // missile's objects ahead of the hit of 30.135942).
                            if let Some(at) = a.hit_at {
                                s.table.queue_through(at);
                            }
                            // The hit restarts the attacker's fog modifier
                            // (c-RZH-1: the Mountain King at 27.341698).
                            if let Some(at) = a.hit_at {
                                super::table::fog_pass(s, h, at, false);
                            }
                            Some((a.target, amount, weapon.attack_type, weapon.area, bash))
                        }
                    }
                }
                Some(a) if due(s, &a) => {
                    // The target is gone.
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        u.attack = None;
                    }
                    None
                }
                _ => None,
            }
        };
        if let Some((target, amount, attack_type, area, bash)) = hit {
            // Bash: its damage comes before the hit, armor not counted
            // (SPEC-0036, SPEC-0037), and the target is stunned.
            if let Some((extra, unit_stun, hero_stun)) = bash {
                // The objects of the stun come before its damage events
                // (SPEC-0024; c-SAK-1: 2183), and the answers to the hit
                // before both damages (c-SAK-1: kobold 10 at the bash on
                // gnoll 16 in 6130).
                {
                    let mut s = state.borrow_mut();
                    if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
                        super::table::stun_objects(&mut s, h, target, Vec::new());
                    }
                }
                answer_hit(&mut state.borrow_mut(), h, target, None);
                super::widgets::damage_widget(vm, state, Some(h), Some(target), extra, 0, 0);
                let mut s = state.borrow_mut();
                if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
                    let hero = s.unit(Some(target)).is_some_and(|u| is_hero(u.type_id));
                    let at = s.event_time.unwrap_or_else(|| s.timers.time());
                    super::cast::stun(
                        &mut s,
                        target,
                        h,
                        if hero { hero_stun } else { unit_stun },
                        at,
                        false,
                    );
                }
            }
            strike(
                vm,
                state,
                h,
                target,
                amount,
                attack_type,
                area,
                None,
                bash.is_some(),
            );
            state.borrow_mut().event_time = None;
        }
        // One tick after the swing the attacker takes a goal again, also
        // with its target in reach, where it stands and turns to its path
        // until its cooldown is over (SPEC-0019, "After the swing").
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let resumed = match s.objects.get(&h) {
            Some(Object::Unit(u)) if u.alive => u.attack.filter(|a| a.resume == Some(tick)),
            _ => None,
        };
        if let Some(a) = resumed
            && standing(&s, a.target)
        {
            // A target in reach keeps the swing timed by the cooldown.
            let in_reach = super::movement::reaches(&s, h, a.target);
            // Its objects count at the backswing's end (SPEC-0024).
            if let Some(end) = s.unit(Some(h)).map(|u| u.backswing_end) {
                s.table_at(end);
            }
            super::movement::chase(&mut s, h, a.target, a.order_id, a.renew_at, in_reach);
            s.table.now = tick;
            if in_reach && let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                u.attack = Some(a);
            }
        }
    }
    let mut s = state.borrow_mut();
    let s = &mut *s;
    s.attacking.retain(
        |h| matches!(s.objects.get(h), Some(Object::Unit(u)) if u.alive && u.attack.is_some()),
    );
}

/// The weapon a unit of `type_id` uses against `target` (SPEC-0016,
/// "Second weapon"): by the target's classes. PROVISIONAL: destructables
/// count as `debris` and items as `item`.
pub(super) fn weapon_for(state: &State, type_id: i32, target: Option<u32>) -> Option<Weapon> {
    let data = state.data.units.get(&type_id)?;
    let classes = match target.and_then(|t| state.objects.get(&t)) {
        Some(Object::Unit(o)) => state
            .data
            .units
            .get(&o.type_id)
            .map(|t| t.target_type.clone())
            .unwrap_or_default(),
        Some(Object::Destructable(_)) => vec!["debris".to_owned()],
        Some(Object::Item(_)) => vec!["item".to_owned()],
        _ => Vec::new(),
    };
    data.weapon_against(&classes).cloned()
}

/// The units that the area of a hit of `h` on `target` damages besides the
/// target, in the order of their creation, with the amounts before armor
/// (SPEC-0016, "Area damage"): by the distance from the target's centre
/// to theirs, `amount` up to `full`, `half_factor` of it up to `half`,
/// `quarter_factor` up to `quarter`, for units of the classes the area
/// allows, allies and neutral ones too unless it names `enemy`.
///
/// PROVISIONAL: the limits are the fields themselves (the game's were
/// about one unit farther, centred a little beyond the target, open
/// question 6); only units are damaged, not destructables or items.
fn splash(state: &State, h: u32, target: u32, area: &Area, amount: u32) -> Vec<(u32, u32)> {
    let Some(t) = state.unit(Some(target)) else {
        return Vec::new();
    };
    splash_at(state, h, [t.x, t.y], Some(target), area, amount)
}

/// The units round point `at` that an area hit of `h` reaches, but
/// `except`, with their shares of `amount`.
fn splash_at(
    state: &State,
    h: u32,
    at: [u32; 2],
    except: Option<u32>,
    area: &Area,
    amount: u32,
) -> Vec<(u32, u32)> {
    let Some(attacker) = state.unit(Some(h)) else {
        return Vec::new();
    };
    let (x, y) = (f32::from_bits(at[0]), f32::from_bits(at[1]));
    let enemies_only = area.targets.iter().any(|c| c == "enemy");
    let mut out = Vec::new();
    for &other in &state.units {
        if Some(other) == except {
            continue;
        }
        let Some(Object::Unit(u)) = state.objects.get(&other) else {
            continue;
        };
        if !u.alive || u.hidden {
            continue;
        }
        let classes = state
            .data
            .units
            .get(&u.type_id)
            .map_or(&[][..], |d| &d.target_type[..]);
        if !classes.iter().any(|c| area.targets.contains(c)) {
            continue;
        }
        if enemies_only && !super::movement::enemies(state, attacker.owner, u.owner) {
            continue;
        }
        let (dx, dy) = (f32::from_bits(u.x) - x, f32::from_bits(u.y) - y);
        let d = (dx * dx + dy * dy).sqrt();
        let share = if d <= area.full {
            amount
        } else if d <= area.half {
            ow3_math::mul(amount, area.half_factor)
        } else if d <= area.quarter {
            ow3_math::mul(amount, area.quarter_factor)
        } else {
            continue;
        };
        out.push((other, share));
    }
    out
}

/// The answers to a hit of `h` on `target`: the creep's spell, the
/// victim's call and its friends' (SPEC-0019, SPEC-0047).
fn answer_hit(s: &mut State, h: u32, target: u32, impact: Option<[u32; 2]>) {
    super::creeps::consider_spell(s, target, target, h, None);
    if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
        super::movement::retaliate(
            s,
            target,
            h,
            impact.map(|p| (p, super::movement::IMPACT_REACH)),
        );
    }
}

/// The damage of a hit of `h` on `target`, by its swing or its missile:
/// the units round the target first, then the target, and the answers
/// to it (SPEC-0016, SPEC-0019), unless they came already (`answered`).
#[allow(clippy::too_many_arguments)]
fn strike(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: u32,
    target: u32,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
    impact: Option<[u32; 2]>,
    answered: bool,
) {
    // The units round the target, before the target takes its hit.
    let splashed = area
        .as_ref()
        .map(|area| splash(&state.borrow(), h, target, area, amount))
        .unwrap_or_default();
    // The victim and its friends answer the hit before its damage, also a
    // fatal one, whose call sees the attacker still attacking the victim
    // (SPEC-0019; c-SAK-1: kobold 10 took the Mountain King at gnoll 16's
    // call in 6131, the hit killing it).
    // A creep hit may cast its spell at the attacker (SPEC-0047).
    if !answered {
        answer_hit(&mut state.borrow_mut(), h, target, impact);
    }
    super::widgets::damage_widget(
        vm,
        state,
        Some(h),
        Some(target),
        amount,
        attack_type,
        DAMAGE_TYPE_NORMAL,
    );
    for (victim, share) in splashed {
        super::widgets::damage_widget(
            vm,
            state,
            Some(h),
            Some(victim),
            share,
            attack_type,
            DAMAGE_TYPE_NORMAL,
        );
    }
    // A kill on an attack-move: the unit walks on after its
    // backswing (SPEC-0019).
    let mut s = state.borrow_mut();
    if !standing(&s, target) {
        let tick = s.ticks;
        let backswing = match s.objects.get(&h) {
            Some(Object::Unit(u)) => weapon_for(&s, u.type_id, Some(target)).map_or(1, |w| {
                ticks(slowed(&s, h, hero_cooldown(&s, h, w.backswing)))
            }),
            _ => 1,
        };
        super::movement::killed(&mut s, h, tick, backswing);
    }
}
/// A missile in flight (SPEC-0016, "Missiles"). One without homing hits
/// its target wherever it is when a timer of `L / Missilespeed` from the
/// launch expires (`L` the plane distance at the launch); a homing one
/// moves `Missilespeed * 0.1` towards the target's centre every 0.1 s
/// and hits once it is nearer than that; a shell of artillery goes to the
/// target's point at the launch and hits the units round it.
#[derive(Clone, Debug, PartialEq)]
pub struct Missile {
    attacker: u32,
    target: u32,
    /// Where it is, for a homing missile, or where it started.
    at: [u32; 2],
    /// The target's point at the launch, for artillery and for drawing.
    point: [u32; 2],
    /// The movement clock at the launch and the expiry of its flight.
    launched: u32,
    expiry: u32,
    /// A homing missile: its step per 0.1 s and the tick of its next step.
    homing: Option<(u32, u64)>,
    artillery: bool,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
    /// A missile of an ability: the stun of its hit, for units and for
    /// heroes, in seconds (SPEC-0037).
    pub(super) spell: Option<(u32, u32)>,
    /// The objects of the engine's table of a spell's missile, kept to
    /// the end of the stun of its hit (SPEC-0024).
    pub(super) objects: Vec<u32>,
}

/// 1/32: world units to path cells.
const CELLS: u32 = 0x3D00_0000;
/// 0.1 s, the step of a homing missile, in ticks and as a real.
const HOMING_TICKS: u64 = 20;
/// The least delay of a timer, 0.0001 s (SPEC-0002).
const MIN_DELAY: u32 = 0x38D1_B717;
const HOMING_STEP: u32 = 0x3DCC_CCCD;

/// The plane distance from `a` to `b` (world units, engine reals).
fn distance(a: [u32; 2], b: [u32; 2]) -> u32 {
    let (dx, dy) = (sub(b[0], a[0]), sub(b[1], a[1]));
    sqrt(add(mul(dx, dx), mul(dy, dy)))
}

/// Where unit `h` is drawn, the base of its launch point: its position, or
/// that + (-16, -16) for a unit whose footprint class (collision / 32:
/// below 0.5, below 1.0, below 1.5, more) is odd (c-SBK-1: the kobold
/// geomancer, collision 31, launching from (-436, -16) at (-420, 0)).
fn drawn_position(state: &State, h: u32) -> [u32; 2] {
    let Some(u) = state.unit(Some(h)) else {
        return [0, 0];
    };
    let r = state
        .data
        .units
        .get(&u.type_id)
        .map_or(0.0, |t| f32::from_bits(t.collision) / 32.0);
    let odd = (0.5..1.0).contains(&r) || r >= 1.5;
    if odd {
        [
            add(u.x, (-16.0f32).to_bits()),
            add(u.y, (-16.0f32).to_bits()),
        ]
    } else {
        [u.x, u.y]
    }
}

/// The missile of `h` launched now at `target`, if its type has a missile
/// speed and the target is not nearer than `Missilespeed * 0.1` to the
/// launch point (then it hits at once). The launch point is `launchY`
/// ahead and `launchX` to the side.
///
/// PROVISIONAL: "ahead" is taken as the direction to the target (the
/// attacker faces it within 0.2 radians when it swings) and the side as
/// its right (SPEC-0016 measured only targets straight ahead).
///
/// `launched` is the movement clock at the start of the launch tick.
#[allow(clippy::too_many_arguments)]
fn launch(
    state: &State,
    h: u32,
    target: u32,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
    artillery: bool,
    launched: u32,
) -> Option<Missile> {
    let u = state.unit(Some(h))?;
    let data = state.data.units.get(&u.type_id)?;
    let speed = data.missile_speed;
    if f32::from_bits(speed) <= 0.0 {
        return None;
    }
    let to = super::movement::position_of(state, target)?;
    // Along the facing the unit shows, `launchY` ahead and `launchX` to
    // the right, floored to quarters (c-RXR-1, five troll missiles).
    let (fx, fy) = (cos(u.facing), sin(u.facing));
    let [side, ahead] = data.launch;
    let [bx, by] = drawn_position(state, h);
    let quarter = |v: u32| mul(floor(mul(v, 4.0f32.to_bits())), 0.25f32.to_bits());
    let at = [
        quarter(add(bx, add(mul(fx, ahead), mul(fy, side)))),
        quarter(add(by, sub(mul(fy, ahead), mul(fx, side)))),
    ];
    let flight = distance(at, to);
    let step = mul(speed, HOMING_STEP);
    if f32::from_bits(flight) < f32::from_bits(step) {
        return None;
    }
    let homing = (data.missile_homing && !artillery).then(|| {
        // The first step is the move of the launch.
        (step, state.ticks + HOMING_TICKS)
    });
    let at = match homing {
        Some(_) => {
            let k = div(step, flight);
            [
                add(at[0], mul(sub(to[0], at[0]), k)),
                add(at[1], mul(sub(to[1], at[1]), k)),
            ]
        }
        None => at,
    };
    Some(Missile {
        attacker: h,
        target,
        at,
        point: to,
        launched,
        // The timer of the movers adds its least delay, 0.0001 (c-RXR-1:
        // troll 18, impact 2715).
        expiry: add(add(launched, div(flight, speed)), MIN_DELAY),
        homing,
        artillery,
        amount,
        attack_type,
        area,
        spell: None,
        objects: Vec::new(),
    })
}

/// A spell's missile from `h` at `target` with `amount` and no stun, at
/// `speed` (SPEC-0041: Death Coil); `None` when the target is within one
/// step of the launch point. It flies as an attack's missile that does not
/// home: one timer of `L / speed` from the start of the tick the effect's
/// queue time `effect_q` is in, `L` from the caster's launch point
/// (`launchY` ahead along its facing, `launchX` to the right) to the
/// target's centre (c-SBJ-1: launch point (-740, 20), hit at 3.134970).
pub(super) fn launch_spell_missile(
    state: &State,
    h: u32,
    target: u32,
    amount: u32,
    speed: u32,
    effect_q: u32,
    centre: bool,
) -> Option<Missile> {
    let u = state.unit(Some(h))?;
    // A knife of Fan of Knives leaves from the centre (c-SBK-1).
    let [side, ahead] = if centre {
        [0, 0]
    } else {
        state.data.units.get(&u.type_id)?.launch
    };
    let [bx, by] = drawn_position(state, h);
    let (fx, fy) = (cos(u.facing), sin(u.facing));
    // Floored to quarters, as a unit's missile (SPEC-0041; c-SBO-1: the
    // Warden's shadow strike from (-511.75, 332.5)).
    let quarter = |v: u32| mul(floor(mul(v, 4.0f32.to_bits())), 0.25f32.to_bits());
    let at = [
        quarter(add(bx, add(mul(fx, ahead), mul(fy, side)))),
        quarter(add(by, sub(mul(fy, ahead), mul(fx, side)))),
    ];
    let to = super::movement::position_of(state, target)?;
    let flight = distance(at, to);
    let step = mul(speed, HOMING_STEP);
    if f32::from_bits(step) <= 0.0 || f32::from_bits(flight) < f32::from_bits(step) {
        return None;
    }
    let next = add(state.move_clock, crate::timers::TICK);
    let launched = if f32::from_bits(effect_q) >= f32::from_bits(next) {
        next
    } else if f32::from_bits(effect_q) >= f32::from_bits(state.move_clock) {
        state.move_clock
    } else {
        state.last_move_clock
    };
    Some(Missile {
        attacker: h,
        target,
        at,
        point: to,
        launched,
        expiry: add(add(launched, div(flight, speed)), MIN_DELAY),
        homing: None,
        artillery: false,
        amount,
        attack_type: 0,
        area: None,
        spell: Some((0, 0)),
        objects: Vec::new(),
    })
}

/// The missiles of this tick: those that arrive strike, in the order of
/// their launch. A missile whose target died or is gone is lost; a shell
/// of artillery goes on to its point.
fn fly(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let arrived = {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let tick = s.ticks;
        // The flights are timers of the movers' scheduler: they end in the
        // tick whose clock, at its end, is at or after the expiry, after
        // the main queue (c-RXR-1, troll 19: expiry 4.297861, impact in
        // tick 860, after the script timer of that tick).
        let next_clock = add(s.move_clock, crate::timers::TICK);
        let mut arrived = Vec::new();
        let mut flying = Vec::with_capacity(s.missiles.len());
        for mut m in std::mem::take(&mut s.missiles) {
            // A plain missile whose target is gone flies on to its expiry: its
            // objects go 5.0 s after that impact (c-RZJ-1: the water
            // elemental's of 12.338930, target dead at 2495, freed at
            // 17.593147).
            if !m.artillery && !standing(s, m.target) && m.spell.is_some() {
                continue;
            }
            // A spell's bolt: it lands at its straight flight's end; at a
            // step's end it measures again to where the target is now, and a
            // target that moved less than a step away is hit at once.
            if m.spell.is_some()
                && let Some((step, next)) = m.homing
            {
                if f32::from_bits(m.expiry) <= f32::from_bits(next_clock) {
                    arrived.push(m);
                    continue;
                }
                if next == tick {
                    let Some(to) = super::movement::position_of(s, m.target) else {
                        continue;
                    };
                    // Only a target moving at the step is aimed at anew and
                    // tested against the step; one standing is left alone,
                    // the bolt flying on to its straight flight's end
                    // (c-RZH-1: the bolt at kobold 8, within a step at
                    // 4.654901, hit at 4.714897).
                    let moving = s
                        .unit(Some(m.target))
                        .and_then(|u| u.movement)
                        .is_some_and(|t| super::movement::walking_now(&t));
                    if to != m.point && moving {
                        let left = distance(m.at, to);
                        if f32::from_bits(left) < f32::from_bits(step) {
                            arrived.push(m);
                            continue;
                        }
                        let speed = div(step, HOMING_STEP);
                        m.expiry = add(next_clock, div(left, speed));
                        m.point = to;
                    }
                    let aimed = distance(m.at, m.point);
                    let k = if f32::from_bits(aimed) > f32::from_bits(step) {
                        div(step, aimed)
                    } else {
                        0x3F80_0000
                    };
                    m.at = [
                        add(m.at[0], mul(sub(m.point[0], m.at[0]), k)),
                        add(m.at[1], mul(sub(m.point[1], m.at[1]), k)),
                    ];
                    m.homing = Some((step, next + HOMING_TICKS));
                }
                flying.push(m);
                continue;
            }
            match m.homing {
                None => {
                    if f32::from_bits(m.expiry) <= f32::from_bits(next_clock) {
                        arrived.push(m);
                    } else {
                        flying.push(m);
                    }
                }
                Some((step, next)) if next == tick => {
                    // A homing one whose target is gone flies on to where the
                    // target was last (c-RZJ-1: the Archmage's of 36.787998,
                    // the Ogre Lord dead at 37.137806, freed at 42.295071).
                    let to = if standing(s, m.target) {
                        let Some(to) = super::movement::position_of(s, m.target) else {
                            continue;
                        };
                        m.point = to;
                        to
                    } else {
                        m.point
                    };
                    let left = distance(m.at, to);
                    if f32::from_bits(left) < f32::from_bits(step) {
                        // Nearer than a step, it flies the rest at its speed
                        // and hits at that flight's end, which may be in a
                        // later tick (c-RZJ-1: the Archmage's missile 4.79
                        // short at 3421, hit held into 3423).
                        let speed = div(step, HOMING_STEP);
                        m.expiry = add(add(s.move_clock, div(left, speed)), MIN_DELAY);
                        m.homing = None;
                        if f32::from_bits(m.expiry) <= f32::from_bits(next_clock) {
                            arrived.push(m);
                        } else {
                            flying.push(m);
                        }
                        continue;
                    }
                    let k = div(step, left);
                    m.at = [
                        add(m.at[0], mul(sub(to[0], m.at[0]), k)),
                        add(m.at[1], mul(sub(to[1], m.at[1]), k)),
                    ];
                    m.homing = Some((step, next + HOMING_TICKS));
                    flying.push(m);
                }
                Some(_) => flying.push(m),
            }
        }
        s.missiles = flying;
        // The impacts come at the end of the tick.
        s.event_time = Some(next_clock);
        arrived
    };
    state.borrow_mut().impact_now = true;
    for m in arrived {
        if let Some(stun) = m.spell {
            // At its expiry: the end of the tick, or the clock of this tick
            // for a bolt launched within a step (c-SAK-1: 39.901287).
            let previous = {
                let mut s = state.borrow_mut();
                let end = s.event_time.unwrap_or(s.move_clock);
                // (A missile of one timer, Death Coil's or a knife, at the end: c-SBK-1,
                // the knife at 4.6952386 in the tick ending 4.699898.)
                let one_timer = stun == (0, 0) || stun == KNIFE;
                let at = if m.homing.is_none()
                    && !one_timer
                    && f32::from_bits(m.expiry) < f32::from_bits(end)
                {
                    m.expiry
                } else {
                    end
                };
                s.table_at(at);
                s.event_time.replace(at)
            };
            spell_hit(vm, state, m.attacker, m.target, m.amount, stun, m.objects);
            state.borrow_mut().event_time = previous;
            {
                let mut s = state.borrow_mut();
                s.table.now = s.ticks;
            }
            continue;
        }
        match (m.artillery, &m.area) {
            // A shell hits the units round its point by their distances.
            (true, Some(area)) => {
                let hit = splash_at(&state.borrow(), m.attacker, m.point, None, area, m.amount);
                for (victim, share) in hit {
                    strike(
                        vm,
                        state,
                        m.attacker,
                        victim,
                        share,
                        m.attack_type,
                        None,
                        Some(m.point),
                        false,
                    );
                }
            }
            (true, None) => {}
            (false, _) => {
                // It hits at the start of the next tick, after this tick's
                // frees (`held_impacts`).
                let mut s = state.borrow_mut();
                let at = s.event_time.unwrap_or_else(|| s.timers.time());
                s.held_impacts.push((m, at));
            }
        }
    }
    state.borrow_mut().impact_now = false;
    state.borrow_mut().event_time = None;
}

/// The attacks' missiles that arrived at the end of the last tick hit,
/// at the queue time of their arrival: their objects go 5.0 s after it
/// (c-RZJ-1: the water elemental's of 5.094869, freed at 10.099306), and
/// the impact calls for help round where the missile hits.
pub(super) fn held_impacts(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let held = std::mem::take(&mut state.borrow_mut().held_impacts);
    // As an impact at the end of a tick (`impact_now`): a unit its call
    // stops makes this tick's step and stands from the next.
    state.borrow_mut().impact_now = !held.is_empty();
    for (m, at) in held {
        if !standing(&state.borrow(), m.target) {
            super::table::missile_hit(&mut state.borrow_mut(), &m.objects, at);
            continue;
        }
        {
            let mut s = state.borrow_mut();
            s.event_time = Some(at);
            super::table::missile_hit(&mut s, &m.objects, at);
            // At the very start of the tick, for the logs' timers.
            let (tick, now) = (s.ticks, s.timers.time());
            if let Some(trace) = s.hit_trace.as_mut() {
                trace.push((tick, at, now, m.target));
            }
        }
        // Only a missile with an area calls round its impact (SPEC-0019,
        // test RZG); a plain one makes the victim's own call, held by its
        // timer (c-RXR-1: the headhunters' spears, 63 calls of 600 in
        // 0 .. 30 s, none of 750).
        let impact = m
            .area
            .is_some()
            .then(|| super::movement::position_of(&state.borrow(), m.target))
            .flatten();
        strike(
            vm,
            state,
            m.attacker,
            m.target,
            m.amount,
            m.attack_type,
            m.area,
            impact,
            false,
        );
        state.borrow_mut().event_time = None;
    }
    state.borrow_mut().impact_now = false;
}

/// A missile in flight as a viewer sees it: the type of the unit that
/// launched it, where it is (world units) and where it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MissileView {
    pub type_id: i32,
    pub at: [f32; 2],
    pub towards: [f32; 2],
}

/// The missiles in flight; one on a timer is drawn on the line from its
/// launch point to the target by the share of its flight time gone.
pub(super) fn views(state: &State) -> Vec<MissileView> {
    let clock = f32::from_bits(state.move_clock);
    state
        .missiles
        .iter()
        .filter_map(|m| {
            let type_id = state.unit(Some(m.attacker)).map_or(0, |u| u.type_id);
            let to = if m.artillery {
                m.point
            } else {
                super::movement::position_of(state, m.target)?
            };
            let to = to.map(f32::from_bits);
            let from = m.at.map(f32::from_bits);
            let at = match m.homing {
                Some(_) => from,
                None => {
                    let (start, end) = (f32::from_bits(m.launched), f32::from_bits(m.expiry));
                    let share = ((clock - start) / (end - start).max(1e-6)).clamp(0.0, 1.0);
                    [
                        from[0] + (to[0] - from[0]) * share,
                        from[1] + (to[1] - from[1]) * share,
                    ]
                }
            };
            Some(MissileView {
                type_id,
                at,
                towards: to,
            })
        })
        .collect()
}

/// The chance-on-attack abilities by their base codes: critical strike,
/// bash, drunken brawler (SPEC-0036).
const PROC_ABILITIES: [[u8; 4]; 3] = [*b"AOcr", *b"AHbh", *b"ANdb"];

/// The chance factors of the pseudo-random distribution by the chance in
/// steps of 5 percent (SPEC-0036, table `C`).
const PROC_TABLE: [f32; 21] = [
    0.0, 0.0038, 0.0147, 0.0320, 0.0547, 0.0819, 0.1126, 0.1461, 0.1815, 0.2184, 0.2562, 0.2953,
    0.3333, 0.3781, 0.4203, 0.4607, 0.5, 0.5799, 0.6666, 0.7706, 1.0,
];

/// The chance-on-attack ability of unit `h`, its base code and level.
fn proc_ability(state: &State, h: u32) -> Option<(i32, i32, i32)> {
    let u = state.unit(Some(h))?;
    let t = state.data.units.get(&u.type_id)?;
    t.abilities
        .iter()
        .chain(&t.hero_abilities)
        .copied()
        .find_map(|a| {
            let base = state.data.abilities.get(&a).map_or(a, |d| d.code);
            let known = PROC_ABILITIES
                .iter()
                .any(|p| i32::from_be_bytes(*p) == base);
            let level = super::heroes::ability_level(state, u, a);
            (known && level > 0).then_some((a, base, level))
        })
}

/// The roll of unit `h`'s chance-on-attack ability at the start of a
/// swing (SPEC-0036): `C[i] * N` against one draw of game generator 3.
fn roll_proc(state: &mut State, h: u32) {
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.procced = None;
    }
    let Some((ability, _, level)) = proc_ability(state, h) else {
        return;
    };
    let chance = state
        .data
        .abilities
        .get(&ability)
        .and_then(|a| a.per_level.get((level - 1).max(0) as usize))
        .map_or(0, |l| l.data[0]);
    let i = ow3_math::r2i(add(mul(chance, 0.2f32.to_bits()), 0.5f32.to_bits())).clamp(0, 20);
    let n = state
        .unit(Some(h))
        .and_then(|u| u.proc_counters.iter().find(|(a, _)| *a == ability))
        .map_or(1, |&(_, n)| n);
    let c = mul(PROC_TABLE[i as usize].to_bits(), ow3_math::i2r(n as i32));
    let x = state.game_random[crate::random::ATTACK_PROC].fraction();
    let procs = f32::from_bits(c) >= f32::from_bits(x);
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        let next = if procs { 1 } else { n + 1 };
        match u.proc_counters.iter_mut().find(|(a, _)| *a == ability) {
            Some(entry) => entry.1 = next,
            None => u.proc_counters.push((ability, next)),
        }
        if procs {
            u.procced = Some((ability, level));
        }
    }
}

/// What the proc of `h`'s swing does to its hit of `amount`: the amount
/// times `DataB` for a critical strike, and a damage of `DataC` before
/// the hit for bash (SPEC-0036).
///
/// PROVISIONAL: the stun of bash and the effects of drunken brawler are
/// not measured; the extra damage of bash takes the hit's attack type.
fn proc_effect(state: &State, h: u32, amount: u32) -> (u32, Option<(u32, u32, u32)>) {
    let Some((ability, level)) = state.unit(Some(h)).and_then(|u| u.procced) else {
        return (amount, None);
    };
    let Some(data) = state.data.abilities.get(&ability) else {
        return (amount, None);
    };
    let numbers = data
        .per_level
        .get((level - 1).max(0) as usize)
        .copied()
        .unwrap_or_default();
    if data.code == i32::from_be_bytes(*b"AOcr") {
        (mul(amount, numbers.data[1]), None)
    } else if data.code == i32::from_be_bytes(*b"AHbh") {
        (
            amount,
            Some((numbers.data[2], numbers.duration, numbers.hero_duration)),
        )
    } else {
        (amount, None)
    }
}

/// The primary attribute of hero `h` times `StrAttackBonus`, added to the
/// damage of its hits (SPEC-0034); 0 for other units.
fn hero_primary(state: &State, h: u32) -> u32 {
    let Some(u) = state.unit(Some(h)).filter(|u| is_hero(u.type_id)) else {
        return 0;
    };
    let points = match state
        .data
        .units
        .get(&u.type_id)
        .map_or(0, |t| t.primary_attribute)
    {
        1 => u.strength,
        2 => u.agility,
        3 => u.intelligence,
        _ => 0,
    };
    mul(ow3_math::i2r(points), state.combat.heroes.str_attack)
}

/// The time `seconds` of a damage point, backswing or cooldown of `h`'s
/// swing divided by its attack-speed factor while a thunder clap slows it
/// (SPEC-0037).
fn slowed(state: &State, h: u32, seconds: u32) -> u32 {
    super::cast::attack_slow(state, h).map_or(seconds, |speed| div(seconds, speed))
}

/// A slow of attack-speed factor `speed` starting at the queue time `now`:
/// the time left of the running cooldown of `u` is divided by it, and a
/// swing waiting for that cooldown starts at the new end with its damage
/// point divided too (SPEC-0037; c-RZH-1, creep 10 at 5281).
pub(super) fn slow_cooldown(u: &mut super::Unit, now: u32, speed: u32, cooldown: u32) {
    scale_times(u, now, cooldown, |t| div(t, speed));
}

/// The end of a slow of attack-speed factor `speed` at the queue time
/// `now`: the times left are sped up again (c-SBJ-1: kobold 6,
/// 26 ticks of its cooldown left at the frost's end at 8.96, its swing
/// 20 ticks later).
pub(super) fn unslow_cooldown(u: &mut super::Unit, now: u32, speed: u32, cooldown: u32) {
    // The rest divided by the slowed rate 1 / speed (c-SBJ-1: 0x3DCCCFC2,
    // 0x3F88F5F1, 0x3DCCD022, 0x3EF0A4BA read in the game).
    let rate = div(1.0f32.to_bits(), speed);
    scale_times(u, now, div(cooldown, speed), |t| div(t, rate));
}

/// Ticks from `from` to `to`, negative when `to` is earlier.
fn ticks_between(from: u32, to: u32) -> i64 {
    if f32::from_bits(to) >= f32::from_bits(from) {
        ticks(sub(to, from)) as i64
    } else {
        -(ticks(sub(from, to)) as i64)
    }
}

fn shift(t: u64, by: i64) -> u64 {
    t.saturating_add_signed(by)
}

/// The rest of the backswing and of the cooldown of `u` after queue time
/// `now`, and a swing waiting for that cooldown, scaled by `scale`.
/// `cooldown` is the cooldown a swing under way started with.
fn scale_times(u: &mut super::Unit, now: u32, cooldown: u32, scale: impl Fn(u32) -> u32) {
    // So does the rest of a backswing (c-SBJ-1: gnoll 4, 40 ticks left at
    // the frost of 4.96, resumed 53 ticks later).
    let back = u.backswing_end;
    if f32::from_bits(back) > f32::from_bits(now) {
        let end = add(now, scale(sub(back, now)));
        let later = ticks_between(back, end);
        u.backswing_end = end;
        if let Some(a) = u.attack.as_mut()
            && let Some(r) = a.resume
        {
            a.resume = Some(shift(r, later));
        }
    }
    let old = u.cooldown_end;
    // A swing under way: the rest of its damage point (c-SBJ-1: kobold
    // 6, its hit at 1813).
    if let Some(a) = u.attack.as_mut()
        && f32::from_bits(a.swing_q) <= f32::from_bits(now)
        && let Some(hit) = a
            .hit_at
            .filter(|&h| f32::from_bits(h) > f32::from_bits(now))
    {
        let end = add(now, scale(sub(hit, now)));
        a.next = shift(a.next, ticks_between(hit, end));
        a.hit_at = Some(end);
        // Its cooldown timer, started with it.
        let cool = a.cool_end.unwrap_or_else(|| add(a.swing_q, cooldown));
        if f32::from_bits(cool) > f32::from_bits(now) {
            a.cool_end = Some(add(now, scale(sub(cool, now))));
        }
    }
    if f32::from_bits(old) <= f32::from_bits(now) {
        return;
    }
    let end = add(now, scale(sub(old, now)));
    let later = ticks_between(old, end);
    u.cooldown_end = end;
    u.weapon_ready = shift(u.weapon_ready, later);
    if let Some(a) = u.attack.as_mut().filter(|a| a.swing_q == old) {
        let point = a.hit_at.map_or(0, |h| sub(h, old));
        let scaled = scale(point);
        a.swing_q = end;
        a.hit_at = Some(add(end, scaled));
        a.next = shift(a.next, later + ticks_between(point, scaled));
    }
}

/// The seconds between the hits of `h` with a weapon of `cooldown`: for a
/// hero `cooldown / (1 + AgiAttackSpeedBonus * AGI)` (SPEC-0034). Its
/// damage point and its backswing shrink the same way (c-RZJ-1: the Paladin,
/// AGI 16, backswing 0.567 / 1.32 = 0.4295 s after his kill at 2836).
/// The cooldown of `h`'s weapon on its attack's target, unslowed.
pub(super) fn swing_cooldown(state: &State, h: u32) -> u32 {
    let Some(u) = state.unit(Some(h)) else {
        return 0;
    };
    let target = u.attack.map(|a| a.target);
    let weapon = weapon_for(state, u.type_id, target).unwrap_or_default();
    hero_cooldown(state, h, weapon.cooldown)
}

fn hero_cooldown(state: &State, h: u32, cooldown: u32) -> u32 {
    match state.unit(Some(h)).filter(|u| is_hero(u.type_id)) {
        Some(u) => {
            let speed = mul(
                state.combat.heroes.agi_attack_speed,
                ow3_math::i2r(u.agility),
            );
            div(cooldown, add(1.0f32.to_bits(), speed))
        }
        None => cooldown,
    }
}

/// The missile of an ability of `h` launched now at `target` from the
/// caster's centre: homing at `speed`, `amount` of spell damage and a stun
/// of `stun` seconds (for units, for heroes) on its hit (SPEC-0037).
pub(super) fn launch_spell(
    state: &State,
    h: u32,
    target: u32,
    amount: u32,
    stun: (u32, u32),
    speed: u32,
    effect_q: u32,
) -> Option<Missile> {
    // It flies straight at the target's position at the launch, from the
    // caster's position floored to 1/128 cell, at its speed (SPEC-0037,
    // "Flight").
    let u = state.unit(Some(h))?;
    let from = [u.x, u.y];
    let to = super::movement::position_of(state, target)?;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let cells = |p: [u32; 2]| super::movement::relative(p, min).map(|c| mul(c, CELLS));
    let floor128 = |c: u32| (f32::from_bits(mul(c, 0x4300_0000)).floor() / 128.0).to_bits();
    let (a, b) = (cells(from).map(floor128), cells(to));
    let (dx, dy) = (sub(b[0], a[0]), sub(b[1], a[1]));
    let d = ow3_math::natives::sqrt(add(mul(dx, dx), mul(dy, dy)));
    let per_tick = mul(mul(speed, CELLS), crate::timers::TICK);
    if f32::from_bits(per_tick) <= 0.0 {
        return None;
    }
    // The flight is a timer of `d / speed` seconds from the clock of the
    // tick the effect's queue time is in, and the impact a 0.0001 s timer
    // after it: in the first tick whose queue time reaches that (c-SAK-1:
    // effect q 3.039998, clock 3.039972, hit at 3.699956; c-RZH-1: bolt 2
    // from clock 13.898668, 0.159922 s, impact timer 14.058690, hit at
    // 14.063641).
    let start = add(state.move_clock, crate::timers::TICK);
    let from_clock = if f32::from_bits(effect_q) < f32::from_bits(start) {
        state.move_clock
    } else {
        start
    };
    let expiry = add(
        add(from_clock, div(d, mul(speed, CELLS))),
        crate::timers::MIN_TIMEOUT,
    );
    // It homes (SPEC-0016): its logical point a 0.1 s step on towards the
    // target, measured again at each step's end (c-SAK-1: the bolt of 2595).
    let step = mul(speed, HOMING_STEP);
    let flight = distance(from, to);
    // Within a step at the launch it hits in the next tick (c-SAK-1: the
    // bolt at gnoll 17, 89.3 away: effect at 39.896290, hit at 39.901287).
    let expiry = if f32::from_bits(flight) <= f32::from_bits(step) {
        start
    } else {
        expiry
    };
    let (at, homing) = if f32::from_bits(flight) > f32::from_bits(step) {
        let k = div(step, flight);
        (
            [
                add(from[0], mul(sub(to[0], from[0]), k)),
                add(from[1], mul(sub(to[1], from[1]), k)),
            ],
            Some((step, state.ticks + HOMING_TICKS)),
        )
    } else {
        (from, None)
    };
    Some(Missile {
        attacker: h,
        target,
        at,
        point: to,
        launched: state.move_clock,
        expiry,
        homing,
        artillery: false,
        amount,
        attack_type: 0,
        area: None,
        spell: Some(stun),
        objects: Vec::new(),
    })
}

/// The hit of a spell of `h` on `target`: a damage event of 0 first, then
/// `amount` of the spells attack type, which armor does not reduce, and
/// the stun (SPEC-0037).
/// The `stun` of a spell missile that hits once with no stun: a knife of
/// Fan of Knives.
pub(super) const KNIFE: (u32, u32) = (u32::MAX, u32::MAX);

pub(super) fn spell_hit(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: u32,
    target: u32,
    amount: u32,
    stun: (u32, u32),
    objects: Vec<u32>,
) {
    // Each damage is a hit: its relay and the victim's call come first
    // (c-SAK-1: the bolt on kobold 14 at 4513 called ogre 18).
    // A knife hits once, without the 0 first (c-SBK-1).
    let knife = stun == KNIFE;
    let stun = if knife { (0, 0) } else { stun };
    // A missile without a stun frees its objects 5.0 s after the hit
    // (c-SBJ-1: Death Coil's), its timer started before the damage (c-SBN-1:
    // the Death Coil that killed at 5.159, freed at 10.159 before the
    // death's queue object).
    if stun == (0, 0) {
        let mut s = state.borrow_mut();
        let at = s.event_time.unwrap_or_else(|| s.timers.time());
        super::table::missile_hit(&mut s, &objects, at);
    }
    let damages: &[u32] = if knife { &[amount] } else { &[0, amount] };
    for (k, &damage) in damages.iter().enumerate() {
        // The objects of the stun come between the two (SPEC-0024).
        if k == 1 && stun != (0, 0) {
            let mut s = state.borrow_mut();
            if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
                super::table::stun_objects(&mut s, h, target, objects.clone());
            }
        }
        {
            let mut s = state.borrow_mut();
            if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
                super::movement::retaliate(&mut s, target, h, None);
            }
        }
        super::widgets::damage_widget(vm, state, Some(h), Some(target), damage, 0, 0);
    }
    let mut s = state.borrow_mut();
    // Also a target the hit killed, for the objects kept to the stun's end.
    if matches!(s.objects.get(&target), Some(Object::Unit(_))) && stun != (0, 0) {
        let hero = s.unit(Some(target)).is_some_and(|u| is_hero(u.type_id));
        // The impact comes at the end of the tick, and the stun counts from
        // there (c-RZH-1: bolt 943, order back at 1946).
        let at = s.event_time.unwrap_or_else(|| s.timers.time());
        super::cast::stun(
            &mut s,
            target,
            h,
            if hero { stun.1 } else { stun.0 },
            at,
            true,
        );
    }
}

/// A War Stomp's hit on `target`: its damage, then the objects of the stun
/// and the stun itself (c-SBI-1: per unit hit, event, stun, its order,
/// record and parts at 4.969878).
pub(super) fn stomp_hit(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: u32,
    target: u32,
    amount: u32,
    stun: (u32, u32),
    at: u32,
) {
    {
        let mut s = state.borrow_mut();
        if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
            super::movement::retaliate(&mut s, target, h, None);
        }
    }
    super::widgets::damage_widget(vm, state, Some(h), Some(target), amount, 0, 0);
    let mut s = state.borrow_mut();
    if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
        super::table::stun_objects(&mut s, h, target, Vec::new());
        // The stun counts from the effect (c-SBI-1: its end at 1595).
        let hero = s.unit(Some(target)).is_some_and(|u| is_hero(u.type_id));
        super::cast::stun(
            &mut s,
            target,
            h,
            if hero { stun.1 } else { stun.0 },
            at,
            true,
        );
    }
}

/// Whether the hit of attack `a` is due in this tick: by the expiry of
/// its damage-point timer against the queue's time at the end of the tick,
/// else by its tick.
fn due(state: &State, a: &Attacking) -> bool {
    match a.hit_at {
        Some(at) => f32::from_bits(at) <= f32::from_bits(state.timers.time()),
        None => a.next <= state.ticks,
    }
}

/// Whether unit `target`, with evasion (`AEev`), evades a hit: one more
/// output of game generator 2, after the attacker's dice, below `DataA`
/// of its level (SPEC-0036). A paused unit does not roll.
fn evades(state: &mut State, target: u32) -> bool {
    let chance = {
        let Some(u) = state.unit(Some(target)).filter(|u| !u.paused) else {
            return false;
        };
        let evasion = i32::from_be_bytes(*b"AEev");
        let data = &state.data;
        let skill = u
            .skills
            .iter()
            .copied()
            .chain(
                data.units
                    .get(&u.type_id)
                    .into_iter()
                    .flat_map(|t| t.abilities.iter().map(|&a| (a, 1))),
            )
            .find(|&(a, level)| {
                level > 0 && data.abilities.get(&a).is_some_and(|d| d.code == evasion)
            });
        let Some((ability, level)) = skill else {
            return false;
        };
        data.abilities
            .get(&ability)
            .and_then(|d| d.per_level.get((level - 1).max(0) as usize))
            .map(|n| n.data[0])
    };
    let Some(chance) = chance else {
        return false;
    };
    let x = state.game_random[ATTACK_DICE].fraction();
    f32::from_bits(x) < f32::from_bits(chance)
}

/// The zero hits of this tick after stuns (SPEC-0037): the stunner deals 0
/// damage to the unit, a hit as any other, after the unit's check of that
/// time (c-SAK-1: the Mountain King on kobold 10 at 1745).
pub(super) fn zero_hits(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let due: Vec<(u32, u32)> = {
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let units = s.units.clone();
        units
            .into_iter()
            .filter_map(|h| match s.objects.get_mut(&h) {
                Some(Object::Unit(u)) if u.alive && u.zero_hit == Some(tick) => {
                    u.zero_hit = None;
                    u.stunner.map(|source| (source, h))
                }
                _ => None,
            })
            .collect()
    };
    // In the order the stuns were made (c-SBI-1: War Stomp's four at 1599).
    let mut due: Vec<(u32, u32)> = due;
    {
        let s = state.borrow();
        due.sort_by_key(|&(_, h)| {
            s.unit(Some(h))
                .and_then(|u| u.entries.order_back)
                .map_or(u32::MAX, |(o, _)| s.table.creation_of(o))
        });
    }
    for (source, target) in due {
        super::widgets::damage_widget(vm, state, Some(source), Some(target), 0, 0, 0);
        super::table::order_back_freed(&mut state.borrow_mut(), Some(target));
        let mut s = state.borrow_mut();
        if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
            super::movement::retaliate(&mut s, target, source, None);
        }
    }
    // The others' (no zero hit: not a creep, or dead).
    super::table::order_back_freed(&mut state.borrow_mut(), None);
}

/// Hero `h`'s agility changed from `old` at the queue time `now`: the time
/// left of its running cooldown is multiplied by the old attack-speed
/// factor over the new, and a swing waiting for that cooldown starts at
/// the new end with the damage point of the new factor (SPEC-0034;
/// c-SAK-1: the Mountain King at his level-up in 6053, 0.121019 left
/// becoming 0.117236).
///
/// PROVISIONAL: a damage point already running was not seen.
pub(super) fn agility_changed(state: &mut State, h: u32, old: i32, now: u32) {
    let Some(u) = state.unit(Some(h)).filter(|u| is_hero(u.type_id)) else {
        return;
    };
    let bonus = state.combat.heroes.agi_attack_speed;
    let factor = |agility: i32| add(1.0f32.to_bits(), mul(bonus, ow3_math::i2r(agility)));
    let ratio = div(factor(old), factor(u.agility));
    // So is the rest of its backswing (c-SBL-1: the Mountain King's after
    // the hit of 2.3568838 that levelled him, 0x3F0864BB -> 0x3F06318C;
    // c-SBO-1: the Warden's at 11.569, ending 12.004086).
    let back = u.backswing_end;
    if f32::from_bits(back) > f32::from_bits(now)
        && let Some(Object::Unit(u)) = state.objects.get_mut(&h)
    {
        let end = add(now, mul(sub(back, now), ratio));
        let later = ticks_between(back, end);
        u.backswing_end = end;
        if let Some(a) = u.attack.as_mut()
            && let Some(r) = a.resume
        {
            a.resume = Some(shift(r, later));
        }
        // With it what waits for its end (`table::script_attack_lost`).
        super::table::backswing_moved(state, h, back, end);
    }
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let end = u.cooldown_end;
    if f32::from_bits(end) <= f32::from_bits(now) {
        return;
    }
    let new_end = add(now, mul(sub(end, now), ratio));
    let point = u.attack.filter(|a| a.swing_q == end).map(|a| {
        let weapon = weapon_for(state, u.type_id, Some(a.target)).unwrap_or_default();
        slowed(state, h, hero_cooldown(state, h, weapon.damage_point))
    });
    let shift =
        |from: u32, to: u32| (f64::from(f32::from_bits(sub(to, from))) * 200.0).round() as i64;
    let moved = |tick: u64, by: i64| (tick as i64 + by).max(0) as u64;
    let Some(Object::Unit(u)) = state.objects.get_mut(&h) else {
        return;
    };
    u.cooldown_end = new_end;
    if let (Some(point), Some(a)) = (point, u.attack.as_mut()) {
        let hit = add(new_end, point);
        let by = a
            .hit_at
            .map_or(shift(end, new_end), |old_hit| shift(old_hit, hit));
        a.swing_q = new_end;
        a.hit_at = Some(hit);
        a.next = moved(a.next, by);
        u.weapon_ready = moved(u.weapon_ready, by);
    } else {
        u.weapon_ready = moved(u.weapon_ready, shift(end, new_end));
    }
}

/// The attack of `h` ends at the queue time `now` with the damage point of
/// its swing reached but not dealt (its target died at that time): the
/// damage point is dropped and the cooldown counts on from the swing's
/// start. A damage point still ahead is cancelled with the cooldown, as
/// before (SPEC-0016, "Timing"; c-SAK-1: footman 6, whose damage point on
/// ogre 13 came at footman 4's fatal hit in 6053, swung next at 6223).
pub(super) fn swing_cut(state: &mut State, h: u32, now: u32) {
    let Some(u) = state.unit(Some(h)) else {
        return;
    };
    let Some(a) = u.attack else {
        return;
    };
    let reached = |t: u32| f32::from_bits(t) <= f32::from_bits(now);
    if !reached(a.swing_q) || !a.hit_at.is_some_and(reached) {
        return;
    }
    let Some(weapon) = weapon_for(state, u.type_id, Some(a.target)) else {
        return;
    };
    let cooldown = slowed(state, h, hero_cooldown(state, h, weapon.cooldown));
    state.cooldown_starts += 1;
    let seq = state.cooldown_starts;
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.cooldown_seq = seq;
        u.cooldown_end = add(a.swing_q, cooldown);
        u.weapon_ready = a.next + ticks(cooldown);
    }
}

/// The earliest expiry of the hits due in this tick, if any.
pub(super) fn next_hit(state: &State) -> Option<u32> {
    state
        .attacking
        .iter()
        .filter_map(|&h| {
            state
                .unit(Some(h))
                .filter(|u| u.alive)
                .and_then(|u| u.attack)
        })
        .filter(|a| due(state, a))
        .filter_map(|a| a.hit_at)
        .min_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)))
}

/// The earliest end of a backswing whose goal `after_swing_walks` takes
/// before `before`: the hits due ahead of it come first (c-RXR-1: a hit
/// of 4.25993 before footman 3's goal at 4.259997).
pub(super) fn first_swing_walk(state: &State, before: Option<u32>) -> Option<u32> {
    swing_walks_due(state, before)
        .into_iter()
        .map(|(_, e)| e)
        .min_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)))
}

/// The units whose goal after the swing is due before `before`, with
/// their backswing ends.
fn swing_walks_due(state: &State, before: Option<u32>) -> Vec<(u32, u32)> {
    let tick = state.ticks;
    state
        .attacking
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| u.alive)?;
            let a = u.attack?;
            let due = before.is_none_or(|b| f32::from_bits(u.backswing_end) < f32::from_bits(b));
            // Also a backswing whose end, a tick later by the tick count,
            // falls before this tick's end on the queue (c-SAK-1: the
            // Mountain King's at 41.520134 in the tick ending 41.520397).
            let in_tick = a.resume == Some(tick + 2)
                && f32::from_bits(u.backswing_end)
                    <= f32::from_bits(add(state.move_clock, crate::timers::TICK));
            ((a.resume == Some(tick + 1) || in_tick)
                && standing(state, a.target)
                && due
                && !u.entries.early_walk
                && !u.entries.early_goal
                // Not a stunned unit (c-SAK-1: kobold 14 at 4619).
                && !super::cast::stunned(state, h))
            .then_some((h, u.backswing_end))
        })
        .collect()
}

/// The walks of the goals after the swings whose backswings end in this
/// tick: the game takes the goal at the backswing's end, a tick before the
/// chase here (SPEC-0024; c-SAK-1: kobold 8 at 1895, before the frees of
/// that tick); with `before`, those ending before that queue time, as
/// timers of the queue ahead of the script timer due then (c-SAK-1: kobold
/// 15 at 9.499967 before the script order at 9.5 in 1901).
pub(super) fn after_swing_walks(state: &mut State, before: Option<u32>) {
    let tick = state.ticks;
    let ending = swing_walks_due(state, before);
    // In the order of the ends, as the queue runs them.
    // Equal ends by the order their swings started (c-SAK-1: footmen 4
    // and 6 at 41.559853).
    let mut ending = ending;
    let seq = |h: u32| state.unit(Some(h)).map_or(0, |u| u.cooldown_seq);
    ending.sort_by(|a, b| {
        f32::from_bits(a.1)
            .total_cmp(&f32::from_bits(b.1))
            .then(seq(a.0).cmp(&seq(b.0)))
    });
    for (h, end) in ending {
        state.table_at(end);
        super::table::walk_made_at_order(state, h);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.entries.early_goal = true;
        }
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.entries.early_walk_q = end;
            u.entries.early_walk = true;
            u.entries.early_walk_at = tick;
        }
        // A goal in reach ends at the unit's first update that starts with
        // its heading on the target, which may come before the chase here
        // (c-SAK-1: the Mountain King, goal 3972, update 3973, freed 3979).
        let in_reach = state
            .unit(Some(h))
            .and_then(|u| u.attack)
            .is_some_and(|a| super::movement::reaches(state, h, a.target));
        if in_reach && let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.entries.quiet_walk = true;
        }
    }
    // A unit that took a new target in its backswing (by a check or a
    // call) takes its goal at the backswing's end (c-SAK-1: footmen 2 and
    // 6 on kobold 10 at 47.079254 and 47.108826).
    let next = add(state.move_clock, crate::timers::TICK);
    let mut chasing: Vec<(u32, u32)> = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state
                .unit(Some(h))
                .filter(|u| u.alive && u.attack.is_none())?;
            let m = u.movement?;
            let super::movement::Order::Attack { target } = m.order else {
                return None;
            };
            // One it can hit: an invulnerable target is lost (c-RZJ-1: kobold
            // on the Paladin's Divine Shield, back to its guard at 24.01697).
            standing(state, target).then_some(())?;
            let end = f32::from_bits(u.backswing_end);
            (end > f32::from_bits(state.move_clock)
                && end <= f32::from_bits(next)
                && before.is_none_or(|b| end < f32::from_bits(b))
                && u.entries.swing_goal_at != Some(u.backswing_end))
            .then_some((h, u.backswing_end))
        })
        .collect();
    chasing.sort_by(|a, b| f32::from_bits(a.1).total_cmp(&f32::from_bits(b.1)));
    for (h, end) in chasing {
        state.table_at(end);
        super::table::walk_made_at_order(state, h);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.entries.swing_goal_at = Some(end);
        }
    }
    attack_move_resumes(state, before);
    // Units whose target died in their backswing walk back at its end, in
    // the order of those ends (c-SAK-1: gnoll 12 at 5460, q 27.299561).
    let next = add(state.move_clock, crate::timers::TICK);
    let mut back: Vec<(u32, u32, [u32; 2])> = state
        .units
        .iter()
        .filter_map(|&h| {
            let u = state.unit(Some(h)).filter(|u| u.alive)?;
            u.guard_return?;
            let end = f32::from_bits(u.backswing_end);
            let due = end <= f32::from_bits(next) && before.is_none_or(|b| end < f32::from_bits(b));
            // Another unit too, at the end of a backswing in this tick
            // (c-SAK-1: the image 20 at 55.907589).
            let creep = u.owner == super::creeps::CREEPS;
            let in_tick = end > f32::from_bits(state.move_clock);
            (due && (creep || in_tick)).then(|| {
                let home = if creep {
                    u.creep.guard.unwrap_or(u.home)
                } else {
                    u.home
                };
                (h, u.backswing_end, home)
            })
        })
        .collect();
    back.sort_by(|a, b| f32::from_bits(a.1).total_cmp(&f32::from_bits(b.1)));
    for (h, end, home) in back {
        state.table_at(end);
        super::movement::guard_back(state, h, home);
    }
    state.table.now = tick;
}

/// The attack-moves going on at backswing ends due before `before` (all
/// of this tick without it), in the order of those ends.
pub(super) fn attack_move_resumes(state: &mut State, before: Option<u32>) {
    let tick = state.ticks;
    let mut resuming = super::movement::attack_moves_resuming(state, before);
    resuming.sort_by(|a, b| f32::from_bits(a.1).total_cmp(&f32::from_bits(b.1)));
    for (h, q) in resuming {
        state.table_at(q);
        super::table::attack_move_resumed(state, h);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.entries.early_walk = true;
            u.entries.early_walk_at = tick;
        }
    }
    state.table.now = tick;
}

/// The earliest expiry of a hit timed in this tick, due or not yet.
pub(super) fn first_hit_in_tick(state: &State) -> Option<u32> {
    let next = add(state.move_clock, crate::timers::TICK);
    state
        .attacking
        .iter()
        .filter_map(|&h| {
            state
                .unit(Some(h))
                .filter(|u| u.alive)
                .and_then(|u| u.attack)
        })
        .filter_map(|a| a.hit_at)
        .filter(|e| f32::from_bits(*e) <= f32::from_bits(next))
        .min_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)))
}

/// The earliest hit timed in this tick that expires before `limit`.
pub(super) fn next_hit_before(state: &State, limit: u32) -> Option<u32> {
    // Only those of this tick: a hit held back (by a stun) is not one.
    let now = f32::from_bits(state.move_clock);
    state
        .attacking
        .iter()
        .filter_map(|&h| {
            state
                .unit(Some(h))
                .filter(|u| u.alive)
                .and_then(|u| u.attack)
        })
        .filter_map(|a| a.hit_at)
        .filter(|e| f32::from_bits(*e) > now && f32::from_bits(*e) < f32::from_bits(limit))
        .min_by(|a, b| f32::from_bits(*a).total_cmp(&f32::from_bits(*b)))
}

/// The hits of this tick that expire before the script timer due at
/// `expiry`: the queue runs them first (SPEC-0018).
pub(super) fn hits_ahead_of(vm: &mut Vm, state: &Rc<RefCell<State>>, expiry: u32) {
    if next_hit_before(&state.borrow(), expiry).is_none() {
        return;
    }
    state.borrow_mut().timers.move_to(expiry);
    hits_before(vm, state, Some(expiry));
}
