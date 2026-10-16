//! Checks against archives from a real Warcraft III 1.27b installation.
//!
//! Place (or symlink) the game's MPQ files in `game-data/1.27b/` at the
//! repository root. Each test passes without checking anything when its
//! archive is absent, so the suite still runs without game data.

use std::path::PathBuf;

use ow3_formats::mpq::Archive;

fn archive(name: &str) -> Option<Archive<std::io::BufReader<std::fs::File>>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../game-data/1.27b")
        .join(name);
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    Some(Archive::open_path(&path).expect("archive opens"))
}

#[test]
fn base_archive_lists_and_reads_unit_data() {
    let Some(mut archive) = archive("War3.mpq") else {
        return;
    };
    let names = archive.list().expect("listfile is readable");
    assert!(names.len() > 10_000);
    let unit_data = archive
        .read("Units\\UnitData.slk")
        .expect("unit data reads");
    assert!(
        unit_data.starts_with(b"ID;"),
        "SLK files start with an ID record"
    );
}

#[test]
fn patch_archive_has_jass_scripts_without_listfile() {
    let Some(mut archive) = archive("War3Patch.mpq") else {
        return;
    };
    let common = archive.read("Scripts\\common.j").expect("common.j reads");
    let common = String::from_utf8(common).expect("common.j is text");
    let natives = common.lines().filter(|l| l.starts_with("native ")).count();
    assert!(natives > 900, "found {natives} natives");
    assert!(archive.contains("Scripts\\Blizzard.j"));
}
