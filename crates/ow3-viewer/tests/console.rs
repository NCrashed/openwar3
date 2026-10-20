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
    // Home again it may fight the peasants of the camp's neighbours, so
    // it ends inside its guard zone, not on its home point.
    assert!(away < 400.0, "{away}");
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
            walk.iter()
                .zip(&carry)
                .any(|(w, c)| w[3] < 0.5 && c[3] > 0.5),
            "load {load}: {walk:?} {carry:?}"
        );
        // Standing with it, it still shows.
        let stand = animation
            .pose_alphas(peasant, UnitPose::LoadStand(300, load))
            .unwrap()
            .0;
        assert!(
            walk.iter()
                .zip(&stand)
                .any(|(w, s)| w[3] < 0.5 && s[3] > 0.5),
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
    assert!(values.iter().any(|a| a[3] > 0.5), "{values:?}");
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
    // The panel shows the group: one icon a peasant, all of the active
    // subgroup (SPEC-0028).
    let frame = sim.interface(&selection, None);
    let icon = sim.hud.buttons(sim.selected(peasants[0]).as_ref()).len();
    assert!(icon > 0);
    let grid = frame
        .quads
        .iter()
        .filter(|q| {
            let (w, h) = (q.rect[2] - q.rect[0], q.rect[3] - q.rect[1]);
            (w - 0.028).abs() < 1e-4 && (h - 0.028).abs() < 1e-4 && !q.texture.is_empty()
        })
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
            false,
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

/// A group walks, one of it is sent to build a farm at once, and the rest
/// walks on to the end of its path (the formation lost track of who is in
/// it and the game stopped).
#[test]
fn one_of_a_group_leaves_to_build() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let hpea = i32::from_be_bytes(*b"hpea");
    let peasants: Vec<u32> = sim
        .units()
        .iter()
        .filter(|u| u.type_id == hpea && u.owner == 0)
        .map(|u| u.handle)
        .collect();
    let hall = view(&sim, first(&sim, b"htow"));
    sim.command_group(
        &peasants,
        None,
        Some([hall.x - 200.0, hall.y - 300.0, 0.0]),
        false,
        false,
    )
    .unwrap();
    // It leaves once the group has its formation.
    sim.advance(1_000_000);
    let east = east_of_hall(&sim);
    let hhou = i32::from_be_bytes(*b"hhou");
    let line = sim
        .run(peasants[0], Command::Build(hhou), Some([east.0, east.1]))
        .unwrap();
    assert!(line.ends_with("taken"), "{line}");
    for _ in 0..150 {
        sim.advance(100_000);
    }
    assert!(peasants[1..].iter().all(|&p| !view(&sim, p).walking));
}

/// The card follows the requirements and the current order (SPEC-0028):
/// with a town hall only, a farm and barracks can be built and some
/// buildings are dark; an idle peasant has stop as its order; the town
/// hall shows cancel only while it trains, and its progress grows.
#[test]
fn requirements_orders_and_training_progress() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let hall = first(&sim, b"htow");
    let s = sim.selected(peasant).unwrap();
    let stop = sim
        .hud
        .buttons(Some(&s))
        .into_iter()
        .find(|b| b.command == Command::Stop)
        .unwrap();
    assert!(stop.active);
    sim.run(peasant, Command::BuildMenu, None);
    let menu = sim.hud.buttons(Some(&s));
    let enabled = |t: &[u8; 4]| {
        let t = i32::from_be_bytes(*t);
        menu.iter()
            .find(|b| b.command == Command::Build(t))
            .map(|b| b.enabled)
    };
    assert_eq!(enabled(b"hhou"), Some(true));
    assert_eq!(enabled(b"hbar"), Some(true));
    assert!(menu.iter().any(|b| !b.enabled), "no dark building");
    sim.run(peasant, Command::Back, None);
    // The town hall: no cancel while idle.
    let h = sim.selected(hall).unwrap();
    assert!(
        !sim.hud
            .buttons(Some(&h))
            .iter()
            .any(|b| b.command == Command::CancelTrain)
    );
    let hpea = i32::from_be_bytes(*b"hpea");
    sim.run(hall, Command::Train(hpea), None).unwrap();
    sim.run(hall, Command::Train(hpea), None).unwrap();
    sim.advance(2_000_000);
    let h = sim.selected(hall).unwrap();
    assert_eq!(h.queue, [hpea, hpea]);
    let early = h.progress.unwrap();
    assert!(early > 0.0 && early < 0.5, "{early}");
    assert!(
        sim.hud
            .buttons(Some(&h))
            .iter()
            .any(|b| b.command == Command::CancelTrain)
    );
    sim.advance(3_000_000);
    let later = sim.selected(hall).unwrap().progress.unwrap();
    assert!(later > early, "{later} after {early}");
    // The information panel shows the bar's fill, as wide as the share.
    let mut selection = ow3_viewer::selection::Selection::default();
    selection.click(hall);
    let frame = sim.interface(&selection, None);
    let width = 0.475_625 - 0.370_625;
    assert!(frame.quads.iter().any(|q| {
        (q.rect[1] - 0.065_938).abs() < 1e-4
            && ((q.rect[2] - q.rect[0]) / width - later).abs() < 0.02
    }));
}

