//! Parses every pathing texture of a real Warcraft III 1.27b installation
//! (SPEC-0022). See `game_data_mpq.rs` for where the game data is
//! expected; the test passes without checking anything when it is absent.

use std::path::PathBuf;

use ow3_formats::mpq::Archive;
use ow3_formats::tga::Tga;

#[test]
fn every_pathing_texture_parses() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let mut parsed = 0;
    let mut failures = Vec::new();
    for archive in ["War3.mpq", "War3x.mpq"] {
        let path = root.join(archive);
        if !path.exists() {
            eprintln!("skipping: {} not found", path.display());
            return;
        }
        let mut mpq = Archive::open_path(&path).unwrap();
        let names: Vec<String> = mpq
            .list()
            .unwrap()
            .into_iter()
            .filter(|n| {
                let n = n.to_ascii_lowercase();
                n.starts_with("pathtextures\\") && n.ends_with(".tga")
            })
            .collect();
        for name in names {
            match Tga::parse(&mpq.read(&name).unwrap()) {
                Ok(t) if t.width > 0 && t.height > 0 => parsed += 1,
                Ok(_) => failures.push(format!("{name}: empty")),
                Err(e) => failures.push(format!("{name}: {e}")),
            }
        }
    }
    assert!(parsed > 150, "{parsed}");
    assert!(failures.is_empty(), "{failures:#?}");
}
