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
    // It works touching the farm (4 x 4 cells, so its centre at most some
    // 64 + 16 + 32 diagonally away), facing it.
    let (w, f) = (view(&sim, peasant), view(&sim, farm));
    let d = ((w.x - f.x).powi(2) + (w.y - f.y).powi(2)).sqrt();
    assert!(d < 135.0, "{d} from the farm");
    let to = (f.y - w.y).atan2(f.x - w.x);
    let off = (w.facing - to + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    assert!(off.abs() < 0.2, "facing {off} off the farm");
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
    let mut shook = false;
    let mut swung_loaded = false;
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
            // After the first hit it swings with its lumber (SPEC-0029).
            swung_loaded |= sim.draws().iter().any(|d| {
                d.handle == peasant
                    && matches!(d.pose, ow3_viewer::models::UnitPose::LoadAttack(_, 2))
            });
        }
        // The tree shakes after each hit.
        shook |= sim
            .destructable_poses()
            .iter()
            .any(|(_, pose)| matches!(pose, ow3_viewer::models::UnitPose::Hit(_)));
        if sim.resources().lumber > lumber {
            break;
        }
    }
    assert!(
        full && chopped && faced && swung && swung_loaded && shook,
        "{full} {chopped} {faced} {swung} {shook}"
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
    animation.unit_parts(peasant, 0, None);
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
        // Standing with it, it still shows.
        let stand = animation
            .pose_alphas(peasant, UnitPose::LoadStand(300, load))
            .unwrap()
            .0;
        assert!(
            walk.iter().zip(&stand).any(|(w, s)| *w < 0.5 && *s > 0.5),
            "load {load} standing: {walk:?} {stand:?}"
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

/// A tree the simulation reports hit is drawn on its own, in its Stand
/// Hit pose, and its static instance hidden.
#[test]
fn a_hit_tree_is_drawn_on_its_own() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let map = dir.join("Maps/(12)IceCrown.w3m");
    if !map.exists() {
        return;
    }
    let scene = ow3_viewer::scene::Scene::load(&dir, &map, true, None).unwrap();
    let loaded = ow3_sim::map::Map::load(&dir, &map).unwrap();
    let d = &loaded.destructables[0];
    let pose = ow3_viewer::models::UnitPose::Hit(100);
    let (draws, hidden) = scene.posed(&[([d.x, d.y], pose)]);
    assert_eq!(draws.len(), 1);
    assert_eq!(hidden.len(), 1);
    assert_eq!(draws[0].pose, pose);
}

/// A tree drawn on its own as well as in the scene shares the alpha slots
/// of its model, so that its Stand Hit shows it (it vanished for the time
/// of the hit when the slots were made twice).
#[test]
fn a_tree_drawn_on_its_own_stays_visible() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let map = dir.join("Maps/(12)IceCrown.w3m");
    if !map.exists() {
        return;
    }
    use ow3_viewer::models::UnitPose;
    let mut scene = ow3_viewer::scene::Scene::load(&dir, &map, true, None).unwrap();
    let mut chain = ow3_formats::mpq::ArchiveChain::open(&dir, &map).unwrap();
    let tree = scene
        .placements
        .iter()
        .find(|p| p.replaceable.is_some() && p.model.contains("Tree"))
        .unwrap()
        .clone();
    let index = scene.animation.model(&mut chain, &tree.model).unwrap();
    let slots = scene.animation.alpha_count();
    scene
        .animation
        .unit_parts(index, 0, tree.replaceable.as_ref());
    assert_eq!(scene.animation.alpha_count(), slots);
    let (values, _) = scene
        .animation
        .pose_alphas(index, UnitPose::Hit(100))
        .unwrap();
    assert!(values.iter().any(|&a| a > 0.5), "{values:?}");
}

/// The melee start puts the camera on the workers of the local player's
/// start location (`SetCameraPositionForPlayer` in Blizzard.j).
#[test]
fn the_camera_starts_at_the_workers() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let ([x, y], moves) = sim.camera_target().unwrap();
    assert!(moves >= 1);
    let hall = view(&sim, first(&sim, b"htow"));
    let d = ((x - hall.x).powi(2) + (y - hall.y).powi(2)).sqrt();
    assert!(d < 600.0, "{d} from the town hall");
}

/// Several units selected (SPEC-0028): a rectangle takes the player's
/// units without its buildings, a double click those of the same type
/// nearest first, a group order moves them all, and the information
/// panel shows the group.
#[test]
fn a_group_of_peasants() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let hpea = i32::from_be_bytes(*b"hpea");
    let units = sim.units();
    let peasants: Vec<u32> = units
        .iter()
        .filter(|u| u.type_id == hpea && u.owner == 0)
        .map(|u| u.handle)
        .collect();
    let hall = first(&sim, b"htow");
    let mut everything = peasants.clone();
    everything.push(hall);
    // A rectangle over the town hall and the peasants takes the peasants.
    assert_eq!(sim.selectable(&everything), peasants);
    // Only the town hall: it is taken.
    assert_eq!(sim.selectable(&[hall]), [hall]);
    // The same type: the clicked one first.
    let same = sim.same_type(peasants[2], &everything);
    assert_eq!(same.len(), peasants.len());
    assert_eq!(same[0], peasants[2]);
    let mut selection = ow3_viewer::selection::Selection::default();
    selection.set(&peasants);
    // The panel shows the group: one icon a peasant.
    let frame = sim.interface(&selection, None);
    let icon = sim.hud.buttons(sim.selected(peasants[0]).as_ref()).len();
    assert!(icon > 0);
    let grid = frame
        .quads
        .iter()
        .filter(|q| (q.rect[2] - q.rect[0] - 0.031_25).abs() < 1e-4 && !q.texture.is_empty())
        .count();
    assert_eq!(grid, peasants.len());
    // A group order moves them all.
    let h = view(&sim, hall);
    let line = sim
        .run_group(
            &peasants,
            &peasants,
            Command::Move,
            Some([h.x + 600.0, h.y]),
        )
        .unwrap();
    assert!(line.ends_with("taken"), "{line}");
    let before: Vec<(f32, f32)> = peasants
        .iter()
        .map(|&p| (view(&sim, p).x, view(&sim, p).y))
        .collect();
    for _ in 0..30 {
        sim.advance(100_000);
    }
    for (&p, b) in peasants.iter().zip(before) {
        let v = view(&sim, p);
        assert!(v.x > b.0 + 100.0, "#{p} from {b:?} to {}, {}", v.x, v.y);
    }
}
