//! Runs the analysts' test maps from `game-data/1.27b/Maps/Test-ow3/`
//! through the map loader with the 1.27b archives, and compares what they
//! print with the results recorded in the specs. Each test passes without
//! checking anything when the game data is absent.

use std::path::PathBuf;

use ow3_sim::game::Game;
use ow3_sim::map::Map;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

/// The test map `name`, or `None` without game data.
fn load(name: &str) -> Option<Map> {
    let path = game_data().join("Maps/Test-ow3").join(name);
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    Some(Map::load(&game_data(), &path).unwrap())
}

/// Starts the map, lets `seconds` of real time pass at full speed and
/// returns the printed lines, or `None` without game data.
fn run(name: &str, seconds: f64) -> Option<Vec<String>> {
    let map = load(name)?;
    let program = map.check().unwrap();
    let mut game = Game::load_map(&program, &map).unwrap();
    game.start().unwrap();
    game.advance_seconds(seconds);
    let output = game.output();
    for line in &output {
        eprintln!("{line}");
    }
    Some(output)
}

/// Asserts that `expected` appear in `output` in this order, possibly with
/// other lines in between.
fn assert_lines(output: &[String], expected: &[&str]) {
    let mut rest = output.iter();
    for line in expected {
        assert!(
            rest.any(|l| l == line),
            "{line:?} missing or out of order in {output:#?}"
        );
    }
}

/// The number after `marker` in the first line starting with `prefix`.
fn number(output: &[String], prefix: &str, marker: &str) -> f64 {
    let line = output
        .iter()
        .find(|l| l.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line {prefix:?} in {output:#?}"));
    let at = line.find(marker).unwrap() + marker.len();
    let text: String = line[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    text.parse().unwrap()
}

/// SPEC-0002 T3c: firing order and times of timers started together.
#[test]
fn t3c_timer_order() {
    let Some(output) = run("ow3-T3c-order.w3x", 2.0) else {
        return;
    };
    let fires: Vec<(&str, f64)> = output
        .iter()
        .filter_map(|l| l.strip_prefix("T3c fire "))
        .map(|l| {
            let (label, time) = l.split_once(" at ").unwrap();
            (label, time.parse().unwrap())
        })
        .collect();
    let labels: Vec<&str> = fires.iter().map(|f| f.0).collect();
    assert_eq!(labels, ["5", "4", "2", "0", "1", "3"]);
    for ((_, got), measured) in fires.iter().zip([1.0001, 1.499, 1.5, 1.5, 1.5, 1.501]) {
        assert!((got - measured).abs() < 2e-4, "{fires:?}");
    }
}

/// SPEC-0002 SP and SQ: a 1 s sleep lasts 1.1, 0.88 and 0.66 game seconds
/// at speed indexes 2, 1 and 0.
#[test]
fn sp_sleep_and_speed() {
    let Some(output) = run("h-SP.w3x", 25.0) else {
        return;
    };
    for (speed, expected) in [("SLOWEST", 0.66), ("SLOW", 0.88), ("NORMAL", 1.1)] {
        let got = number(&output, &format!("SP MAP_SPEED_{speed} sleep"), "game ");
        assert!((got - expected).abs() < 1e-3, "{speed}: {got}");
    }
    let polled = number(&output, "SP MAP_SPEED_NORMAL PolledWait", "game ");
    assert!((1.17..=1.27).contains(&polled), "PolledWait {polled}");
}

/// SPEC-0002 verification item 10 (SQ): the most common tick counts. The
/// occasional extra chunk of the real game depends on real time.
#[test]
fn pq_sleep_ticks() {
    let Some(output) = run("h-PQ.w3x", 25.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "SQ speed 2 sleep 1.0 ticks: 220 220 220 220 220",
            "SQ speed 1 sleep 1.0 ticks: 176 176 176 176 176",
            "SQ speed 0 sleep 1.0 ticks: 132 132 132 132 132",
        ],
    );
}

/// `GetHandleId` of a `Convert*` value is the integer itself (test B1).
#[test]
fn gs_converted_handle_ids() {
    let Some(output) = run("h-GS.w3x", 3.0) else {
        return;
    };
    assert_lines(&output, &["GS before 2", "GS after SLOWEST 0"]);
}

/// SPEC-0002: the sleep clock runs during a pause; the pause takes effect
/// at the end of the chunk.
#[test]
fn pz_pause() {
    let Some(output) = run("h-PZ.w3x", 5.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "PZ paused, game 1.0000",
            "PZ woke, game 1.0250",
            "PZ timer 0.5 fired at game 1.5000",
        ],
    );
}

/// SPEC-0002 verification items 7 (LT) and 9 (RD).
#[test]
fn lt_long_timers() {
    let Some(output) = run("h-LT.w3x", 460.0) else {
        return;
    };
    for timeout in [
        "299.000", "299.999", "300.000", "301.000", "310.000", "450.000",
    ] {
        let start = format!("timeout={timeout} remaining={timeout}");
        assert!(output.iter().any(|l| l.ends_with(&start)), "{start}");
    }
    // "at sec N + x": N ticks of a periodic 1 s counter started together
    // with the timers, and x the counter's elapsed time.
    let fires: Vec<(f64, i32, f64)> = output
        .iter()
        .filter_map(|l| l.strip_prefix("LT fire timeout="))
        .map(|l| {
            let (timeout, rest) = l.split_once(" at sec ").unwrap();
            let (sec, elapsed) = rest.split_once(" + ").unwrap();
            (
                timeout.parse().unwrap(),
                sec.parse().unwrap(),
                elapsed.parse().unwrap(),
            )
        })
        .collect();
    assert_eq!(fires.len(), 6, "{fires:?}");
    for (timeout, sec, elapsed) in &fires {
        assert!(
            (f64::from(*sec) + elapsed - timeout).abs() < 2e-3,
            "{fires:?}"
        );
    }
    // The 310 s timer fires slightly early, before the counter's 310th tick.
    assert_eq!((fires[4].1, fires[4].2), (309, 1.0), "{fires:?}");
}

#[test]
fn rd_timer_readings() {
    let Some(output) = run("h-CU.w3x", 3.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "RD never started: remaining=0.000 elapsed=0.000 timeout=0.000",
            "RD expired one-shot: remaining=0.000 elapsed=0.500 timeout=0.500",
        ],
    );
}