/// Shift queues (SPEC-0031): a peasant sent east and then, with Shift,
/// north goes to both points in turn.
#[test]
fn shift_queues_an_order() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let peasant = first(&sim, b"hpea");
    let hall = view(&sim, first(&sim, b"htow"));
    let east = [hall.x + 600.0, hall.y - 300.0];
    let north = [hall.x + 600.0, hall.y + 200.0];
    sim.run_group(&[peasant], &[peasant], Command::Move, Some(east), false)
        .unwrap();
    sim.run_group(&[peasant], &[peasant], Command::Move, Some(north), true)
        .unwrap();
    let mut passed_east = false;
    for _ in 0..200 {
        sim.advance(100_000);
        let p = view(&sim, peasant);
        passed_east |= (p.x - east[0]).abs() < 40.0 && (p.y - east[1]).abs() < 40.0;
    }
    let p = view(&sim, peasant);
    assert!(passed_east);
    assert!(
        (p.x - north[0]).abs() < 40.0 && (p.y - north[1]).abs() < 40.0,
        "{p:?}"
    );
}

/// The fog of war (SPEC-0032): after the first update the local player
/// sees round its town hall, and the creeps far away are not drawn.
#[test]
fn the_fog_hides_far_creeps() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(1_000_000);
    let vision = sim.vision().unwrap();
    let visible = vision.iter().filter(|&&c| c == 2).count();
    assert!(visible > 100 && visible < vision.len() / 10, "{visible}");
    let drawn: Vec<u32> = sim.draws().iter().map(|d| d.handle).collect();
    let units = sim.units();
    let creeps: Vec<_> = units
        .iter()
        .filter(|u| u.owner == ow3_sim::game::CREEPS && u.alive)
        .collect();
    assert!(!creeps.is_empty());
    assert!(creeps.iter().all(|c| !drawn.contains(&c.handle)));
    let hall = first(&sim, b"htow");
    assert!(drawn.contains(&hall));
}

/// The level line (SPEC-0028): a creep shows `INFOPANEL_LEVEL` with its
/// level under its name, from 0.08906 to 0.09756 in height and centred on
/// x 0.403435; a peasant shows none.
#[test]
fn a_creep_shows_its_level() {
    let Some(mut sim) = start() else {
        return;
    };
    sim.advance(100_000);
    let units = sim.units();
    let creep = units
        .iter()
        .find(|u| u.owner == ow3_sim::game::CREEPS && u.alive)
        .unwrap();
    let selected = sim.selected(creep.handle).unwrap();
    let level = selected.level.unwrap();
    assert!(level > 0);
    let mut selection = ow3_viewer::selection::Selection::default();
    selection.click(creep.handle);
    let frame = sim.interface(&selection, None);
    let line = frame
        .texts
        .iter()
        .find(|t| t.text.contains(&level.to_string()) && t.rect[3] < 0.1)
        .unwrap_or_else(|| panic!("{:?}", frame.texts));
    assert!((line.rect[3] - 0.097_56).abs() < 0.001, "{:?}", line.rect);
    let centre = (line.rect[0] + line.rect[2]) / 2.0;
    assert!((centre - 0.403_435).abs() < 0.002, "{:?}", line.rect);

    let peasant = first(&sim, b"hpea");
    assert_eq!(sim.selected(peasant).unwrap().level, None);
}
