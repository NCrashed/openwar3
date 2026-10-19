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

/// A place for a farm, 512 east of the town hall: the peasants and the
/// gold mine of the start location are to its west.
fn east_of_hall(sim: &Sim) -> (f32, f32) {
    let hall = view(sim, first(sim, b"htow"));
    (hall.x + 512.0, hall.y)
}

/// Unit `h` as the simulation shows it.
fn view(sim: &Sim, h: u32) -> ow3_sim::game::UnitView {
    sim.units().into_iter().find(|u| u.handle == h).unwrap()
}

#[test]
fn a_farm_is_not_placed_over_the_town_hall() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let hall = view(&sim, first(&sim, b"htow"));
    let hhou = i32::from_be_bytes(*b"hhou");
    let line = sim
        .run(peasant, Command::Build(hhou), Some([hall.x + 64.0, hall.y]))
        .unwrap();
    assert!(line.ends_with("refused"), "{line}");
    assert_eq!(sim.resources().gold, 500);
    // While the farm waits for its place it shows red there, green on
    // open ground.
    sim.hud.pending = Some(Command::Build(hhou));
    let tint = |sim: &mut Sim, at: [f32; 2]| {
        sim.ghost = Some(at);
        sim.draws()
            .into_iter()
            .find(|d| d.handle == 0)
            .map(|d| d.tint)
    };
    assert_eq!(
        tint(&mut sim, [hall.x + 64.0, hall.y]),
        Some([1.0, 0.25, 0.25])
    );
    let east = east_of_hall(&sim);
    assert_eq!(tint(&mut sim, [east.0, east.1]), Some([0.3, 1.0, 0.3]));
}