/// SPEC-0005: a division by zero ends the thread; the caller of
/// `ExecuteFunc` and the next action go on.
#[test]
fn l2_division_by_zero() {
    let Some(output) = run("h-L2-divzero.w3x", 3.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "D0 start",
            "D1 before int /0",
            "D0 after ExecuteFunc OW3_DivZ",
            "D2 before real /0",
            "D0 after ExecuteFunc OW3_DivZR",
            "D9 next action of the same trigger",
        ],
    );
}

/// SPEC-0005: the operation limit ends a thread, and the next action of the
/// trigger runs; a sleep resets the count.
#[test]
fn operation_limit() {
    if let Some(output) = run("h-L4-oplimit.w3x", 3.0) {
        assert!(output.iter().any(|l| l.ends_with("iter=42856")), "L4");
    }
    if let Some(output) = run("h-B2-oplimit-next.w3x", 3.0) {
        assert_lines(&output, &["X1 spin start", "X2 next action ran"]);
    }
    if let Some(output) = run("h-BR.w3x", 5.0) {
        assert_lines(&output, &["BR done 40000 iterations"]);
    }
}

/// SPEC-0005: operation costs, the four published counts.
#[test]
fn oc_operation_costs() {
    let Some(output) = run("h-OC.w3x", 10.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "OC|V00 base|42856",
            "OC|V01 set global = literal|33333",
            "OC|V09 call jass 1 arg|21428",
            "OC|V12 call native 0 args|37499",
        ],
    );
}

/// SPEC-0005: sleeping outside of trigger actions.
#[test]
fn l5_sleep_in_callbacks() {
    let Some(output) = run("h-L5-sleep-outside.w3x", 5.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "Z2 ForForce callback before sleep",
            "Z2 after ForForce",
            "Z3 condition before sleep",
            "Z3 evaluate -> 0",
            "Z4 ExecuteFunc before sleep",
            "Z4 after ExecuteFunc",
            "Z1 timer callback before sleep",
            "Z4 ExecuteFunc after sleep",
        ],
    );
    assert!(
        !output
            .iter()
            .any(|l| l.starts_with("Z1") && l.contains("after"))
    );
    assert!(
        !output
            .iter()
            .any(|l| l.starts_with("Z3") && l.contains("after"))
    );
    assert!(
        !output
            .iter()
            .any(|l| l == "Z2 ForForce callback after sleep")
    );
}

