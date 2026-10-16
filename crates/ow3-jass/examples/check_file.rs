//! Parses and checks a JASS file as a map script, against `common.j` and
//! `Blizzard.j` from `game-data/1.27b/War3Patch.mpq`.
//!
//!     cargo run --example check_file -- path/to/script.j

use ow3_formats::mpq::Archive;

fn main() -> std::process::ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: check_file <script.j>");
        return std::process::ExitCode::FAILURE;
    };
    let mut patch = Archive::open_path("game-data/1.27b/War3Patch.mpq").expect("War3Patch.mpq");
    let mut load = |name: &str| {
        let data = patch.read(name).expect("script in archive");
        ow3_jass::parse(std::str::from_utf8(&data).expect("UTF-8")).expect("game script parses")
    };
    let common = load("Scripts\\common.j");
    let blizzard = load("Scripts\\Blizzard.j");
    let source = std::fs::read_to_string(&path).expect("readable file");
    let script = match ow3_jass::parse(&source) {
        Ok(script) => script,
        Err(err) => {
            eprintln!("{path}:{err}");
            return std::process::ExitCode::FAILURE;
        }
    };
    match ow3_jass::check::check(&[&common, &blizzard, &script]) {
        Ok(_) => {
            println!("{path}: parses and checks");
            std::process::ExitCode::SUCCESS
        }
        Err(diagnostics) => {
            for d in diagnostics {
                eprintln!(
                    "{path}:{}:{}: {}",
                    d.position.line, d.position.column, d.message
                );
            }
            std::process::ExitCode::FAILURE
        }
    }
}
