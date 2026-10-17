//! Orders and the approach to an attack target (SPEC-0019) with the custom
//! data set of the game, on a world whose lower left corner is (-12288,
//! -12288). Skipped without game data.

use ow3_sim::data::UnitType;
use ow3_sim::game::{Game, UnitView};

/// The script: `UNITS` creates the units (the first one is `a`, the second
/// `t`), and `ORDERS` runs from a timer 0.5 s later, after
/// `SetRandomSeed(7)` (a reaction delay of 0.0015 s, SPEC-0019).
const SCRIPT: &str = r#"
type agent extends handle
type player extends agent
type widget extends agent
type unit extends widget
type timer extends agent
native Player takes integer number returns player
native CreateUnit takes player id, integer unitid, real x, real y, real face returns unit
native CreateTimer takes nothing returns timer
native TimerStart takes timer whichTimer, real timeout, boolean periodic, code handlerFunc returns nothing
native IssuePointOrder takes unit whichUnit, string order, real x, real y returns boolean
native IssueTargetOrder takes unit whichUnit, string order, widget targetWidget returns boolean
native IssueImmediateOrder takes unit whichUnit, string order returns boolean
native GetUnitCurrentOrder takes unit whichUnit returns integer
native SetRandomSeed takes integer seed returns nothing
globals
    unit a = null
    unit t = null
endglobals
EXTRA
function Orders takes nothing returns nothing
    call SetRandomSeed(7)
    ORDERS
endfunction
function main takes nothing returns nothing
    UNITS
    call TimerStart(CreateTimer(), 0.5, false, function Orders)
endfunction
"#;

/// The tick at which the 0.5 s timer gives the orders.
const ORDER_TICK: u64 = 101;

fn game(
    units: &str,
    orders: &str,
    change: impl FnOnce(&mut ow3_sim::data::ObjectData),
) -> Option<Game<'static>> {
    let game_data =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    if !game_data.join("War3Patch.mpq").exists() {
        return None;
    }
    // The tables of the game, read once for all the games of the tests.
    static DATA: std::sync::OnceLock<ow3_sim::data::ObjectData> = std::sync::OnceLock::new();
    let mut data = DATA
        .get_or_init(|| {
            let mut archives = ow3_sim::map::Archives::game(&game_data).unwrap();
            ow3_sim::data::ObjectData::load(&mut archives, false).unwrap()
        })
        .clone();
    change(&mut data);
    let (orders, extra) = orders.split_once("\n---\n").unwrap_or((orders, ""));
    let text = SCRIPT
        .replace("UNITS", units)
        .replace("ORDERS", orders)
        .replace("EXTRA", extra);
    let script = Box::leak(Box::new(ow3_jass::parse(&text).unwrap()));
    let refs: &'static [&'static _] = Box::leak(Box::new([&*script]));
    let program = Box::leak(Box::new(ow3_jass::check::check(refs).unwrap()));
    let mut game = Game::load(program, refs).unwrap();
    {
        let mut state = game.state.borrow_mut();
        state.data = data;
        state.world_bounds = [-12288.0f32, -12288.0, 12288.0, 12288.0].map(f32::to_bits);
    }
    game.run("main");
    Some(game)
}

/// The units at every tick up to `until`.
fn run(game: &mut Game, until: u64) -> Vec<(u64, Vec<UnitView>)> {
    let mut out = Vec::new();
    while game.ticks() < until {
        game.step();
        out.push((game.ticks(), game.units()));
    }
    out
}

fn code(c: &[u8; 4]) -> i32 {
    i32::from_be_bytes(*c)
}

/// A target that cannot move, with plenty of life and `collision`.
fn target_type(data: &mut ow3_sim::data::ObjectData, collision: f32) {
    let t: &mut UnitType = data.units.get_mut(&code(b"hpea")).unwrap();
    t.speed = 0;
    t.collision = collision.to_bits();
    t.hp = 100_000f32.to_bits();
}