/// SPEC-0005: `or` binds tighter than `and`, both short-circuit.
#[test]
fn q_precedence() {
    let Some(output) = run("h-Q-precedence.w3x", 3.0) else {
        return;
    };
    let results: Vec<&str> = output
        .iter()
        .filter_map(|l| l.trim().strip_prefix("-> "))
        .collect();
    assert_eq!(
        results,
        [
            "Q1: true",
            "Q2: false",
            "Q3: false",
            "Q4: false",
            "Q5: true",
            "Q6: false",
            "Q7: true",
            "Q8: false",
            "Q9: false",
            "Q10: true",
            "Q11: false",
        ]
    );
}

/// SPEC-0005 literals: valid forms and their values. The invalid forms
/// (K4, K9, K10) are rejected by the parser tests.
#[test]
fn k_literals() {
    for (map, line) in [
        ("h-K1-hex.w3x", "K1 0x10=16 0xFF=255 0xffffffff=-1"),
        ("h-K2-dollar.w3x", "K2 $10=16 $ff=255"),
        ("h-K3-hexupper.w3x", "K3 0X1f=31"),
        ("h-K4b-octal-valid.w3x", "K4b 010=8 017=15"),
        (
            "h-K5-overflow.w3x",
            "K5 2147483647+1=-2147483648 4294967296=0",
        ),
        ("h-K11-rawcode-esc.w3x", "K11 '\\n'=10"),
        ("h-K12-real-forms.w3x", "K12 5.=5.000 .25=0.250"),
    ] {
        if let Some(output) = run(map, 3.0) {
            assert_lines(&output, &[line]);
        }
    }
}

/// Exploratory: prints what every test map prints in 25 s. Run with
/// `cargo test -p ow3-sim --test analyst_maps survey -- --ignored --nocapture`.
#[test]
#[ignore]
fn survey() {
    let dir = game_data().join("Maps/Test-ow3");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        let map = match Map::load(&game_data(), &dir.join(&name)) {
            Ok(map) => map,
            Err(e) => {
                println!("== {name}: does not load ({e})");
                continue;
            }
        };
        let program = match map.check() {
            Ok(program) => program,
            Err(e) => {
                println!("== {name}: does not check ({e})");
                continue;
            }
        };
        let mut game = match Game::load_map(&program, &map) {
            Ok(game) => game,
            Err(e) => {
                println!("== {name}: does not load ({e})");
                continue;
            }
        };
        if let Err(e) = game.start() {
            println!("== {name}: does not start ({e})");
            continue;
        }
        game.advance_seconds(25.0);
        println!("== {name}");
        for line in game.output().iter().take(60) {
            println!("   {line}");
        }
        for error in game.state.borrow().errors.iter().take(3) {
            println!("   ! {error}");
        }
    }
}

/// SPEC-0005: `ForGroup` callbacks and `GroupEnum` filters that sleep end
/// at the sleep, and the enumeration goes on. The three units are created
/// by the script. Y3 runs once per destructable of the map, a number the
/// spec does not give. A death event action (Y5) sleeps like any action:
/// the code after `KillUnit` goes on, and the action ends later.
#[test]
fn b1_callbacks_sleep() {
    let Some(output) = run("h-B1-callbacks-speed.w3x", 3.0) else {
        return;
    };
    let count = |line: &str| output.iter().filter(|l| *l == line).count();
    assert_eq!(count("Y1 ForGroup cb before sleep"), 3);
    assert_eq!(count("Y2 filter before sleep"), 3);
    assert!(!output.iter().any(|l| l.ends_with("cb after sleep")));
    assert!(!output.iter().any(|l| l == "Y2 filter after sleep"));
    assert_lines(
        &output,
        &[
            "Y1 after ForGroup",
            "Y2 after GroupEnum",
            "Y4 TriggerExecute action before sleep",
            "Y4 after TriggerExecute",
            "Y5 death event action before sleep",
            "Y5 after KillUnit",
            "G1 speed 2",
            "G2 after SLOWEST 0",
            "G3 after FASTEST 2",
            "G4 after SLOW 1",
            "Y4 TriggerExecute action after sleep",
            "Y5 death event action after sleep",
        ],
    );
}

