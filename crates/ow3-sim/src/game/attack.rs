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
use ow3_math::natives::sqrt;
use ow3_math::{add, div, mul, sub};

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
    resume: Option<u64>,
    /// The attacker swings again, or takes a new goal and stands, at its
    /// first update from this tick on: the cooldown less the damage point
    /// after the last hit (SPEC-0019, "After the swing").
    pub(super) renew_at: Option<u64>,
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
        Some(Object::Unit(u)) => u.alive,
        Some(Object::Destructable(d)) => d.alive,
        Some(Object::Item(i)) => f32::from_bits(i.life) > 0.0,
        _ => false,
    }
}

/// Unit `h` starts hitting `target` at tick `tick`: the first hit comes its
/// weapon's damage point later (SPEC-0019).
pub(super) fn begin(state: &mut State, h: u32, target: u32, tick: u64, order_id: i32) {
    let damage_point = match state.objects.get(&h) {
        Some(Object::Unit(u)) => {
            weapon_for(state, u.type_id, Some(target)).map_or(1, |w| ticks(w.damage_point))
        }
        _ => return,
    };
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.attack = Some(Attacking {
            target,
            next: (tick + damage_point).max(u.weapon_ready),
            order_id,
            resume: None,
            renew_at: None,
        });
    }
    if !state.attacking.contains(&h) {
        state.attacking.push(h);
    }
}