#[test]
fn a_farm_waits_for_its_worker_and_takes_damage() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let hhou = i32::from_be_bytes(*b"hhou");
    let east = east_of_hall(&sim);
    sim.run(peasant, Command::Build(hhou), Some([east.0, east.1]))
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
    // Built east of the town hall.
    let east = east_of_hall(&sim);
    let line = sim
        .run(peasant, Command::Build(hhou), Some([east.0, east.1]))
        .unwrap();
    assert!(line.ends_with("taken"), "{line}");
    assert_eq!(sim.hud.card, Card::Main);
    assert_eq!(sim.resources().gold, 500 - 80);
    for _ in 0..100 {
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

#[test]
fn a_peasant_gathers_gold() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let ngol = i32::from_be_bytes(*b"ngol");
    // The mine of the start location, nearest the town hall.
    let p = view(&sim, first(&sim, b"htow"));
    let mine = sim
        .units()
        .into_iter()
        .filter(|u| u.type_id == ngol && u.alive)
        .min_by(|a, b| {
            let d = |u: &ow3_sim::game::UnitView| (u.x - p.x).powi(2) + (u.y - p.y).powi(2);
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let before = mine.resources;
    assert!(before > 0);
    // The card of a worker has gather at (3,1) (SPEC-0028).
    let s = sim.selected(peasant).unwrap();
    assert!(
        sim.hud
            .buttons(Some(&s))
            .iter()
            .any(|b| b.command == Command::Gather && b.place == (3, 1))
    );
    sim.command(peasant, Some(mine.handle), None, false)
        .unwrap();
    assert_eq!(view(&sim, peasant).order, 852_018);
    // The start location's own mine, some 770 from the town hall.
    let d = ((mine.x - p.x).powi(2) + (mine.y - p.y).powi(2)).sqrt();
    assert!(d < 1000.0, "{d}");
    // In, out with 10 gold (the mine 10 down), and back to the town hall.
    let mut hidden = false;
    let mut out = false;
    for _ in 0..400 {
        sim.advance(100_000);
        let v = view(&sim, peasant);
        hidden |= v.hidden;
        if !out && v.carrying == Some((1, 10)) {
            out = true;
            assert_eq!(v.order, 852_017);
            assert_eq!(view(&sim, mine.handle).resources, before - 10);
            // With the load the card offers to take it back, at (3,1).
            let s = sim.selected(peasant).unwrap();
            assert!(
                sim.hud
                    .buttons(Some(&s))
                    .iter()
                    .any(|b| b.command == Command::ReturnGoods && b.place == (3, 1))
            );
        }
    }
    assert!(hidden && out);
    // Trip after trip in 40 s.
    let trips = (sim.resources().gold - 500) / 10;
    assert!(trips >= 3, "{trips} trips");
    let carried = view(&sim, peasant).carrying.map_or(0, |(_, n)| n);
    assert_eq!(
        view(&sim, mine.handle).resources,
        before - 10 * trips - carried
    );
}

#[test]
fn a_peasant_gathers_lumber() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let hall = view(&sim, first(&sim, b"htow"));
    let (tree, tx, ty) = sim.tree_near(hall.x, hall.y, 3000.0).unwrap();
    assert!(sim.harvest(peasant, tree));
    let lumber = sim.resources().lumber;
    let mut full = false;
    let mut chopped = false;
    let mut swung = false;
    let mut faced = false;
    // Steps of 50 ms, to see the swings.
    for _ in 0..1800 {
        sim.advance(50_000);
        let v = view(&sim, peasant);
        full |= v.carrying == Some((2, 10)) && v.order == 852_017;
        if v.chopping.is_some() {
            chopped = true;
            // It faces the tree and swings at it.
            let to = (ty - v.y).atan2(tx - v.x);
            let off = (v.facing - to + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            faced |= off.abs() < 0.2;
            swung |= sim.draws().iter().any(|d| {
                d.handle == peasant && matches!(d.pose, ow3_viewer::models::UnitPose::Attack(_))
            });
        }
        if sim.resources().lumber > lumber {
            break;
        }
    }
    assert!(
        full && chopped && faced && swung,
        "{full} {chopped} {faced} {swung}"
    );
    assert_eq!(sim.resources().lumber, lumber + 10);
    assert_eq!(view(&sim, peasant).order, 852_018);
}

#[test]
fn a_creep_chases_and_goes_home() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let nfrp = i32::from_be_bytes(*b"nfrp");
    let p = view(&sim, peasant);
    let creep = sim
        .units()
        .into_iter()
        .filter(|u| u.type_id == nfrp && u.alive)
        .min_by(|a, b| {
            let d = |u: &ow3_sim::game::UnitView| (u.x - p.x).powi(2) + (u.y - p.y).powi(2);
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let home = (creep.x, creep.y);
    // The peasant walks up to the creep, then runs back.
    sim.run(peasant, Command::Move, Some([home.0, home.1 + 300.0]));
    let mut farthest = 0.0f32;
    let mut went_home = false;
    let mut ran = false;
    for _ in 0..900 {
        sim.advance(100_000);
        let c = view(&sim, creep.handle);
        let away = ((c.x - home.0).powi(2) + (c.y - home.1).powi(2)).sqrt();
        farthest = farthest.max(away);
        went_home |= c.order == 851_986;
        if !ran && (view(&sim, peasant).y - home.1) < 380.0 {
            ran = true;
            sim.run(peasant, Command::Move, Some([p.x, p.y]));
        }
    }
    let c = view(&sim, creep.handle);
    let away = ((c.x - home.0).powi(2) + (c.y - home.1).powi(2)).sqrt();
    assert!(farthest > 600.0, "{farthest}");
    assert!(went_home);
    assert!(away < 20.0, "{away}");
}

/// A peasant walking with gold shows a geoset (the sack) that its plain
/// walk hides, and so with lumber.
#[test]
fn a_load_shows_on_the_peasant() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    if !dir.join("War3.mpq").exists() {
        return;
    }
    use ow3_viewer::models::{Animation, UnitPose};
    let mut chain = ow3_formats::mpq::ArchiveChain::game(&dir).unwrap();
    let mut animation = Animation::default();
    let peasant = animation
        .model(&mut chain, "Units\\Human\\Peasant\\Peasant.mdx")
        .unwrap();
    animation.unit_parts(peasant, 0);
    let walk = animation
        .pose_alphas(peasant, UnitPose::Walk(300))
        .unwrap()
        .0;
    for load in [1, 2] {
        let carry = animation
            .pose_alphas(peasant, UnitPose::Carry(300, load))
            .unwrap()
            .0;
        assert!(
            walk.iter().zip(&carry).any(|(w, c)| *w < 0.5 && *c > 0.5),
            "load {load}: {walk:?} {carry:?}"
        );
    }
}

/// The time of day indicator of the Human console stands in its Stand
/// sequence at `h / 24 * 60` s (SPEC-0028).
#[test]
fn the_time_indicator_follows_the_day() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let (model, t) = sim.time_indicator().unwrap();
    assert_eq!(
        model.to_ascii_lowercase(),
        "ui\\console\\human\\humanui-timeindicator.mdx"
    );
    let expected = sim.time_of_day() / 24.0 * 60_000.0;
    assert!((t as f32 - expected).abs() <= 1.0, "{t} {expected}");
}
