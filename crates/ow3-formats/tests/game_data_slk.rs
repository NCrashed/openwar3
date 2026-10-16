//! Parses the SLK tables of a real Warcraft III 1.27b installation. See
//! `game_data_mpq.rs` for where the game data is expected; each test passes
//! without checking anything when it is absent.

use std::path::PathBuf;

use ow3_formats::mpq::Archive;
use ow3_formats::slk::{Table, Value};

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

/// Every table parses and keeps its cells inside the declared size.
#[test]
fn all_tables_parse_within_declared_bounds() {
    let (Some(mut base), Some(mut expansion), Some(mut patch)) =
        (open("War3.mpq"), open("War3x.mpq"), open("War3Patch.mpq"))
    else {
        return;
    };
    let mut names: Vec<String> = base.list().unwrap();
    names.extend(expansion.list().unwrap());
    names.retain(|n| n.to_ascii_lowercase().ends_with(".slk"));
    names.sort();
    names.dedup();

    let mut parsed = 0;
    for archive in [&mut base, &mut expansion, &mut patch] {
        for name in &names {
            let Ok(data) = archive.read(name) else {
                continue;
            };
            let table = Table::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
            for ((row, column), _) in table.cells() {
                assert!(
                    row <= table.declared_rows && column <= table.declared_columns,
                    "{name}: cell ({row}, {column}) outside {}x{}",
                    table.declared_rows,
                    table.declared_columns
                );
            }
            assert!(table.row(1).next().is_some(), "{name}: empty header row");
            parsed += 1;
        }
    }
    assert!(parsed > 100, "parsed only {parsed} tables");
}

#[test]
fn unit_data_has_the_footman() {
    let Some(mut archive) = open("War3Patch.mpq") else {
        return;
    };
    let table = Table::parse(&archive.read("Units\\UnitData.slk").unwrap()).unwrap();
    assert_eq!(table.get(1, 1), Some(&Value::Text("unitID".into())));
    let footman = (2..=table.last_row())
        .find(|&row| table.get(row, 1).and_then(Value::as_text) == Some("hfoo"))
        .expect("hfoo row");
    let race = table.column("race").expect("race column");
    assert_eq!(table.get(footman, race).map(Value::raw), Some("human"));
}
