//! The console of the viewer driving the simulation on `(12)IceCrown`:
//! the command cards of a peasant and a town hall, a farm built and a
//! peasant trained through them. Passes without checking anything when
//! the game data is absent.

use std::path::PathBuf;

use ow3_viewer::hud::{Card, Command};
use ow3_viewer::sim_units::Sim;

fn start() -> Option<Sim> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let map = dir.join("Maps/(12)IceCrown.w3m");
    if !map.exists() {
        return None;
    }
    Some(Sim::start(&dir, &map).unwrap())
}

/// The local player's first unit of type `code`.
fn first(sim: &Sim, code: &[u8; 4]) -> u32 {
    let t = i32::from_be_bytes(*code);
    sim.units()
        .into_iter()
        .find(|u| u.type_id == t && u.owner == 0)
        .unwrap()
        .handle
}

/// Unit `h` as the simulation shows it.
fn view(sim: &Sim, h: u32) -> ow3_sim::game::UnitView {
    sim.units().into_iter().find(|u| u.handle == h).unwrap()
}

#[test]
fn a_farm_waits_for_its_worker_and_takes_damage() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let hhou = i32::from_be_bytes(*b"hhou");
    let p = view(&sim, peasant);
    sim.run(peasant, Command::Build(hhou), Some([p.x + 400.0, p.y]))
        .unwrap();
    for _ in 0..60 {
        sim.advance(100_000);
    }
    let farm = sim
        .units()
        .into_iter()
        .find(|u| u.type_id == hhou && u.owner == 0)
        .unwrap()
        .handle;
    assert!(view(&sim, farm).constructing);
    // Sent away, the worker leaves the farm as it is.
    let p = view(&sim, peasant);
    sim.run(peasant, Command::Move, Some([p.x - 300.0, p.y]));
    sim.advance(100_000);
    let left = view(&sim, farm).life;
    for _ in 0..30 {
        sim.advance(100_000);
    }
    assert_eq!(view(&sim, farm).life, left);
    // Back on it, the work goes on and ends.
    sim.command(peasant, Some(farm), None, false).unwrap();
    for _ in 0..60 {
        sim.advance(100_000);
    }
    let grown = view(&sim, farm).life;
    assert!(grown > left, "{grown} after {left}");
    // A hit from its own worker takes life off as it grows.
    let other = sim
        .units()
        .into_iter()
        .find(|u| u.type_id == i32::from_be_bytes(*b"hpea") && u.owner == 0 && u.handle != peasant)
        .unwrap()
        .handle;
    sim.run_on(other, Command::Attack, farm).unwrap();
    for _ in 0..80 {
        sim.advance(100_000);
    }
    let hit = view(&sim, farm);
    assert!(hit.constructing);
    // 8 s of growth at the farm's rate.
    let undamaged = grown + 8.0 * 12.857_142;
    assert!(
        hit.life < undamaged - 4.0,
        "{} not below {undamaged}",
        hit.life
    );
    sim.command(other, None, Some([p.x, p.y, 0.0]), false);
    for _ in 0..400 {
        sim.advance(100_000);
    }
    let done = view(&sim, farm);
    assert!(!done.constructing);
}

#[test]
fn a_peasant_builds_a_farm_and_the_town_hall_trains_one() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let hall = first(&sim, b"htow");
    let s = sim.selected(peasant).unwrap();
    assert!(s.own);
    // The peasant's card: the orders, Build at its place.
    let buttons = sim.hud.buttons(Some(&s));
    let build = buttons
        .iter()
        .find(|b| b.command == Command::BuildMenu)
        .unwrap();
    assert_eq!(build.place, (0, 2));
    assert!(
        buttons
            .iter()
            .any(|b| b.command == Command::Move && b.place == (0, 0))
    );
    // Its list of buildings has the farm.
    sim.run(peasant, Command::BuildMenu, None);
    assert_eq!(sim.hud.card, Card::Build);
    let hhou = i32::from_be_bytes(*b"hhou");
    let farm = sim
        .hud
        .buttons(Some(&s))
        .into_iter()
        .find(|b| b.command == Command::Build(hhou))
        .unwrap();
    assert!(farm.hotkey.is_some());
    // Built 400 to the east of the peasant.
    let p = sim
        .units()
        .into_iter()
        .find(|u| u.handle == peasant)
        .unwrap();
    let line = sim
        .run(peasant, Command::Build(hhou), Some([p.x + 400.0, p.y]))
        .unwrap();
    assert!(line.ends_with("taken"), "{line}");
    assert_eq!(sim.hud.card, Card::Main);
    assert_eq!(sim.resources().gold, 500 - 80);
    for _ in 0..40 {
        sim.advance(100_000);
    }
    assert!(
        sim.units()
            .iter()
            .any(|u| u.type_id == hhou && u.owner == 0)
    );
    // The town hall trains a peasant.
    let h = sim.selected(hall).unwrap();
    let hpea = i32::from_be_bytes(*b"hpea");
    assert!(
        sim.hud
            .buttons(Some(&h))
            .iter()
            .any(|b| b.command == Command::Train(hpea) && b.place == (0, 0))
    );
    let before = sim.units().iter().filter(|u| u.type_id == hpea).count();
    let line = sim.run(hall, Command::Train(hpea), None).unwrap();
    assert!(line.ends_with("taken"), "{line}");
    for _ in 0..170 {
        sim.advance(100_000);
    }
    let after = sim.units().iter().filter(|u| u.type_id == hpea).count();
    assert_eq!(after, before + 1);
}