/// The hits due at this tick, in the order the attacks began.
pub(super) fn hits(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    fly(vm, state);
    let attackers = state.borrow().attacking.clone();
    for h in attackers {
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
                        let point = weapon_for(s, u.type_id, Some(a.target))
                            .map_or(1, |w| ticks(w.damage_point));
                        (a, a.next.saturating_sub(point))
                    })
                }
                _ => None,
            };
            // A target out of reach when the next swing would start, or at
            // the new goal after the swing: chased from the new goal on,
            // the attacker standing until then (SPEC-0019).
            if let Some((a, start)) = next_swing
                && (start == tick || (s.update_now && a.renew_at.is_some_and(|r| tick >= r)))
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
                    super::movement::chase(s, h, a.target, a.order_id, None);
                }
                continue;
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
                Some(a) if a.next <= tick && standing(s, a.target) => {
                    let data = s.data.units.get(&type_id);
                    let weapon = weapon_for(s, type_id, Some(a.target)).unwrap_or_default();
                    let primary = data.filter(|_| is_hero(type_id)).map_or(0, |t| t.primary);
                    let amount = hit_amount(&weapon, primary, &mut s.game_random[ATTACK_DICE]);
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        let next = a.next + ticks(weapon.cooldown);
                        let end = tick + ticks(weapon.backswing);
                        let resume = Some(end + 1);
                        // The next swing, and with it the new goal of a
                        // chase, at the first update from `D + cool -
                        // dmgpt` on (SPEC-0019).
                        let renew_at = Some(
                            (tick + ticks(weapon.cooldown))
                                .saturating_sub(ticks(weapon.damage_point)),
                        );
                        u.attack = Some(Attacking {
                            next,
                            resume,
                            renew_at,
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
                        )
                    {
                        s.missiles.push(missile);
                        None
                    } else {
                        Some((a.target, amount, weapon.attack_type, weapon.area))
                    }
                }
                Some(a) if a.next <= tick => {
                    // The target is gone.
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        u.attack = None;
                    }
                    None
                }
                _ => None,
            }
        };
        if let Some((target, amount, attack_type, area)) = hit {
            strike(vm, state, h, target, amount, attack_type, area);
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
            super::movement::chase(&mut s, h, a.target, a.order_id, a.renew_at);
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

/// The damage of a hit of `h` on `target`, by its swing or its missile:
/// the units round the target first, then the target, and the answers
/// to it (SPEC-0016, SPEC-0019).
fn strike(
    vm: &mut Vm,
    state: &Rc<RefCell<State>>,
    h: u32,
    target: u32,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
) {
    // The units round the target, before the target takes its hit.
    let splashed = area
        .as_ref()
        .map(|area| splash(&state.borrow(), h, target, area, amount))
        .unwrap_or_default();
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
    // The victim and its friends answer the hit (SPEC-0019).
    if matches!(s.objects.get(&target), Some(Object::Unit(u)) if u.alive) {
        super::movement::retaliate(&mut s, target, h);
    }
    if !standing(&s, target) {
        let tick = s.ticks;
        let backswing = match s.objects.get(&h) {
            Some(Object::Unit(u)) => {
                weapon_for(&s, u.type_id, Some(target)).map_or(1, |w| ticks(w.backswing))
            }
            _ => 1,
        };
        super::movement::killed(&mut s, h, tick, backswing);
    }
}
/// A missile in flight (SPEC-0016, "Missiles"): it moves `step` world
/// units a tick towards the target's centre, or for artillery towards
/// the point where the target stood at the launch, and strikes when it
/// reaches it.
#[derive(Clone, Debug, PartialEq)]
pub struct Missile {
    attacker: u32,
    target: u32,
    at: [u32; 2],
    step: u32,
    /// The point a shell of artillery goes to.
    ground: Option<[u32; 2]>,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
}

/// Below this distance from the launch point (90) the damage comes at the
/// launch.
const NO_FLIGHT: f32 = 90.0;

/// The missile of `h` launched now at `target`, if its type has a missile
/// speed and the target is 90 or more from the launch point. The launch
/// point is `launchY` ahead and `launchX` to the side.
///
/// PROVISIONAL: "ahead" is taken as the direction to the target (the
/// attacker faces it within 0.2 radians when it swings) and the side as
/// its right (SPEC-0016 measured only targets straight ahead).
fn launch(
    state: &State,
    h: u32,
    target: u32,
    amount: u32,
    attack_type: i32,
    area: Option<Area>,
    artillery: bool,
) -> Option<Missile> {
    let u = state.unit(Some(h))?;
    let data = state.data.units.get(&u.type_id)?;
    if f32::from_bits(data.missile_speed) <= 0.0 {
        return None;
    }
    let to = super::movement::position_of(state, target)?;
    let (dx, dy) = (sub(to[0], u.x), sub(to[1], u.y));
    let length = sqrt(add(mul(dx, dx), mul(dy, dy)));
    let (fx, fy) = if f32::from_bits(length) > 0.0 {
        (div(dx, length), div(dy, length))
    } else {
        (1.0f32.to_bits(), 0)
    };
    let [side, ahead] = data.launch;
    // Forward (fx, fy) and to the right (fy, -fx).
    let at = [
        add(u.x, add(mul(fx, ahead), mul(fy, side))),
        add(u.y, sub(mul(fy, ahead), mul(fx, side))),
    ];
    let (lx, ly) = (sub(to[0], at[0]), sub(to[1], at[1]));
    let flight = f32::from_bits(sqrt(add(mul(lx, lx), mul(ly, ly))));
    if flight < NO_FLIGHT {
        return None;
    }
    Some(Missile {
        attacker: h,
        target,
        at,
        step: div(data.missile_speed, 200.0f32.to_bits()),
        ground: artillery.then_some(to),
        amount,
        attack_type,
        area,
    })
}

/// The missiles of this tick move on; those that arrive strike, in the
/// order of their launch. A missile whose target died or is gone is lost;
/// a shell of artillery goes on to its point.
///
/// PROVISIONAL: how a missile follows a target that moves fast past the
/// attacker is open (SPEC-0016, open question 9).
fn fly(vm: &mut Vm, state: &Rc<RefCell<State>>) {
    let arrived = {
        let mut s = state.borrow_mut();
        let s = &mut *s;
        let mut arrived = Vec::new();
        let mut flying = Vec::with_capacity(s.missiles.len());
        for mut m in std::mem::take(&mut s.missiles) {
            let to = match m.ground {
                Some(point) => point,
                None if standing(s, m.target) => {
                    let Some(to) = super::movement::position_of(s, m.target) else {
                        continue;
                    };
                    to
                }
                None => continue,
            };
            let (dx, dy) = (sub(to[0], m.at[0]), sub(to[1], m.at[1]));
            let length = sqrt(add(mul(dx, dx), mul(dy, dy)));
            if f32::from_bits(length) <= f32::from_bits(m.step) {
                m.at = to;
                arrived.push(m);
                continue;
            }
            let scale = div(m.step, length);
            m.at = [add(m.at[0], mul(dx, scale)), add(m.at[1], mul(dy, scale))];
            flying.push(m);
        }
        s.missiles = flying;
        arrived
    };
    for m in arrived {
        match (m.ground, &m.area) {
            // A shell hits the units round its point by their distances.
            (Some(point), Some(area)) => {
                let hit = splash_at(&state.borrow(), m.attacker, point, None, area, m.amount);
                for (victim, share) in hit {
                    strike(vm, state, m.attacker, victim, share, m.attack_type, None);
                }
            }
            (Some(_), None) => {}
            (None, _) => strike(
                vm,
                state,
                m.attacker,
                m.target,
                m.amount,
                m.attack_type,
                m.area,
            ),
        }
    }
}

/// A missile in flight as a viewer sees it: the type of the unit that
/// launched it, where it is (world units) and where it goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MissileView {
    pub type_id: i32,
    pub at: [f32; 2],
    pub towards: [f32; 2],
}

/// The missiles in flight.
pub(super) fn views(state: &State) -> Vec<MissileView> {
    state
        .missiles
        .iter()
        .filter_map(|m| {
            let type_id = state.unit(Some(m.attacker)).map_or(0, |u| u.type_id);
            let to = m
                .ground
                .or_else(|| super::movement::position_of(state, m.target))?;
            Some(MissileView {
                type_id,
                at: m.at.map(f32::from_bits),
                towards: to.map(f32::from_bits),
            })
        })
        .collect()
}
