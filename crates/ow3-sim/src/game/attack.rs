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
use crate::data::{Area, Weapon, is_hero};
use crate::random::ATTACK_DICE;

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
                    Some((a.target, amount, weapon.attack_type, weapon.area))
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
    let (Some(attacker), Some(t)) = (state.unit(Some(h)), state.unit(Some(target))) else {
        return Vec::new();
    };
    let (x, y) = (f32::from_bits(t.x), f32::from_bits(t.y));
    let enemies_only = area.targets.iter().any(|c| c == "enemy");
    let mut out = Vec::new();
    for &other in &state.units {
        if other == target {
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
