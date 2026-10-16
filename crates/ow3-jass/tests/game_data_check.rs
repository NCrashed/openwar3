//! Checks names and types of the JASS scripts of a Warcraft III 1.27b
//! installation, in the environments the game runs them in: `common.j`,
//! `Blizzard.j` and a map's `war3map.j`; or `common.j`, `common.ai` and an
//! AI script. Each test passes without checking anything when the game data
//! is absent.

use std::path::{Path, PathBuf};

use ow3_formats::mpq::Archive;
use ow3_jass::ast::Script;
use ow3_jass::check::check;

type Mpq = Archive<std::io::BufReader<std::fs::File>>;

/// AI scripts in the archives that call `common.ai` functions with older
/// signatures or use names it does not declare. Whether the game ever
/// compiles them is an open question (specs/QUESTIONS.md).
const OUTDATED_AI_SCRIPTS: &[&str] = &[
    "Scripts\\demo.ai",
    "Scripts\\h04_orange.ai",
    "Scripts\\orc03blue.ai",
    "Scripts\\orc03lightblue.ai",
    "Scripts\\orc03purple.ai",
    "Scripts\\orc03teal.ai",
    "Scripts\\orc03warsong.ai",
];

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

fn load(archive: &mut Mpq, name: &str) -> Option<Script> {
    let data = archive.read(name).ok()?;
    Some(ow3_jass::parse(std::str::from_utf8(&data).ok()?).expect("scripts parse"))
}

struct Base {
    common: Script,
    blizzard: Script,
    common_ai: Script,
}

fn base() -> Option<Base> {
    let path = game_data().join("War3Patch.mpq");
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    let mut patch = Archive::open_path(path).unwrap();
    Some(Base {
        common: load(&mut patch, "Scripts\\common.j")?,
        blizzard: load(&mut patch, "Scripts\\Blizzard.j")?,
        common_ai: load(&mut patch, "Scripts\\common.ai")?,
    })
}

#[test]
fn game_scripts_check_cleanly() {
    let Some(base) = base() else { return };
    let program = check(&[&base.common, &base.blizzard]).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(program.function("BJDebugMsg").is_some_and(|f| !f.native));
    assert!(program.function("KillUnit").is_some_and(|f| f.native));
    let unit = program.type_named("unit").unwrap();
    let widget = program.type_named("widget").unwrap();
    assert!(program.assignable(unit, widget));
    assert!(!program.assignable(widget, unit));
}

#[test]
fn ai_scripts_check_except_outdated_ones() {
    let Some(base) = base() else { return };
    let root = game_data();
    let mut archives: Vec<Mpq> = ["War3Patch.mpq", "War3x.mpq", "War3.mpq"]
        .iter()
        .map(|name| Archive::open_path(root.join(name)).unwrap())
        .collect();
    let mut names: Vec<String> = archives[1].list().unwrap();
    names.extend(archives[2].list().unwrap());
    names.retain(|n| n.to_ascii_lowercase().ends_with(".ai") && n != "Scripts\\common.ai");
    names.sort();
    names.dedup();

    let mut failing = Vec::new();
    let mut checked = 0;
    for name in &names {
        let Some(script) = archives.iter_mut().find_map(|a| load(a, name)) else {
            continue;
        };
        checked += 1;
        if check(&[&base.common, &base.common_ai, &script]).is_err() {
            failing.push(name.clone());
        }
    }
    eprintln!("checked {checked} AI scripts, {} outdated", failing.len());
    assert!(checked > 150);
    let unexpected: Vec<_> = failing
        .iter()
        .filter(|n| !OUTDATED_AI_SCRIPTS.contains(&n.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "AI scripts failing the check: {unexpected:?}"
    );
}

fn maps(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            maps(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("w3m") || e.eq_ignore_ascii_case("w3x"))
        {
            out.push(path);
        }
    }
}

#[test]
fn map_scripts_check_against_blizzard_j() {
    let Some(base) = base() else { return };
    let root = game_data().join("Maps");
    let mut paths = Vec::new();
    maps(&root, &mut paths);
    let (mut checked, mut rejected_as_in_game) = (0, 0);
    let mut failures = Vec::new();
    for path in &paths {
        let mut archive = Archive::open_path(path).unwrap();
        let data = ["war3map.j", "Scripts\\war3map.j"]
            .iter()
            .find_map(|name| archive.read(name).ok())
            .unwrap();
        let Ok(script) = ow3_jass::parse(std::str::from_utf8(&data).unwrap()) else {
            continue; // Covered by the parser tests.
        };
        checked += 1;
        let Err(diagnostics) = check(&[&base.common, &base.blizzard, &script]) else {
            continue;
        };
        // SPEC-0001: a map that declares a native named like a Blizzard.j
        // function does not load. Test maps for that spec do exactly this.
        let clash = |d: &ow3_jass::check::Diagnostic| {
            d.script == 2 && d.message.ends_with("is already declared as a function")
        };
        if diagnostics.iter().all(clash) {
            rejected_as_in_game += 1;
            continue;
        }
        let name = path.strip_prefix(&root).unwrap().display();
        for d in diagnostics {
            failures.push(format!("{name} {d}"));
        }
    }
    eprintln!("checked {checked} map scripts, {rejected_as_in_game} rejected as the game does");
    assert!(checked > 100);
    assert!(
        failures.is_empty(),
        "{} problems:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
