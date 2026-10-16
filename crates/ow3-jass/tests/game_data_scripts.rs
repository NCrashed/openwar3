//! Parses every JASS script of a Warcraft III 1.27b installation: the
//! scripts in the game archives and the `war3map.j` of every map. Expects
//! `game-data/1.27b/` as described in ow3-formats; each test passes without
//! checking anything when the data is absent.

use std::path::{Path, PathBuf};

use ow3_formats::mpq::Archive;
use ow3_jass::ast::Declaration;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

fn check(name: &str, data: &[u8], failures: &mut Vec<String>) -> Option<ow3_jass::ast::Script> {
    let source = match std::str::from_utf8(data) {
        Ok(source) => source,
        Err(err) => {
            failures.push(format!("{name}: not UTF-8 ({err})"));
            return None;
        }
    };
    match ow3_jass::parse(source) {
        Ok(script) => Some(script),
        Err(err) => {
            let line = source
                .lines()
                .nth(err.position.line as usize - 1)
                .unwrap_or("");
            failures.push(format!("{name}:{err}\n    {}", line.trim()));
            None
        }
    }
}

#[test]
fn every_script_in_the_game_archives_parses() {
    let root = game_data();
    if !root.join("War3.mpq").exists() {
        eprintln!("skipping: game data not found");
        return;
    }
    let mut failures = Vec::new();
    let mut names = Vec::new();
    for archive in ["War3.mpq", "War3x.mpq"] {
        names.extend(
            Archive::open_path(root.join(archive))
                .unwrap()
                .list()
                .unwrap(),
        );
    }
    names.retain(|n| {
        let n = n.to_ascii_lowercase();
        n.ends_with(".j") || n.ends_with(".ai")
    });
    names.sort();
    names.dedup();

    let mut parsed = 0;
    for archive in ["War3.mpq", "War3x.mpq", "War3Patch.mpq"] {
        let mut mpq = Archive::open_path(root.join(archive)).unwrap();
        for name in &names {
            if let Ok(data) = mpq.read(name) {
                parsed += 1;
                check(&format!("{archive}/{name}"), &data, &mut failures);
            }
        }
    }
    eprintln!("parsed {parsed} scripts from the game archives");
    assert!(parsed > 20, "only {parsed} scripts");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures.join("\n")
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
fn every_map_script_parses() {
    let root = game_data().join("Maps");
    if !root.exists() {
        eprintln!("skipping: {} not found", root.display());
        return;
    }
    let mut paths = Vec::new();
    maps(&root, &mut paths);
    paths.sort();
    let mut failures = Vec::new();
    let (mut parsed, mut functions) = (0, 0);
    let mut rejected_as_in_game = Vec::new();
    for path in &paths {
        let mut archive = Archive::open_path(path).unwrap();
        let Some(data) = ["war3map.j", "Scripts\\war3map.j"]
            .iter()
            .find_map(|name| archive.read(name).ok())
        else {
            failures.push(format!("{}: no war3map.j", path.display()));
            continue;
        };
        parsed += 1;
        let name = path.strip_prefix(&root).unwrap().display().to_string();
        let mut errors = Vec::new();
        if let Some(script) = check(&name, &data, &mut errors) {
            functions += script.functions.len();
        }
        // Test maps made to check SPEC-0001 declare a native after the
        // first function on purpose; the game rejects them too.
        let (expected, unexpected): (Vec<_>, Vec<_>) = errors
            .into_iter()
            .partition(|e| e.contains("declaration after the first function"));
        rejected_as_in_game.extend(expected);
        failures.extend(unexpected);
    }
    eprintln!("parsed {parsed} map scripts with {functions} functions");
    for rejected in &rejected_as_in_game {
        eprintln!("rejected as the game does (SPEC-0001): {rejected}");
    }
    assert!(parsed > 100, "only {parsed} map scripts");
    assert!(
        failures.is_empty(),
        "{} failed:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
}

/// Verification item 1 of SPEC-0001, through the parser.
#[test]
fn spec_0001_declaration_counts() {
    let path = game_data().join("War3Patch.mpq");
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return;
    }
    let mut archive = Archive::open_path(path).unwrap();
    let mut load = |name: &str| {
        let data = archive.read(name).unwrap();
        ow3_jass::parse(std::str::from_utf8(&data).unwrap()).unwrap()
    };
    let common_j = load("Scripts\\common.j");
    let common_ai = load("Scripts\\common.ai");

    let natives = |script: &ow3_jass::ast::Script| -> Vec<String> {
        script
            .declarations
            .iter()
            .filter_map(|d| match d {
                Declaration::Native(native) => Some(native.signature.name.clone()),
                _ => None,
            })
            .collect()
    };
    let types = common_j
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Type(_)))
        .count();
    let (j, ai) = (natives(&common_j), natives(&common_ai));
    assert_eq!((j.len(), types, ai.len()), (1160, 91, 123));
    assert!(
        j.iter().all(|name| !ai.contains(name)),
        "a native is in both files"
    );
}