/// The approach table of SPEC-0019: the attacker stops within one update of
/// motion below rangeN1 + both collision sizes, and hits dmgpt1 later.
#[test]
fn approach() {
    for (attacker, collision, stood, limit) in [
        (b"hfoo", 16.0f32, 130.47f32, 137.0f32),
        (b"hfoo", 64.0, 179.02, 185.0),
        (b"hkni", 32.0, 159.24, 164.0),
        (b"hrif", 16.0, 446.11, 448.0),
    ] {
        let units = format!(
            "set a = CreateUnit(Player(0), '{}', -3000, -2000, 0)\n    set t = CreateUnit(Player(15), 'hpea', -2400, -2000, 180)",
            String::from_utf8_lossy(attacker)
        );
        let Some(mut game) = game(&units, "call IssueTargetOrder(a, \"attack\", t)", |d| {
            target_type(d, collision)
        }) else {
            return;
        };
        let path = run(&mut game, 1200);
        // Where the attacker came to rest.
        let rest = path
            .windows(2)
            .find(|w| w[0].0 > ORDER_TICK + 7 && w[0].1[0].x == w[1].1[0].x)
            .unwrap()[0]
            .clone();
        let distance: f32 = -2400.0 - rest.1[0].x;
        let name = String::from_utf8_lossy(attacker);
        assert!(distance <= limit, "{name}: {distance}");
        assert!(
            (distance - stood).abs() < 0.5,
            "{name}: {distance} for {stood}"
        );
        // The first damage, the weapon's damage point after the update at
        // which it stopped.
        let hit = path
            .iter()
            .find(|(_, u)| u[1].life < u[1].max_life)
            .unwrap()
            .0;
        let stop = rest.0;
        let point = match &attacker[..] {
            b"hfoo" => 100,
            b"hkni" => 132,
            _ => 34,
        };
        assert_eq!(hit - stop, point, "{name}");
    }
}

/// The turning table of SPEC-0019: a knight next to a target at `a`
/// degrees from its heading, first damage in ticks after the order.
#[test]
fn knight_turns_before_it_strikes() {
    for (a, ticks) in [
        (0.0f32, 138),
        (30.0, 144),
        (45.0, 150),
        (60.0, 150),
        (90.0, 156),
        (120.0, 162),
        (135.0, 168),
        (150.0, 168),
        (180.0, 174),
    ] {
        let (s, c) = a.to_radians().sin_cos();
        let units = format!(
            "set a = CreateUnit(Player(0), 'hkni', 0, 0, 0)\n    set t = CreateUnit(Player(15), 'hpea', {:.4}, {:.4}, 0)",
            100.0 * c,
            100.0 * s
        );
        let Some(mut game) = game(&units, "call IssueTargetOrder(a, \"attack\", t)", |d| {
            target_type(d, 32.0)
        }) else {
            return;
        };
        let path = run(&mut game, 400);
        let hit = path
            .iter()
            .find(|(_, u)| u[1].life < u[1].max_life)
            .unwrap()
            .0;
        assert_eq!(hit - ORDER_TICK, ticks, "{a} degrees");
    }
}

/// Verification 2 of SPEC-0019: `stop` ends the motion at once, also
/// between two updates.
#[test]
fn stop_while_walking() {
    let orders = "call IssuePointOrder(a, \"move\", -1000, -2000)\n    call TimerStart(CreateTimer(), 0.5, false, function Stop)\n---\nfunction Stop takes nothing returns nothing\n    call IssueImmediateOrder(a, \"stop\")\nendfunction";
    let Some(mut game) = game(
        "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)",
        orders,
        |_| {},
    ) else {
        return;
    };
    let path = run(&mut game, 400);
    let x = |tick: u64| path.iter().find(|p| p.0 == tick).unwrap().1[0].x;
    // The last tick with motion.
    let stop = path
        .windows(2)
        .find(|w| w[0].0 > ORDER_TICK + 7 && w[0].1[0].x == w[1].1[0].x)
        .unwrap()[0]
        .0;
    assert_ne!((stop - ORDER_TICK) % 6, 0, "stopped at an update");
    // Every tick from the one after update 1 moved by 1.35.
    let moved = x(stop) - -3000.0;
    let expected = (stop - ORDER_TICK - 6) as f32 * 1.35;
    assert!((moved - expected).abs() < 0.05, "{moved} for {expected}");
    assert_eq!(x(stop + 100), x(stop));
}