/// SPEC-0005: `ForForce` callbacks that sleep end at the sleep; in single
/// player `GetPlayersAll()` holds one player.
#[test]
fn b6_forforce() {
    let Some(output) = run("h-B6-forforce.w3x", 3.0) else {
        return;
    };
    assert_eq!(
        output[1..],
        [
            "W cb player 0 before sleep",
            "W cb player 1 before sleep",
            "W cb player 2 before sleep",
            "W after ForForce custom",
            "V cb player 0 no sleep",
            "V after ForForce all (no sleep)",
            "W cb player 0 before sleep",
            "W after ForForce all (sleep)",
        ]
    );
}

/// SPEC-0003 verification, test W1: references resolve in
/// `DisplayTextToPlayer` and stay as they are in script strings.
#[test]
fn w1_trigger_strings() {
    let Some(output) = run("ow3-W1-wts.w3x", 3.0) else {
        return;
    };
    let shown = |n: &str| {
        let at = output
            .iter()
            .position(|l| *l == format!("W1 {n} literal: [TRIGSTR_{n}]"))
            .unwrap_or_else(|| panic!("no W1 {n} in {output:#?}"));
        output[at + 1].clone()
    };
    assert_eq!(shown("900"), "first 900");
    assert_eq!(shown("901"), "");
    assert_eq!(shown("902"), "text 902");
    assert_eq!(shown("903"), "");
    assert_eq!(
        shown("904").lines().collect::<Vec<_>>(),
        ["line one", "line two"]
    );
    assert_eq!(shown("0902"), "text 902");
    assert!(output.contains(&"W1 900: [TRIGSTR_900]".to_owned()));
}

/// SPEC-0003: only resolving natives see the entry text, whatever way the
/// reference was built.
#[test]
fn w2c_resolving_natives() {
    let Some(output) = run("h-W2c.w3x", 3.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "C1 len literal=8 len var=8 len concat=8",
            "C2 SubString=[TRIG] StringCase=[TEXT 910] S2I=0",
            "C3 literal == text: false",
            "C4 concat == literal: true",
            "C5 var == text: false",
        ],
    );
}

/// SPEC-0005 and SPEC-0006 compiler tests: maps the game rejects, and what
/// the accepted ones print.
#[test]
fn c_compiler_maps() {
    let dir = game_data().join("Maps/Test-ow3");
    if !dir.exists() {
        return;
    }
    for name in [
        "c-NM-underscore-end.w3x",
        "c-NM-underscore-start.w3x",
        "c-NU-return-null-int.w3x",
        "c-NU-set-bool-null.w3x",
        "c-NU-set-int-null.w3x",
        "c-NU-set-real-null.w3x",
        "c-SN-global-eq-function.w3x",
        "c-CP-int-eq-bool.w3x",
        "c-OR-call-before-decl.w3x",
        "c-OR-funcref-before-decl.w3x",
        "c-OR-global-init-func.w3x",
        "c-OR-global-init-later.w3x",
        "c-RB-early-handle-int.w3x",
        "c-RB-early-string-int.w3x",
        "c-RW-debug.w3x",
    ] {
        let rejected =
            Map::load(&game_data(), &dir.join(name)).map_or(true, |m| m.check().is_err());
        assert!(rejected, "{name} should not load");
    }
    for (name, lines) in [
        ("c-LC-repeat-diff.w3x", &["LC diff = 2.500"][..]),
        (
            "c-LC-local-is-param-difftype.w3x",
            &["LC local=param difftype = 9.500"],
        ),
        ("c-NU-arg-int-null.w3x", &["NU arg null = 0"]),
        ("c-NU-native-int-null.w3x", &["NU I2S(null) = 0"]),
        ("c-CP-unit-eq-player.w3x", &["CP unit==player: false"]),
        ("c-CP-int-eq-real.w3x", &["CP 1==1.0: true"]),
        ("c-NL-string-lf.w3x", &["NL len = 3"]),
        ("c-CR-line-ending.w3x", &["CR x=2"]),
        ("c-NM-digit-start.w3x", &["NM x1 = 4"]),
        (
            "c-EF-unknown.w3x",
            &["EF before unknown", "EF after unknown"],
        ),
        ("c-EF-params.w3x", &["EF before params"]),
    ] {
        let Some(output) = run(name, 3.0) else {
            continue;
        };
        assert_lines(&output, lines);
    }
}

