//! Orders and the approach to an attack target (SPEC-0019) with the custom
//! data set of the game, on a world whose lower left corner is (-12288,
//! -12288). Skipped without game data.

use ow3_sim::data::UnitType;
use ow3_sim::game::{Game, UnitView};

/// The script: `UNITS` creates the units (the first one is `a`, the second
/// `t`), and `ORDERS` runs from a timer 0.51 s later, in the update tick 103
/// (SPEC-0018, "Phase"), so that the orders are taken 6 ticks later as in
/// the tests of SPEC-0019, after
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
native PauseUnit takes unit whichUnit, boolean flag returns nothing
type event extends agent
type trigger extends agent
type triggeraction extends handle
type unitevent extends handle
native ConvertUnitEvent takes integer i returns unitevent
native CreateTrigger takes nothing returns trigger
native TriggerRegisterUnitEvent takes trigger whichTrigger, unit whichUnit, unitevent whichEvent returns event
native TriggerAddAction takes trigger whichTrigger, code actionFunc returns triggeraction
native GetTriggerUnit takes nothing returns unit
native KillUnit takes unit whichUnit returns nothing
type unitstate extends handle
native ConvertUnitState takes integer i returns unitstate
native SetUnitState takes unit whichUnit, unitstate whichUnitState, real newVal returns nothing
native SetHeroStr takes unit whichHero, integer newStr, boolean permanent returns nothing
native SetUnitFacing takes unit whichUnit, real facingAngle returns nothing
native DisplayTextToPlayer takes player toPlayer, real x, real y, string message returns nothing
native I2S takes integer i returns string
native R2S takes real r returns string
type fgamestate extends handle
native ConvertFGameState takes integer i returns fgamestate
native SetFloatGameState takes fgamestate whichFloatGameState, real value returns nothing
native GetUnitState takes unit whichUnit, unitstate whichUnitState returns real
native SetHeroInt takes unit whichHero, integer newInt, boolean permanent returns nothing
native GetHeroStr takes unit whichHero, boolean includeBonuses returns integer
native OrderId takes string orderIdString returns integer
native OrderId2String takes integer orderId returns string
native IssuePointOrderById takes unit whichUnit, integer order, real x, real y returns boolean
native IssueTargetOrderById takes unit whichUnit, integer order, widget targetWidget returns boolean
native IssueImmediateOrderById takes unit whichUnit, integer order returns boolean
type location extends agent
native Location takes real x, real y returns location
native IssuePointOrderLoc takes unit whichUnit, string order, location whichLocation returns boolean
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
    call TimerStart(CreateTimer(), 0.51, false, function Orders)
endfunction
"#;