/// Patrol: back and forth between the target and the start, turning on
/// the spot at each end (24 ticks for a footman turning round).
#[test]
fn patrol() {
    let Some(mut game) = game(
        "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)",
        "call IssuePointOrder(a, \"patrol\", -2400, -2000)",
        |_| {},
    ) else {
        return;
    };
    let path = run(&mut game, 1500);
    let xs: Vec<f32> = path.iter().map(|p| p.1[0].x).collect();
    // The far end, the turn there and the way back.
    let far = xs.iter().cloned().fold(f32::MIN, f32::max);
    assert!(far > -2420.0 && far < -2400.0, "{far}");
    let at_far = xs.iter().position(|&x| x == far).unwrap();
    let still = xs[at_far..].iter().take_while(|&&x| x == far).count();
    assert_eq!(still, 24 + 1);
    assert!(xs[at_far + 60] < far - 10.0);
}

/// A new move to the same point while walking costs one tick.
#[test]
fn same_move_again() {
    let units = "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)";
    let once = "call IssuePointOrder(a, \"move\", -1000, -2000)";
    let twice = "call IssuePointOrder(a, \"move\", -1000, -2000)\n    call TimerStart(CreateTimer(), 0.3, false, function Again)\n---\nfunction Again takes nothing returns nothing\n    call IssuePointOrder(a, \"move\", -1000, -2000)\nendfunction";
    let (Some(mut one), Some(mut two)) = (game(units, once, |_| {}), game(units, twice, |_| {}))
    else {
        return;
    };
    let a = run(&mut one, 400);
    let b = run(&mut two, 400);
    let last = |p: &[(u64, Vec<UnitView>)]| p.last().unwrap().1[0].x;
    let lost = (last(&a) - last(&b)) / 1.35;
    assert!((lost - 1.0).abs() < 0.05, "{lost} ticks");
}

/// Acquisition of SPEC-0019: an idle footman takes an enemy footman at 562
/// and not at 563 (`acquire` 500 and both collisions 31).
#[test]
fn idle_footmen_acquire() {
    for (gap, takes) in [(562, true), (563, false)] {
        let units = format!(
            "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)\n    set t = CreateUnit(Player(1), 'hfoo', {}, -2000, 180)",
            -3000 + gap
        );
        let Some(mut game) = game(&units, "", |_| {}) else {
            return;
        };
        let path = run(&mut game, 400);
        let last = &path.last().unwrap().1;
        let moved = last[0].x != -3000.0 || last[1].x != (-3000 + gap) as f32;
        assert_eq!(moved, takes, "{gap}");
    }
}

/// The reaction delay of SPEC-0019: with seed 777 (0.0329 s, past the
/// first update) a knight next to its target hits one update later than
/// with seed 7.
#[test]
fn reaction_delay_waits_an_update() {
    let units = "set a = CreateUnit(Player(0), 'hkni', 0, 0, 0)\n    set t = CreateUnit(Player(15), 'hpea', 100, 0, 0)";
    let Some(mut game) = game(
        units,
        "call SetRandomSeed(777)\n    call IssueTargetOrder(a, \"attack\", t)",
        |d| target_type(d, 32.0),
    ) else {
        return;
    };
    let path = run(&mut game, 400);
    let hit = path
        .iter()
        .find(|(_, u)| u[1].life < u[1].max_life)
        .unwrap()
        .0;
    assert_eq!(hit - ORDER_TICK, 138 + 6);
}

/// SPEC-0019: a walking unit given an attack order stops at once and
/// stands for its reaction delay (seed 777: 0.0329 s, past one update).
#[test]
fn attack_order_stops_a_walking_unit() {
    let orders = "call IssuePointOrder(a, \"move\", -1000, -2000)\n    call TimerStart(CreateTimer(), 0.3, false, function Attack)\n---\nfunction Attack takes nothing returns nothing\n    call SetRandomSeed(777)\n    call IssueTargetOrder(a, \"attack\", t)\nendfunction";
    let Some(mut game) = game(
        "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)\n    set t = CreateUnit(Player(15), 'hpea', -2600, -1000, 0)",
        orders,
        |d| target_type(d, 16.0),
    ) else {
        return;
    };
    let path = run(&mut game, 400);
    let at = |tick: u64| {
        let u = &path.iter().find(|p| p.0 == tick).unwrap().1[0];
        (u.x, u.y)
    };
    // The 0.3 s timer fires 60 ticks after the move order.
    let order = ORDER_TICK + 60;
    assert!(at(order).0 > at(order - 1).0, "walking before");
    for tick in order + 1..=order + 12 {
        assert_eq!(at(tick), at(order), "tick {tick}");
    }
}