/// SPEC-0001 test B: a map may declare and call the `common.ai` native
/// `UnitAlive`; it is true for a new footman and false after `KillUnit`.
#[test]
fn b_unit_alive() {
    let Some(output) = run("ow3-B-unitalive.w3x", 3.0) else {
        return;
    };
    assert_lines(&output, &["alive before: 1", "alive after: 0"]);
}

/// SPEC-0003: values stored by resolving natives keep the entry text.
#[test]
fn w2a_stored_texts() {
    let Some(output) = run("h-W2a-wts-forms.w3x", 3.0) else {
        return;
    };
    assert_lines(
        &output,
        &[
            "GET leaderboard label=[text 910]",
            "GET multiboard title=[text 910]",
            "GET localized=[text 910]",
            "GET length=8",
        ],
    );
}

/// A custom type of the map (`war3map.w3u`): the target of the SPEC-0016
/// attack tests, a footman with 10000 life and armor type `normal`, keeps
/// the footman's weapon.
#[test]
fn custom_unit_type() {
    let Some(map) = load("c-AT10.w3x") else {
        return;
    };
    let code = |c: &[u8; 4]| i32::from_be_bytes(*c);
    let target = &map.data.units[&code(b"hv03")];
    let footman = &map.data.units[&code(b"hfoo")];
    assert_eq!(f32::from_bits(target.hp), 10_000.0);
    assert_eq!(target.armor_type, Some(4));
    assert_eq!(target.weapon, footman.weapon);
    assert_eq!(target.name.as_deref(), Some("custom_hv03"));
}

/// The positions the PF maps print every tick (`T tick unit x y ...`, reals
/// as bit patterns), by unit: (tick, x, y).
fn tracks(output: &[String]) -> std::collections::BTreeMap<u32, Vec<(u32, f32, f32)>> {
    let mut tracks: std::collections::BTreeMap<u32, Vec<(u32, f32, f32)>> = Default::default();
    for line in output {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() < 5 || f[0] != "T" {
            continue;
        }
        let real = |s: &str| f32::from_bits(s.parse::<i32>().unwrap() as u32);
        tracks.entry(f[2].parse().unwrap()).or_default().push((
            f[1].parse().unwrap(),
            real(f[3]),
            real(f[4]),
        ));
    }
    tracks
}

/// Where a unit came to rest and its last tick of motion.
fn rest(track: &[(u32, f32, f32)]) -> ((f32, f32), u32) {
    let last = track.last().unwrap();
    let moved = track
        .windows(2)
        .filter(|w| (w[0].1, w[0].2) != (w[1].1, w[1].2))
        .map(|w| w[1].0)
        .next_back()
        .unwrap_or(0);
    ((last.1, last.2), moved)
}

