//! Attacks of units (SPEC-0016).
//!
//! `IssueTargetOrder(u, "attack", target)` makes a unit with a weapon hit
//! the target until one of them dies or the unit gets another order. Each
//! hit does the damage of SPEC-0015 for the amount of
//! [`crate::combat::hit_amount`], with the dice from game generator 2, and
//! hits follow `cool1` apart.
//!
//! PROVISIONAL (SPEC-0016 open question 1): the first hit comes `dmgpt1`
//! after the order; units do not move, turn or check their range, so any
//! target is hit where it is; hits come after the timers of their tick.
//! Only the string orders "attack" and "stop" are known.

use std::cell::RefCell;
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};

use super::{Object, State, handle, string};
use crate::combat::{DAMAGE_TYPE_NORMAL, hit_amount};
use crate::data::is_hero;
use crate::random::{ATTACK_DICE, ATTACK_START};

/// A unit's attack on a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attacking {
    target: u32,
    /// The tick of the next hit.
    next: u64,
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
fn standing(state: &State, target: u32) -> bool {
    match state.objects.get(&target) {
        Some(Object::Unit(u)) => u.alive,
        Some(Object::Destructable(d)) => d.alive,
        Some(Object::Item(i)) => f32::from_bits(i.life) > 0.0,
        _ => false,
    }
}

/// Starts or stops the attack of unit `h`.
fn order(state: &mut State, h: u32, target: Option<u32>) -> bool {
    let tick = state.ticks;
    let weapon = match state.objects.get(&h) {
        Some(Object::Unit(u)) if u.alive => state
            .data
            .units
            .get(&u.type_id)
            .and_then(|t| t.weapon.clone()),
        _ => return false,
    };
    let attack = match (target, weapon) {
        (Some(target), Some(weapon)) if target != h && standing(state, target) => Some(Attacking {
            target,
            next: tick + ticks(weapon.damage_point),
        }),
        (Some(_), _) => return false,
        (None, _) => None,
    };
    if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
        u.attack = attack;
    }
    if attack.is_some() {
        // PROVISIONAL: the output is drawn at the order and not used.
        state.game_random[ATTACK_START].step();
    }
    if attack.is_some() && !state.attacking.contains(&h) {
        state.attacking.push(h);
    }
    true
}

pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    native!(
        add,
        state,
        "native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean",
        |vm, state, args| {
            let (Some(h), Some("attack")) = (handle(&args[0]), string(&args[1])) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let done = order(&mut state.borrow_mut(), h, handle(&args[2]));
            Ok(Some(Value::Boolean(done)))
        }
    );
    native!(
        add,
        state,
        "native IssueImmediateOrder takes unit whichUnit, string order returns boolean",
        |vm, state, args| {
            let (Some(h), Some("stop")) = (handle(&args[0]), string(&args[1])) else {
                return Ok(Some(Value::Boolean(false)));
            };
            let done = order(&mut state.borrow_mut(), h, None);
            Ok(Some(Value::Boolean(done)))
        }
    );
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
                        u.attack = Some(Attacking {
                            next: a.next + ticks(weapon.cooldown),
                            ..a
                        });
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
        }
    }
    let mut s = state.borrow_mut();
    let s = &mut *s;
    s.attacking.retain(
        |h| matches!(s.objects.get(h), Some(Object::Unit(u)) if u.alive && u.attack.is_some()),
    );
}
