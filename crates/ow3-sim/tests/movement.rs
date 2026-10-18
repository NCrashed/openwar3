//! The move order of SPEC-0018 with the custom data set of the game, on a
//! world whose lower left corner is (-12288, -12288). Skipped without game
//! data.

use ow3_sim::game::Game;

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
globals
    unit footman = null
endglobals
function Go takes nothing returns nothing
    call IssuePointOrder(footman, "move", TARGET)
endfunction
function main takes nothing returns nothing
    set footman = CreateUnit(Player(0), 'hfoo', -3000, -2000, 0)
    call TimerStart(CreateTimer(), 0.5, false, function Go)
endfunction
"#;

/// A footman at (-3000, -2000) facing east, ordered at tick 100 to move
/// to `target` ("x, y" in JASS).
fn game_to(target: &str) -> Option<Game<'static>> {
    game_with(target, |_| {})
}

/// The same, with the footman's data changed by `change`.
fn game_with(
    target: &str,
    change: impl FnOnce(&mut ow3_sim::data::UnitType),
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
    change(data.units.get_mut(&i32::from_be_bytes(*b"hfoo")).unwrap());
    let text = SCRIPT.replace("TARGET", target);
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

/// (tick, x, y, walking, facing) of the footman at every tick up to
/// `until`.
fn track(game: &mut Game, until: u64) -> Vec<(u64, f32, f32, bool, f32)> {
    let mut out = Vec::new();
    while game.ticks() < until {
        game.step();
        let u = game.units()[0];
        out.push((game.ticks(), u.x, u.y, u.walking, u.facing));
    }
    out
}

/// Verification 1 of SPEC-0018.
#[test]
// The positions are written as the spec gives them.
#[allow(clippy::excessive_precision)]
fn footman_walks_east() {
    let Some(mut game) = game_to("-2000, -2000") else {
        return;
    };
    let path = track(&mut game, 1000);
    let at = |tick: u64| *path.iter().find(|p| p.0 == tick).unwrap();
    // The order: update 1 comes 6 ticks after it, and motion the tick
    // after that.
    let first = path.iter().find(|p| p.1 != -3000.0).unwrap().0;
    let order = first - 7;
    // The steps after update 1 in 1024ths of the stored coordinate near
    // 9288.
    let start = at(order + 6).1;
    let steps: Vec<i32> = (1..=6)
        .map(|n| ((at(order + 6 + n).1 - start) * 1024.0).round() as i32)
        .collect();
    assert_eq!(steps, [1382, 2764, 4147, 5529, 6911, 8294]);
    // The stored positions relative to the corner at ticks 12 to 30
    // (SPEC-0018, "Movement arithmetic"), exactly.
    for (tick, x, y) in [
        (12, 9296.099609f32, 10288.043945f32),
        (18, 9304.199219, 10288.016602),
        (24, 9312.298828, 10287.977539),
        (30, 9320.398438, 10287.990234),
    ] {
        let corner = 12288f32.to_bits();
        let expected = [x, y].map(|c| f32::from_bits(ow3_math::sub(c.to_bits(), corner)));
        let p = at(order + tick);
        assert_eq!([p.1, p.2], expected, "tick {tick}");
    }
    // At rest from update 123, 738 ticks after the order, at -2011.9326 in
    // the game (within 0.1: the game's occasional extra commits between
    // updates are open in SPEC-0018).
    let rest = at(order + 738);
    assert!((rest.1 - -2011.9326).abs() < 0.1, "{rest:?}");
    assert!(at(order + 737).3 && !rest.3);
    assert_eq!(at(order + 800).1, rest.1);
}

/// The update of the first motion for a target 1000 away at `degrees`.
fn first_motion(degrees: f32) -> Option<u64> {
    // The target as a script computes it, with the natives of SPEC-0011.
    use ow3_math::natives::{cos, deg2rad, sin};
    let r = |x: f32| x.to_bits();
    let a = deg2rad(r(degrees));
    let x = ow3_math::add(r(-3000.0), ow3_math::mul(r(1000.0), cos(a)));
    let y = ow3_math::add(r(-2000.0), ow3_math::mul(r(1000.0), sin(a)));
    let target = format!("{:?}, {:?}", f32::from_bits(x), f32::from_bits(y));
    let mut game = game_to(&target)?;
    let path = track(&mut game, 300);
    let first = path.iter().find(|p| (p.1, p.2) != (-3000.0, -2000.0))?.0;
    // The order came at tick 101: motion starts the tick after update k.
    Some((first - 101 - 1) / 6)
}

/// The start table of SPEC-0018 for the footman (turnRate 0.6, propWin
/// 60).
#[test]
fn when_the_footman_starts() {
    let Some(_) = game_to("0, 0") else {
        return;
    };
    for (angles, update) in [
        (&[30.0, 45.0][..], 1),
        // Exactly 60 degrees is left out: the angle comes out 3 units of the
        // last place below propWin here, so the unit starts at update 1
        // (specs/QUESTIONS.md).
        (&[70.0, 80.0, 90.0, -90.0][..], 2),
        (&[100.0, 110.0, 120.0][..], 3),
        (&[135.0, 150.0][..], 4),
        (&[165.0, 180.0][..], 5),
    ] {
        for &a in angles {
            assert_eq!(first_motion(a), Some(update), "{a} degrees");
        }
    }
}

/// Verification 3 of SPEC-0018: with turnRate 0.1 the facing of a footman
/// turning round goes 4.0107, 10.6987, 16.3289, 22.0670, 27.7958, 33.5255
/// degrees at updates 1 to 6: two growing steps, then the brake keeps it
/// 0.85 degrees behind the heading. The brake table is interpolated
/// linearly here (log-log in the spec), which costs up to 0.002 degrees.
#[test]
fn facing_follows_a_slow_heading() {
    let Some(mut game) = game_with("-4000, -2000", |t| t.turn_rate = 0.1f32.to_bits()) else {
        return;
    };
    let path = track(&mut game, 200);
    let facings: Vec<f32> = (1..=6)
        .map(|k| {
            path.iter()
                .find(|p| p.0 == 101 + 6 * k)
                .unwrap()
                .4
                .to_degrees()
        })
        .collect();
    let expected = [4.0107, 10.6987, 16.3289, 22.0670, 27.7958, 33.5255];
    for (got, want) in facings.iter().zip(expected) {
        assert!((got - want).abs() < 0.002, "{facings:?}");
    }
}