/// SPEC-0020, verification: where the units of PF1 and PF2 come to rest
/// round their walls, within 0.1 with the movement arithmetic of
/// SPEC-0018, and their last tick of motion (the maps print every 6 ticks
/// after the start, so within 6).
#[test]
fn paths_round_walls() {
    // (map, unit, rest x, rest y, last moving tick, tolerance)
    let cases = [
        ("c-PF1.w3x", 0, -1507.54, -6007.75, 989, 0.1),
        ("c-PF1.w3x", 2, -1503.87, 4.38, 851, 0.1),
        ("c-PF1.w3x", 3, -1507.72, 3010.23, 1127, 0.1),
        ("c-PF2.w3x", 0, -1506.70, -8894.89, 995, 0.1),
        ("c-PF2.w3x", 1, -1509.09, -6892.18, 983, 0.1),
        ("c-PF2.w3x", 3, -1507.10, -2893.71, 1031, 0.1),
        ("c-PF2.w3x", 5, -1507.68, 1107.82, 1031, 0.1),
        ("c-PF2.w3x", 6, -1507.75, 3108.23, 1067, 0.1),
        ("c-PF2.w3x", 8, -1509.22, 7108.35, 1007, 0.1),
        ("c-PF2.w3x", 9, -1505.94, 9105.19, 791, 0.1),
    ];
    for map in ["c-PF1.w3x", "c-PF2.w3x"] {
        let Some(output) = run(map, 8.0) else {
            return;
        };
        let tracks = tracks(&output);
        for &(_, unit, x, y, last, tolerance) in cases.iter().filter(|c| c.0 == map) {
            let ((rx, ry), moved) = rest(&tracks[&unit]);
            let off = (rx - x).hypot(ry - y);
            assert!(
                off <= tolerance,
                "{map} unit {unit}: rest ({rx}, {ry}), {off} off"
            );
            assert!(
                moved.abs_diff(last) <= 6,
                "{map} unit {unit}: last moving {moved} for {last}"
            );
        }
    }
}

/// SPEC-0021, walking units (c-OM461): the eastward footman of the first
/// pair is made to wait from tick 364 at (-2022.15, -6000.01), and the
/// footman of the fourth pair waits for the knight from tick 316 at
/// (-2079.42, 2994.36). The maps print every 6 ticks, so the stands begin
/// within 6 ticks. The second and third pairs turn on one unit of the
/// last place of the squared speeds and are left out, as in the spec.
#[test]
fn units_wait_for_each_other() {
    let Some(output) = run("c-OM461.w3x", 9.0) else {
        return;
    };
    let tracks = tracks(&output);
    for (unit, from, until, x, y) in [
        (0, 364, 459, -2022.15f32, -6000.01f32),
        (6, 316, 370, -2079.42, 2994.36),
    ] {
        let track = &tracks[&unit];
        for &(tick, px, py) in track.iter().filter(|p| (from + 6..=until).contains(&p.0)) {
            let off = (px - x).hypot(py - y);
            assert!(off < 0.1, "unit {unit} at tick {tick}: ({px}, {py})");
        }
        let before = track.iter().rev().find(|p| p.0 < from - 6).unwrap();
        assert!(
            (before.1 - x).hypot(before.2 - y) > 0.1,
            "unit {unit} stands early"
        );
    }
}

/// SPEC-0022, verification 1 (c-BG2): buildings move to the grid of their
/// pathing textures; the necropolis at facings 0, 90, 180 and 270 and the
/// farm, as `GetUnitX` and `GetUnitY` give them.
#[test]
fn buildings_move_to_the_grid() {
    let Some(output) = run("c-BG2.w3x", 3.0) else {
        return;
    };
    let placed: std::collections::BTreeMap<u32, (f32, f32)> = output
        .iter()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            let real = |s: &str| f32::from_bits(s.parse::<i32>().ok()? as u32).into();
            (f.len() >= 4 && f[0] == "S").then(|| {
                let x: Option<f32> = real(f[2]);
                let y: Option<f32> = real(f[3]);
                Some((f[1].parse().ok()?, (x?, y?)))
            })?
        })
        .collect();
    for (unit, x, y) in [
        (0, -8000.0, -8000.0),
        (1, -6784.0, -8000.0),
        (2, -5568.0, -8000.0),
        (3, -4352.0, -8000.0),
        (9, -7936.0, -4992.0),
    ] {
        assert_eq!(placed[&unit], (x, y), "unit {unit}");
    }
}

/// SPEC-0021, shared budget (c-UO2): the first searches of the walkers
/// created last use up the budget of 1100 iterations, so the four walkers
/// created first queue and start moving one update after the other eight.
/// The walkers are the units of even index.
#[test]
fn searches_share_a_budget() {
    let Some(output) = run("c-UO2.w3x", 4.0) else {
        return;
    };
    let tracks = tracks(&output);
    let first_motion = |unit: u32| {
        let track = &tracks[&unit];
        track
            .windows(2)
            .find(|w| (w[0].1, w[0].2) != (w[1].1, w[1].2))
            .map(|w| w[1].0)
            .unwrap()
    };
    let late: Vec<u32> = (0..4).map(|k| first_motion(2 * k)).collect();
    let early: Vec<u32> = (4..12).map(|k| first_motion(2 * k)).collect();
    assert!(early.iter().all(|&t| t == early[0]), "{early:?}");
    assert!(
        late.iter().all(|&t| t == early[0] + 6),
        "{late:?} after {early:?}"
    );
}