/// The tick at which the 0.51 s timer gives the orders.
const ORDER_TICK: u64 = 103;

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
    type Tables = (ow3_sim::data::ObjectData, ow3_sim::combat::Combat);
    static DATA: std::sync::OnceLock<Tables> = std::sync::OnceLock::new();
    let (mut data, combat) = DATA
        .get_or_init(|| {
            let mut archives = ow3_sim::map::Archives::game(&game_data).unwrap();
            let data = ow3_sim::data::ObjectData::load(&mut archives, false).unwrap();
            (data, ow3_sim::combat::Combat::load(&mut archives, false))
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
        state.combat = combat;
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
/// degrees from its heading, first damage in ticks after the order given
/// in an update tick. The knight turns at the first update at or after
/// order + 0.31, but not before order + 2, and swings one update later
/// ("When it starts"): every turning knight hits at the table's tick, the
/// one facing its target 6 ticks later than printed (the table was
/// measured with orders at no exact tick; the exact series of the spec
/// gives 12 ticks to the swing).
#[test]
fn knight_turns_before_it_strikes() {
    for (a, ticks) in [
        (0.0f32, 138 + 6),
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
/// `IssuePointOrderLoc` is the point order to the location's point.
#[test]
fn point_order_to_a_location() {
    let units = "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)";
    let (Some(mut by_point), Some(mut by_location)) = (
        game(
            units,
            "call IssuePointOrder(a, \"move\", -1000, -1500)",
            |_| {},
        ),
        game(
            units,
            "call IssuePointOrderLoc(a, \"move\", Location(-1000, -1500))",
            |_| {},
        ),
    ) else {
        return;
    };
    let a = run(&mut by_point, 400);
    let b = run(&mut by_location, 400);
    assert_eq!(a, b);
    assert!(a.last().unwrap().1[0].x > -2900.0);
}

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

/// Verification 8 of SPEC-0019: a paused enemy footman at the centre and
/// 20 footmen on a circle of radius 300 round it, created after it in
/// tick 201 of the game, after `SetRandomSeed`. Counted from the tick of
/// the creation, each circle footman acquires the centre in tick
/// `floor(100 * x)`; with seed 7 the first ones turn at the first update
/// after `100 * x` plus their reaction delays: 10, 10, 34 and 64.
#[test]
fn acquisition_ticks() {
    for (seed, expected) in [
        (
            7,
            [
                3, 8, 18, 22, 26, 31, 36, 44, 45, 46, 48, 68, 70, 72, 81, 84, 88, 88, 90, 94,
            ],
        ),
        (
            8,
            [
                1, 5, 13, 16, 19, 32, 33, 33, 40, 46, 67, 69, 70, 73, 79, 81, 85, 89, 91, 96,
            ],
        ),
    ] {
        let mut create = format!(
            "    call SetRandomSeed({seed})\n    set t = CreateUnit(Player(1), 'hfoo', 0, 0, 0)\n    call PauseUnit(t, true)\n"
        );
        for i in 0..20 {
            let angle = (i as f64 * 18.0).to_radians();
            create += &format!(
                "    call CreateUnit(Player(0), 'hfoo', {:.3}, {:.3}, 0)\n",
                300.0 * angle.cos(),
                300.0 * angle.sin()
            );
        }
        // From the orders at tick 103, a timer for tick 201.
        let orders = format!(
            "call TimerStart(CreateTimer(), 0.49, false, function Create)\n---\nfunction Create takes nothing returns nothing\n{create}endfunction"
        );
        let Some(mut game) = game("", &orders, |_| {}) else {
            return;
        };
        let path = run(&mut game, 400);
        let created = path.iter().find(|p| !p.1.is_empty()).unwrap().0;
        assert_eq!(created, 201);
        let first = |unit: usize, seen: &dyn Fn(&UnitView, &UnitView) -> bool| {
            path.windows(2)
                .filter(|w| !w[0].1.is_empty())
                .find(|w| seen(&w[0].1[unit], &w[1].1[unit]))
                .map(|w| w[1].0 - created)
        };
        let mut acquired: Vec<(u64, usize)> = (1..=20)
            .map(|i| (first(i, &|_, b| b.target.is_some()).unwrap(), i))
            .collect();
        acquired.sort();
        let ticks: Vec<u64> = acquired.iter().map(|a| a.0).collect();
        assert_eq!(ticks, expected, "seed {seed}");
        if seed == 7 {
            let turn = |tick: u64| {
                let i = acquired.iter().find(|a| a.0 == tick).unwrap().1;
                first(i, &|a, b| a.facing != b.facing)
            };
            let turns = [turn(3), turn(8), turn(22), turn(18)];
            assert_eq!(turns, [Some(10), Some(10), Some(34), Some(64)]);
        }
    }
}

/// SPEC-0019: `EVENT_UNIT_ACQUIRED_TARGET` fires at the check that finds
/// a target, here killing the unit in the tick of its acquisition.
#[test]
fn acquired_target_event() {
    let units = "call SetRandomSeed(7)\n    set a = CreateUnit(Player(0), 'hfoo', 0, 0, 0)\n    set t = CreateUnit(Player(1), 'hfoo', 300, 0, 0)\n    call PauseUnit(t, true)";
    let watch = "\n---\nfunction Die takes nothing returns nothing\n    call KillUnit(GetTriggerUnit())\nendfunction\nfunction Watch takes unit u returns nothing\n    local trigger trig = CreateTrigger()\n    call TriggerRegisterUnitEvent(trig, u, ConvertUnitEvent(60))\n    call TriggerAddAction(trig, function Die)\nendfunction";
    let (Some(mut plain), Some(mut watched)) = (
        game(units, "", |_| {}),
        game(&format!("{units}\n    call Watch(a)"), watch, |_| {}),
    ) else {
        return;
    };
    let path = run(&mut plain, 120);
    let acquired = path.iter().find(|p| p.1[0].target.is_some()).unwrap().0;
    run(&mut watched, 120);
    assert_eq!(watched.units()[0].died_at, Some(acquired));
}

/// The choice of target of SPEC-0019: a unit before a nearer structure,
/// and between enemies at the same distance the one with the smaller y,
/// then the smaller x.
#[test]
fn acquisition_choice() {
    let enemy = |kind: &str, x: i32, y: i32| {
        format!(
            "\n    set t = CreateUnit(Player(1), '{kind}', {x}, {y}, 0)\n    call PauseUnit(t, true)"
        )
    };
    // Eight enemies exactly 300 away.
    let around: String = [(300, 0), (240, 180), (0, 300), (-180, 240), (-300, 0)]
        .iter()
        .chain(&[(-240, -180), (0, -300), (180, -240)])
        .map(|&(x, y)| enemy("hfoo", x, y))
        .collect();
    let level = enemy("hfoo", 300, -300) + &enemy("hfoo", -300, -300);
    for (enemies, at) in [
        (
            enemy("hfoo", 300, 0) + &enemy("hhou", -128, 0),
            (300.0, 0.0),
        ),
        (
            enemy("hhou", -128, 0) + &enemy("hfoo", 300, 0),
            (300.0, 0.0),
        ),
        (around, (0.0, -300.0)),
        (level, (-300.0, -300.0)),
    ] {
        let units = format!("set a = CreateUnit(Player(0), 'hfoo', 0, 0, 0){enemies}");
        let Some(mut game) = game(&units, "", |_| {}) else {
            return;
        };
        let path = run(&mut game, 120);
        let last = &path.last().unwrap().1;
        let target = last.iter().find(|u| Some(u.handle) == last[0].target);
        assert_eq!(target.map(|u| (u.x, u.y)), Some(at), "{enemies}");
    }
}

/// The reaction delay of SPEC-0019: with seed 777 (0.0329 s, past the
/// seven ticks after the order) a knight next to its target hits one update
/// later than with seed 7.
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
    // The swing at 121 for the order at 103 (SPEC-0019, "When it starts").
    assert_eq!(hit - ORDER_TICK, 121 - 103 + 132);
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

/// The chase of SPEC-0019: a footman after a target walking away at 150
/// swings from farther than the static limit of 152 (178 to 218 in the
/// game), since it swings once its goal point is in reach, and goes on
/// hitting as it catches up again.
#[test]
fn chase_a_target_walking_away() {
    let orders = "call IssuePointOrder(t, \"move\", 6000, -2000)\n    call IssueTargetOrder(a, \"attack\", t)";
    let Some(mut game) = game(
        "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)\n    set t = CreateUnit(Player(15), 'hpea', -2700, -2000, 0)",
        orders,
        |d| {
            target_type(d, 31.0);
            d.units.get_mut(&code(b"hpea")).unwrap().speed = 150f32.to_bits();
        },
    ) else {
        return;
    };
    let path = run(&mut game, 3000);
    // Ticks of damage, and the distance when each swing began (dmgpt1 0.5 s
    // before the damage).
    let hits: Vec<u64> = path
        .windows(2)
        .filter(|w| w[1].1[1].life < w[0].1[1].life)
        .map(|w| w[1].0)
        .collect();
    assert!(hits.len() >= 3, "{hits:?}");
    let distance = |tick: u64| {
        let u = &path.iter().find(|p| p.0 == tick).unwrap().1;
        u[1].x - u[0].x
    };
    let first = distance(hits[0] - 100);
    assert!((160.0..235.0).contains(&first), "{first}");
}

/// An attacker walks round a wall to its target by the paths of SPEC-0020
/// (provisional for chases, SPEC-0019): a knight west of a wall attacks a
/// target east of it.
#[test]
fn attacker_walks_round_a_wall() {
    let units = "set a = CreateUnit(Player(0), 'hkni', -2300, -2000, 0)\n    set t = CreateUnit(Player(15), 'hpea', -1700, -2000, 180)";
    let Some(mut game) = game(units, "call IssueTargetOrder(a, \"attack\", t)", |d| {
        target_type(d, 16.0)
    }) else {
        return;
    };
    // A 768 x 768 map whose corner is -12288, with a wall over
    // -2000 <= x < -1968 and -2400 <= y < -1600.
    let mut cells = vec![true; 768 * 768];
    for y in 309..334 {
        for x in 321..=322 {
            cells[y * 768 + x] = false;
        }
    }
    game.set_pathing(ow3_sim::path::Grid::from_cells(768, 768, cells));
    let path = run(&mut game, 2000);
    for (tick, u) in &path {
        let (x, y) = (u[0].x, u[0].y);
        let inside = (-2000.0..-1968.0).contains(&x) && (-2400.0..-1600.0).contains(&y);
        assert!(!inside, "tick {tick}: ({x}, {y}) in the wall");
    }
    assert!(
        path.iter().any(|(_, u)| u[1].life < u[1].max_life),
        "no hit"
    );
}

/// SPEC-0013 with the phase of SPEC-0018: the facing set from a timer
/// changes at the updates of the shared cycle, the first one 6 ticks
/// after a call in an update tick (the 0.5 s timer) and 3 ticks after a
/// call 0.015 s later.
#[test]
fn set_facing_follows_the_updates() {
    for (orders, first) in [
        ("call SetUnitFacing(a, 90)", ORDER_TICK + 6),
        (
            "call TimerStart(CreateTimer(), 0.015, false, function Turn)\n---\nfunction Turn takes nothing returns nothing\n    call SetUnitFacing(a, 90)\nendfunction",
            ORDER_TICK + 6,
        ),
    ] {
        let Some(mut game) = game(
            "set a = CreateUnit(Player(0), 'hfoo', 0, 0, 0)",
            orders,
            |_| {},
        ) else {
            return;
        };
        let path = run(&mut game, 300);
        let changes: Vec<u64> = path
            .windows(2)
            .filter(|w| w[0].1[0].facing != w[1].1[0].facing)
            .map(|w| w[1].0)
            .collect();
        assert_eq!(changes[0], first, "{orders}");
        assert!(changes.windows(2).all(|w| w[1] - w[0] == 6));
    }
}

/// SPEC-0013: after `SetHeroStr(30)` a paladin's life regenerates at
/// 0.25 + 30 * 0.05 per second from 0.0001 s later, so its first step at
/// a life near 100 is 1123 units of the last place instead of 1147.
#[test]
fn strength_restarts_regeneration() {
    // The life is set after the call, which moves it by 25 a point.
    let orders = "call SetHeroStr(a, 30, true)\n    call SetUnitState(a, ConvertUnitState(0), 100)";
    let Some(mut game) = game(
        "set a = CreateUnit(Player(0), 'Hpal', 0, 0, 0)",
        orders,
        |_| {},
    ) else {
        return;
    };
    let path = run(&mut game, ORDER_TICK + 3);
    let life = |tick: u64| {
        path.iter().find(|p| p.0 == tick).unwrap().1[0]
            .life
            .to_bits()
    };
    let steps = [
        life(ORDER_TICK + 1) - life(ORDER_TICK),
        life(ORDER_TICK + 2) - life(ORDER_TICK + 1),
    ];
    assert_eq!(life(ORDER_TICK), 100f32.to_bits());
    assert!(
        steps[0].abs_diff(1123) <= 1 && steps[1].abs_diff(1147) <= 1,
        "{steps:?}"
    );
}

/// SPEC-0013: `SetUnitFacing` on a unit walking a move order changes
/// nothing, its path is the same tick by tick.
#[test]
fn set_facing_does_not_touch_a_walk() {
    let units = "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)";
    let walk = "call IssuePointOrder(a, \"move\", -1000, -2000)";
    let turned = format!(
        "{walk}\n    call TimerStart(CreateTimer(), 0.5, false, function Turn)\n---\nfunction Turn takes nothing returns nothing\n    call SetUnitFacing(a, 90)\nendfunction"
    );
    let (Some(mut plain), Some(mut other)) =
        (game(units, walk, |_| {}), game(units, &turned, |_| {}))
    else {
        return;
    };
    assert_eq!(run(&mut plain, 400), run(&mut other, 400));
}

/// The order ids of SPEC-0019: `OrderId`, `OrderId2String` and the
/// `...ById` natives, each followed by the current order.
#[test]
fn orders_by_id() {
    let orders = r#"call Show(OrderId("MOVE"))
    call Show(OrderId("patrol"))
    call Show(OrderId("nonsense"))
    if OrderId2String(851983) == "attack" and OrderId2String(12345) == null and OrderId2String(0) == null then
        call Print("names")
    endif
    call Issued(IssuePointOrderById(a, 851986, -1000, -2000))
    call Issued(IssuePointOrderById(a, 12345, -1000, -2000))
    call Issued(IssuePointOrderById(a, 0, -1000, -2000))
    call Issued(IssueImmediateOrderById(a, 851972))
    call Issued(IssueImmediateOrderById(a, 851986))
    call Issued(IssueTargetOrderById(a, 851983, t))
    call Issued(IssueTargetOrderById(a, 851986, t))
    call Issued(IssueImmediateOrderById(a, 851993))
    call Issued(IssuePointOrderById(a, 851983, -1000, -2000))
---
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Show takes integer i returns nothing
    call Print(I2S(i))
endfunction
function Issued takes boolean b returns nothing
    if b then
        call Print("true " + I2S(GetUnitCurrentOrder(a)))
    else
        call Print("false " + I2S(GetUnitCurrentOrder(a)))
    endif
endfunction"#;
    let Some(mut game) = game(
        "set a = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)\n    set t = CreateUnit(Player(1), 'hfoo', -2000, -1000, 0)\n    call PauseUnit(t, true)",
        orders,
        |_| {},
    ) else {
        return;
    };
    run(&mut game, ORDER_TICK + 1);
    assert_eq!(
        game.output(),
        [
            "851986",
            "851990",
            "0",
            "names",
            "true 851986",
            "false 851986",
            "false 851986",
            "true 0",
            "true 0",
            "true 851983",
            "true 851986",
            "true 0",
            "true 851983",
        ]
    );
}

/// Verification 5 of SPEC-0013, "Hero attributes": fresh paladins at the
/// life given, then `SetHeroStr`; and the mana example of intelligence.
#[test]
fn hero_attributes() {
    let orders = r#"call Str(300, 10)
    call Str(650, 10)
    call Str(650, 0)
    call Str(650, -5)
    call Str(100, -5)
    call Str(10, 21)
    call Str(0.5, 22)
    call Str(650, 23)
    call Str(325, 30)
    set a = CreateUnit(Player(0), 'Hpal', 0, 0, 0)
    call SetUnitState(a, ConvertUnitState(2), 100)
    call SetHeroInt(a, 2, true)
    call Print(R2S(GetUnitState(a, ConvertUnitState(2))) + " of " + R2S(GetUnitState(a, ConvertUnitState(3))))
    call SetHeroInt(a, 40, false)
    call Print(R2S(GetUnitState(a, ConvertUnitState(2))) + " of " + R2S(GetUnitState(a, ConvertUnitState(3))))
---
function Print takes string s returns nothing
    call DisplayTextToPlayer(Player(0), 0, 0, s)
endfunction
function Str takes real life, integer s returns nothing
    set a = CreateUnit(Player(0), 'Hpal', 0, 0, 0)
    call SetUnitState(a, ConvertUnitState(0), life)
    call SetHeroStr(a, s, true)
    call Print(R2S(GetUnitState(a, ConvertUnitState(0))) + " of " + R2S(GetUnitState(a, ConvertUnitState(1))) + " str " + I2S(GetHeroStr(a, true)))
endfunction"#;
    let Some(mut game) = game("", orders, |_| {}) else {
        return;
    };
    run(&mut game, ORDER_TICK + 1);
    assert_eq!(
        game.output(),
        [
            "1.000 of 350.000 str 10",
            "350.000 of 350.000 str 10",
            "100.000 of 100.000 str 0",
            "0.000 of -25.000 str -5",
            "0.000 of -25.000 str -5",
            "1.000 of 625.000 str 21",
            "0.500 of 650.000 str 22",
            "675.000 of 675.000 str 23",
            "525.000 of 850.000 str 30",
            "0.000 of 30.000",
            "570.000 of 600.000",
        ]
    );
}

/// SPEC-0013, "Night": an archer (`regenType` night) gains life from the
/// first tick whose hour is above 18.01, and still in the first tick
/// after the hour passes 6.01, not in the next.
#[test]
fn night_regeneration() {
    for (start, begins) in [(17.95f32, true), (5.95, false)] {
        let orders = format!(
            "call SetFloatGameState(ConvertFGameState(2), {start})\n    call SetUnitState(a, ConvertUnitState(0), 50)"
        );
        let Some(mut game) = game(
            "set a = CreateUnit(Player(0), 'earc', 0, 0, 0)",
            &orders,
            |_| {},
        ) else {
            return;
        };
        let path = run(&mut game, ORDER_TICK + 400);
        let life = |tick: u64| path.iter().find(|p| p.0 == tick).unwrap().1[0].life;
        let hour = |tick: u64| start + 0.05 * 0.005 * (tick - ORDER_TICK) as f32;
        let gains: Vec<u64> = (ORDER_TICK + 1..ORDER_TICK + 400)
            .filter(|&t| life(t) > life(t - 1))
            .collect();
        if begins {
            let first = gains[0];
            assert!(
                hour(first) > 18.01 && hour(first - 1) <= 18.01,
                "{}",
                hour(first)
            );
        } else {
            let last = *gains.last().unwrap();
            assert!(
                hour(last) > 6.01 && hour(last - 1) <= 6.01,
                "{}",
                hour(last)
            );
        }
    }
}

/// Verification 9 of SPEC-0019, attack-move: ten footmen ordered to
/// attack towards points 5000 east, paused enemy footmen beside their
/// ways. They acquire at the first check (order + 102 + 100 j) with an
/// enemy in range, stand, and turn or walk again at the first update
/// after the reaction delay, the delays drawn in the order of the
/// acquisitions.
#[test]
fn attack_move() {
    // The orders come in a tick 0 modulo 6, as in the game's test (whose
    // updates came 805 and 907 ticks after them), from a timer of 0.025 s.
    let mut go = String::new();
    for k in 0..10 {
        let y = -9000 + 1500 * k;
        go += &format!(
            "    set a = CreateUnit(Player(0), 'hfoo', -3000, {y}, 0)\n    call IssuePointOrder(a, \"attack\", 2000, {y})\n    set t = CreateUnit(Player(1), 'hfoo', {}, {}, 0)\n    call PauseUnit(t, true)\n",
            -1500 + 100 * k,
            y + 250
        );
    }
    let orders = format!(
        "call TimerStart(CreateTimer(), 0.025, false, function Go)\n---\nfunction Go takes nothing returns nothing\n{go}endfunction"
    );
    let Some(mut game) = game("", &orders, |_| {}) else {
        return;
    };
    let path = run(&mut game, ORDER_TICK + 1700);
    let order = path.iter().find(|p| !p.1.is_empty()).unwrap().0;
    assert_eq!(order % 6, 0);
    let acquired = [802, 902, 902, 1002, 1102, 1202, 1202, 1302, 1402, 1502];
    let again = [805, 907, 949, 1015, 1129, 1219, 1207, 1309, 1423];
    for k in 0..10 {
        let unit = 2 * k;
        let first = path
            .iter()
            .find(|p| p.1.get(unit).is_some_and(|u| u.target.is_some()))
            .map(|p| p.0 - order);
        assert_eq!(first, Some(acquired[k]), "acquisition of {k}");
        if k < 9 {
            let moved = path
                .windows(2)
                .filter(|w| w[0].0 > order + acquired[k])
                .find(|w| {
                    let (a, b) = (&w[0].1[unit], &w[1].1[unit]);
                    (a.x, a.y, a.facing) != (b.x, b.y, b.facing)
                })
                .map(|w| w[1].0 - order);
            assert_eq!(moved, Some(again[k]), "again of {k}");
        }
    }
}

/// Verification 10 of SPEC-0019, retaliation: a glaive thrower attacks an
/// idle footman; at the first damage the footman and the one 300 away
/// acquire the thrower, those 1200 and 1500 away do not. They wait their
/// reaction delays from the tick of the damage, the damaged one drawing
/// first.
#[test]
fn retaliation() {
    let units = "set a = CreateUnit(Player(1), 'ebal', 0, 1000, 270)\n    set t = CreateUnit(Player(0), 'hfoo', 0, 0, 270)\n    call CreateUnit(Player(0), 'hfoo', 300, 0, 270)\n    call CreateUnit(Player(0), 'hfoo', 1200, 0, 270)\n    call CreateUnit(Player(0), 'hfoo', -1500, 0, 270)";
    // The thrower comes 0.5 s before its order, and acquires the footman
    // at its own first check (SPEC-0019, verification 10).
    let orders = format!(
        "{units}\n    call TimerStart(CreateTimer(), 0.5, false, function Attack)\n---\nfunction Attack takes nothing returns nothing\n    call IssueTargetOrder(a, \"attack\", t)\nendfunction"
    );
    let Some(mut game) = game("", &orders, |_| {}) else {
        return;
    };
    let path = run(&mut game, ORDER_TICK + 600);
    let damage = path
        .iter()
        .find(|p| p.1.get(1).is_some_and(|u| u.life < u.max_life))
        .unwrap()
        .0;
    let at = |tick: u64| &path.iter().find(|p| p.0 == tick).unwrap().1;
    let targets: Vec<Option<u32>> = at(damage)[1..].iter().map(|u| u.target).collect();
    let thrower = Some(at(damage)[0].handle);
    assert_eq!(targets, [thrower, thrower, None, None]);
    let turn = |unit: usize| {
        path.windows(2)
            .filter(|w| w[0].0 >= damage)
            .find(|w| w[0].1[unit].facing != w[1].1[unit].facing)
            .map(|w| w[1].0 - damage)
    };
    // The thrower drew twice (its acquisition and its order), so the victim
    // draws the third delay (42.84 ticks) and the helper the fourth
    // (10.51), each turning at the first update at or after the damage
    // plus its delay.
    let update_after = |delay: f64| {
        let mut t = damage + delay.ceil() as u64;
        while t % 6 != 1 {
            t += 1;
        }
        t - damage
    };
    assert_eq!(
        (turn(1), turn(2)),
        (Some(update_after(42.838)), Some(update_after(10.514)))
    );
}

/// SPEC-0019, call for help with `CallForHelp` 600: idle footmen 750 away
/// answer, one 800 away does not, nor do a peasant next to the victim and
/// a footman walking past.
#[test]
fn call_for_help_reach() {
    let orders = "set a = CreateUnit(Player(1), 'ebal', 0, 1000, 270)\n    set t = CreateUnit(Player(0), 'hfoo', 0, 0, 270)\n    call CreateUnit(Player(0), 'hfoo', 750, 0, 270)\n    call CreateUnit(Player(0), 'hfoo', -530, -530, 270)\n    call CreateUnit(Player(0), 'hfoo', 0, -800, 270)\n    call CreateUnit(Player(0), 'hpea', 100, 0, 270)\n    call IssuePointOrder(CreateUnit(Player(0), 'hfoo', -200, 0, 270), \"move\", -3000, -3000)\n    call IssueTargetOrder(a, \"attack\", t)";
    let Some(mut game) = game("", orders, |_| {}) else {
        return;
    };
    let path = run(&mut game, ORDER_TICK + 600);
    let damage = path
        .iter()
        .find(|p| p.1.get(1).is_some_and(|u| u.life < u.max_life))
        .unwrap()
        .0;
    let units = &path.iter().find(|p| p.0 == damage).unwrap().1;
    let thrower = Some(units[0].handle);
    let targets: Vec<Option<u32>> = units[1..].iter().map(|u| u.target).collect();
    assert_eq!(targets, [thrower, thrower, thrower, None, None, None]);
}

/// The hits of unit 0 on unit `target` in a run: (tick, damage).
fn hits_on(path: &[(u64, Vec<UnitView>)], target: usize) -> Vec<(u64, f32)> {
    path.windows(2)
        .filter(|w| w[0].1.len() > target && w[1].1.len() > target)
        .filter_map(|w| {
            let lost = w[0].1[target].life - w[1].1[target].life;
            (lost > 1.0).then_some((w[1].0, lost))
        })
        .collect()
}

/// SPEC-0016, "Second weapon": the mortar team hits a farm with weapon 2
/// every 3.5 s without area damage; the gargoyle hits a footman with
/// weapon 2 every 2.2 s, a gryphon rider with weapon 1 every 1.4 s. The
/// damage lies within what the dice of that weapon give after armor
/// (the game's samples, 63.5 to 66.9, 16.1 to 18.8 and 62 to 69, too).
#[test]
fn second_weapon() {
    let cases: [(&str, &str, u64, f32, f32); 3] = [
        ("hmtm", "hhou", 700, 60.0, 73.9),
        ("ugar", "hfoo", 440, 16.0, 18.8),
        ("ugar", "hgry", 280, 61.0, 70.0),
    ];
    for (attacker, target, every, low, high) in cases {
        let units = format!(
            "set t = CreateUnit(Player(1), '{target}', 0, 0, 0)\n    call PauseUnit(t, true)\n    set a = CreateUnit(Player(0), '{attacker}', -300, 0, 0)\n    call PauseUnit(CreateUnit(Player(1), 'hfoo', 0, 60, 0), true)"
        );
        let Some(mut game) = game(&units, "call IssueTargetOrder(a, \"attack\", t)", |d| {
            for id in [b"hhou", b"hfoo", b"hgry"] {
                if let Some(t) = d.units.get_mut(&i32::from_be_bytes(*id)) {
                    t.hp = 100_000f32.to_bits();
                }
            }
        }) else {
            return;
        };
        let path = run(&mut game, ORDER_TICK + 3000);
        let hits = hits_on(&path, 0);
        assert!(hits.len() >= 3, "{attacker} on {target}: {hits:?}");
        assert!(
            hits.windows(2).all(|w| w[1].0 - w[0].0 == every),
            "{attacker} on {target}: {hits:?}"
        );
        assert!(
            hits.iter().all(|h| h.1 >= low - 0.05 && h.1 <= high + 0.05),
            "{attacker} on {target}: {hits:?}"
        );
        if attacker == "hmtm" {
            assert!(hits_on(&path, 2).is_empty(), "area damage on the farm");
        }
    }
}

/// SPEC-0016, "Area damage": a mortar team's hit on a footman damages the
/// units round it by distance, the full hit up to 25, 0.4 of it up to 150,
/// 0.1 up to 250, allies of the mortar and neutral units too.
#[test]
fn area_damage() {
    let around = [
        ("Player(0)", 0.0f32, 20.0f32, 1.0f32),
        ("Player(0)", 0.0, 30.0, 0.4),
        ("Player(1)", 0.0, -50.0, 0.4),
        ("Player(15)", 50.0, 50.0, 0.4),
        ("Player(0)", 0.0, 160.0, 0.1),
        ("Player(0)", 0.0, 300.0, 0.0),
    ];
    let mut units = String::from(
        "set t = CreateUnit(Player(0), 'hfoo', 0, 0, 0)\n    call PauseUnit(t, true)\n    set a = CreateUnit(Player(1), 'hmtm', -1000, 0, 0)",
    );
    for (player, x, y, _) in around {
        units += &format!("\n    call PauseUnit(CreateUnit({player}, 'hfoo', {x}, {y}, 0), true)");
    }
    let Some(mut game) = game(&units, "call IssueTargetOrder(a, \"attack\", t)", |d| {
        let footman = d.units.get_mut(&i32::from_be_bytes(*b"hfoo")).unwrap();
        footman.hp = 5000f32.to_bits();
        footman.collision = 0;
    }) else {
        return;
    };
    let path = run(&mut game, ORDER_TICK + 1500);
    let first = hits_on(&path, 0)[0];
    for (i, (_, _, _, share)) in around.iter().enumerate() {
        let hit = hits_on(&path, 2 + i)
            .into_iter()
            .find(|h| h.0 == first.0)
            .map_or(0.0, |h| h.1);
        assert!(
            (hit - share * first.1).abs() < 0.01,
            "unit {i}: {hit} for {share} of {}",
            first.1
        );
    }
}
