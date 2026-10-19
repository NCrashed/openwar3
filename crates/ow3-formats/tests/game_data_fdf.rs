//! Parses the interface definition files of a real Warcraft III 1.27b
//! installation. See `game_data_mpq.rs` for where the game data is
//! expected; each test passes without checking anything when it is absent.

use std::path::PathBuf;

use ow3_formats::fdf::{Definition, definitions};
use ow3_formats::mpq::Archive;

fn open(name: &str) -> Option<Archive<std::io::BufReader<std::fs::File>>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../game-data/1.27b")
        .join(name);
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    Some(Archive::open_path(&path).unwrap())
}

fn count(d: &[Definition]) -> usize {
    d.iter().map(|d| 1 + count(&d.children)).sum()
}

/// Every file listed in `FrameDef.toc` parses into definitions; the console
/// of `ConsoleUI.fdf` has its nine textures.
#[test]
fn frame_definitions_parse() {
    let Some(mut base) = open("War3.mpq") else {
        return;
    };
    let toc =
        String::from_utf8_lossy(&base.read("UI\\FrameDef\\FrameDef.toc").unwrap()).into_owned();
    let mut files = 0;
    for name in toc.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(data) = base.read(name) else {
            continue;
        };
        let text = String::from_utf8_lossy(&data);
        let found = definitions(&text);
        assert!(!found.is_empty(), "{name}");
        assert!(count(&found) >= found.len());
        files += 1;
    }
    assert!(files > 50, "{files}");
    let text = String::from_utf8_lossy(&base.read("UI\\FrameDef\\UI\\ConsoleUI.fdf").unwrap())
        .into_owned();
    let console = definitions(&text);
    assert_eq!(console[0].name.as_deref(), Some("ConsoleUI"));
    assert_eq!(console[0].children.len(), 9);
    assert!(console[0].children.iter().all(|t| t.kind == "Texture"));
}