/// SPEC-0019, verification 5 (c-CH1, rows 1 and 4): chases by paths, in
/// the ticks of the spec's table since the update cycle has the game's
/// phase (SPEC-0018, "Phase from the start of the game"). Row 1: swings from the
/// updates 322 and 688, chase resumed with first motion at 533 and 893,
/// one update standing at 598 and 958, damage at 427 (428 here) and 788;
/// row 4: a swing from 706, damage at 806.
#[test]
fn chases_by_paths() {
    let Some(output) = run("c-CH1.w3x", 6.0) else {
        return;
    };
    let tracks = tracks(&output);
    // First tick without motion and first tick with motion again, in the
    // spec's ticks.
    let stands = |unit: u32| -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        let mut since = None;
        for w in tracks[&unit].windows(2) {
            let still = (w[0].1, w[0].2) == (w[1].1, w[1].2);
            match (still, since) {
                (true, None) => since = Some(w[1].0),
                (false, Some(s)) => {
                    out.push((s, w[1].0));
                    since = None;
                }
                _ => {}
            }
        }
        out
    };
    let row1 = stands(2);
    for stand in [(323, 533), (599, 605), (689, 893), (959, 965)] {
        assert!(row1.contains(&stand), "{stand:?} not in {row1:?}");
    }
    assert!(stands(0).contains(&(707, 911)), "{:?}", stands(0));
    let damage: Vec<(u32, u32)> = output
        .iter()
        .filter_map(|l| {
            let f: Vec<&str> = l.split(' ').collect();
            (f.len() >= 3 && f[0] == "D")
                .then(|| (f[2].parse().unwrap(), f[1].parse::<u32>().unwrap()))
        })
        .collect();
    assert!(
        damage.contains(&(2, 428)) && damage.contains(&(2, 788)),
        "{damage:?}"
    );
    assert!(damage.contains(&(0, 806)), "{damage:?}");
}

