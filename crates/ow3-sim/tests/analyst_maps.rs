//! Runs the analysts' test maps from `game-data/1.27b/Maps/Test-ow3/` with
//! the 1.27b `common.j` and `Blizzard.j`, and compares what they print with
//! the results recorded in the specs. Each test passes without checking
//! anything when the game data is absent.

use std::path::PathBuf;

use ow3_formats::mpq::Archive;
use ow3_jass::ast::Script;
use ow3_sim::game::Game;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

fn parse(data: &[u8]) -> Script {
    ow3_jass::parse(std::str::from_utf8(data).unwrap()).unwrap()
}

/// `common.j`, `Blizzard.j` and the map's script, or `None` without data.
fn scripts(map: &str) -> Option<[Script; 3]> {
    let root = game_data();
    let map = root.join("Maps/Test-ow3").join(map);
    if !map.exists() {
        eprintln!("skipping: {} not found", map.display());
        return None;
    }
    let mut patch = Archive::open_path(root.join("War3Patch.mpq")).unwrap();
    let common = parse(&patch.read("Scripts\\common.j").unwrap());
    let blizzard = parse(&patch.read("Scripts\\Blizzard.j").unwrap());
    let mut archive = Archive::open_path(&map).unwrap();
    let script = parse(&archive.read("war3map.j").unwrap());
    Some([common, blizzard, script])
}

/// Runs `config` and `main`, then `seconds` of real time at full speed,
/// and returns the printed lines, or `None` without game data.
fn run(map: &str, seconds: f64) -> Option<Vec<String>> {
    let scripts = scripts(map)?;
    let refs: Vec<&Script> = scripts.iter().collect();
    let program = ow3_jass::check::check(&refs).unwrap();
    let mut game = Game::load(&program, &refs).unwrap();
    game.run("config");
    game.run("main");
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
        let map = dir.join(&name);
        let data = Archive::open_path(&map).unwrap().read("war3map.j").unwrap();
        if let Err(err) = ow3_jass::parse(std::str::from_utf8(&data).unwrap()) {
            println!("== {name}: does not parse ({err})");
            continue;
        }
        let Some(scripts) = scripts(&name) else {
            continue;
        };
        let refs: Vec<&Script> = scripts.iter().collect();
        let program = match ow3_jass::check::check(&refs) {
            Ok(program) => program,
            Err(d) => {
                println!("== {name}: does not check ({})", d[0].message);
                continue;
            }
        };
        let mut game = match Game::load(&program, &refs) {
            Ok(game) => game,
            Err(e) => {
                println!("== {name}: does not load ({e})");
                continue;
            }
        };
        game.run("config");
        game.run("main");
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
/// by the script. Destructables and death events are not simulated yet, so
/// Y3 and Y5 are not checked.
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
            "G1 speed 2",
            "G2 after SLOWEST 0",
            "G3 after FASTEST 2",
            "G4 after SLOW 1",
            "Y4 TriggerExecute action after sleep",
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
