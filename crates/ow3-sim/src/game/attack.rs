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
use crate::data::is_hero;
use crate::random::ATTACK_DICE;

/// A unit's attack on a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attacking {
    target: u32,
    /// The tick of the next hit.
    next: u64,
    /// The order id `GetUnitCurrentOrder` gives: 0 for an attack the unit
    /// began on its own (SPEC-0019).
    pub(super) order_id: i32,
    /// The tick after the end of the last swing, when a target out of
    /// reach is chased again (SPEC-0019).
    resume: Option<u64>,
}

impl Attacking {
    /// The attack as a viewer sees it, for a unit of type `type_id`.
    pub(super) fn view(&self, state: &State, type_id: i32) -> super::AttackView {
        let weapon = state
            .data
            .units
            .get(&type_id)
            .and_then(|t| t.weapon.clone())
            .unwrap_or_default();
        super::AttackView {
            target: self.target,
            next_hit: self.next,
            cooldown: ticks(weapon.cooldown),
            damage_point: ticks(weapon.damage_point),
        }
    }
}

/// Seconds as a whole number of ticks of 0.005 s, at least one.
fn ticks(seconds: u32) -> u64 {
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
        Some(Object::Unit(u)) => state
            .data
            .units
            .get(&u.type_id)
            .and_then(|t| t.weapon.as_ref())
            .map_or(1, |w| ticks(w.damage_point)),
        _ => return,
    };
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.attack = Some(Attacking {
            target,
            next: (tick + damage_point).max(u.weapon_ready),
            order_id,
            resume: None,
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
                    let weapon = data.and_then(|t| t.weapon.clone()).unwrap_or_default();
                    let primary = data.filter(|_| is_hero(type_id)).map_or(0, |t| t.primary);
                    let amount = hit_amount(&weapon, primary, &mut s.game_random[ATTACK_DICE]);
                    if let Some(Object::Unit(u)) = s.objects.get_mut(&h) {
                        let next = a.next + ticks(weapon.cooldown);
                        let resume = Some(tick + ticks(weapon.backswing) + 1);
                        u.attack = Some(Attacking { next, resume, ..a });
                        u.weapon_ready = next;
                    }
                    Some((a.target, amount, weapon.attack_type))
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
        if let Some((target, amount, attack_type)) = hit {
            super::widgets::damage_widget(
                vm,
                state,
                Some(h),
                Some(target),
                amount,
                attack_type,
                DAMAGE_TYPE_NORMAL,
            );
            // A kill on an attack-move: the unit walks on after its
            // backswing (SPEC-0019).
            let mut s = state.borrow_mut();
            if !standing(&s, target) {
                let tick = s.ticks;
                let backswing = match s.objects.get(&h) {
                    Some(Object::Unit(u)) => s
                        .data
                        .units
                        .get(&u.type_id)
                        .and_then(|t| t.weapon.as_ref())
                        .map_or(1, |w| ticks(w.backswing)),
                    _ => 1,
                };
                super::movement::killed(&mut s, h, tick, backswing);
            }
        }
        // One tick after the swing a target that has left the attacker's
        // reach is chased.
        let mut s = state.borrow_mut();
        let tick = s.ticks;
        let resumed = match s.objects.get(&h) {
            Some(Object::Unit(u)) if u.alive => u.attack.filter(|a| a.resume == Some(tick)),
            _ => None,
        };
        if let Some(a) = resumed
            && standing(&s, a.target)
            && !super::movement::reaches(&s, h, a.target)
        {
            super::movement::chase(&mut s, h, a.target, a.order_id);
        }
    }
    let mut s = state.borrow_mut();
    let s = &mut *s;
    s.attacking.retain(
        |h| matches!(s.objects.get(h), Some(Object::Unit(u)) if u.alive && u.attack.is_some()),
    );
}