/// A walker of SPEC-0022: map, unit, position at tick 292, the tick its
/// order ends and where it rests.
type Walker = (&'static str, u32, (f32, f32), u32, (f32, f32));

/// The positions the PF and B maps print (`T tick unit x y facing order`,
/// reals as bit patterns), by unit: (tick, x, y, order).
fn orders(output: &[String]) -> std::collections::BTreeMap<u32, Vec<(u32, f32, f32, i32)>> {
    let mut out: std::collections::BTreeMap<u32, Vec<(u32, f32, f32, i32)>> = Default::default();
    for line in output {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() < 7 || f[0] != "T" {
            continue;
        }
        let real = |s: &str| f32::from_bits(s.parse::<i32>().unwrap() as u32);
        out.entry(f[2].parse().unwrap()).or_default().push((
            f[1].parse().unwrap(),
            real(f[3]),
            real(f[4]),
            f[6].parse().unwrap(),
        ));
    }
    out
}

/// SPEC-0022, verifications 4 and 5: walkers past buildings (c-BW1) and
/// past green-only obstacles (c-BC1, and c-BC2 with `.wpm` cells instead).
/// The maps count ticks 2 lower than the spec and print every 6 ticks
/// later on, so the spec's tick 292 is compared with the printed tick
/// after it (one tick of motion, 1.35, at most apart) and the end of the
/// order with the 6 printed ticks before it.
#[test]
// The numbers are written as the spec gives them.
#[allow(clippy::excessive_precision)]
fn walkers_past_buildings() {
    // (map, unit, position at 292, order ends, rest)
    let cases: [Walker; 11] = [
        (
            "c-BW1.w3x",
            0,
            (-2358.3994, -7836.124),
            1127,
            (-1311.9971, -7993.6968),
        ),
        (
            "c-BW1.w3x",
            2,
            (-2315.6211, -6057.0381),
            1061,
            (-1307.7354, -6004.7988),
        ),
        (
            "c-BW1.w3x",
            4,
            (-2324.4551, -3903.0527),
            1085,
            (-1310.3691, -3997.1836),
        ),
        (
            "c-BW1.w3x",
            6,
            (-2311.2285, -1999.9893),
            1037,
            (-1315.082, -2000.0352),
        ),
        (
            "c-BW1.w3x",
            8,
            (-2318.332, 65.1357),
            1055,
            (-1310.5762, 1.8174),
        ),
        (
            "c-BW1.w3x",
            10,
            (-2311.6348, 2017.46),
            1043,
            (-1308.6797, 2000.5273),
        ),
        (
            "c-BC1.w3x",
            0,
            (-2111.5938, -6016.5),
            742,
            (-1514.4121, -6001.7783),
        ),
        (
            "c-BC1.w3x",
            1,
            (-2114.875, -2947.0303),
            760,
            (-1513.5557, -2994.5225),
        ),
        (
            "c-BC1.w3x",
            2,
            (-2122.5381, 86.3047),
            767,
            (-1510.2676, 3.2852),
        ),
        (
            "c-BC2.w3x",
            0,
            (-2111.5938, -6016.5),
            742,
            (-1514.4121, -6001.7783),
        ),
        (
            "c-BC2.w3x",
            1,
            (-2114.875, -2947.0303),
            760,
            (-1513.5557, -2994.5225),
        ),
    ];
    for map in ["c-BW1.w3x", "c-BC1.w3x", "c-BC2.w3x"] {
        let Some(output) = run(map, 7.0) else {
            return;
        };
        let tracks = orders(&output);
        for &(_, unit, at, end, (rx, ry)) in cases.iter().filter(|c| c.0 == map) {
            let track = &tracks[&unit];
            let near = track.iter().find(|p| p.0 + 2 >= 292).unwrap();
            let off = (near.1 - at.0).hypot(near.2 - at.1);
            assert!(off <= 1.5, "{map} unit {unit} at tick 292: {near:?}");
            let ended = track.iter().find(|p| p.3 == 0).unwrap().0 + 2;
            assert!(
                (ended - 6 + 1..=ended).contains(&end),
                "{map} unit {unit}: order ends by {ended}"
            );
            let rest = track.last().unwrap();
            let off = (rest.1 - rx).hypot(rest.2 - ry);
            assert!(
                off <= 0.07,
                "{map} unit {unit} rests at {rest:?}, {off} off"
            );
        }
    }
}

/// SPEC-0019, verification 6 (c-CH5): 13 footmen attacking peasants that
/// walk away, ordered from a 0 s timer of `main`, with different reaction
/// delays: their first motion and the update their first swing starts
/// from (the first tick standing after walking), in the map's ticks as in
/// the spec's table. Two of them do not swing before 840.
#[test]
fn swings_against_the_goal_point() {
    let Some(output) = run("c-CH5.w3x", 6.0) else {
        return;
    };
    let tracks = tracks(&output);
    let first_motion = [5, 5, 47, 17, 29, 17, 5, 5, 23, 17, 29, 29, 47];
    let swing = [706, 706, 0, 802, 820, 802, 712, 712, 814, 802, 826, 826, 0];
    for k in 0..13 {
        let track = &tracks[&(2 * k)];
        let moved = track
            .windows(2)
            .find(|w| (w[0].1, w[0].2) != (w[1].1, w[1].2))
            .map(|w| w[1].0);
        assert_eq!(moved, Some(first_motion[k as usize]), "first motion of {k}");
        // The update of the swing: the last tick of motion before the
        // first stand, which is the update the attacker stops at.
        let stood = track
            .windows(2)
            .filter(|w| w[0].0 > first_motion[k as usize])
            .find(|w| (w[0].1, w[0].2) == (w[1].1, w[1].2))
            .map(|w| w[0].0)
            .filter(|&t| t <= 840);
        let expected = (swing[k as usize] != 0).then_some(swing[k as usize]);
        assert_eq!(stood, expected, "swing of {k}");
    }
}
