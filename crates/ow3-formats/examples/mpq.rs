//! Inspect MPQ archives.
//!
//!     cargo run --example mpq -- list  <archive>
//!     cargo run --example mpq -- cat   <archive> <file>
//!     cargo run --example mpq -- check <archive>...
//!     cargo run --example mpq -- extract <archive> <suffix> <dir>

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::process::ExitCode;

use ow3_formats::mpq::{Archive, Error};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["list", path] => list(path),
        ["cat", path, name] => cat(path, name),
        ["check", ref paths @ ..] if !paths.is_empty() => check(paths),
        ["extract", path, suffix, dir] => extract(path, suffix, dir),
        _ => {
            eprintln!(
                "usage: mpq list <archive> | cat <archive> <file> | check <archive>... \
                 | extract <archive> <suffix> <dir>"
            );
            return ExitCode::FAILURE;
        }
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn list(path: &str) -> Result<bool, Error> {
    let mut archive = Archive::open_path(path)?;
    for name in archive.list()? {
        match archive.file_size(&name) {
            Some(size) => println!("{size:>10}  {name}"),
            None => println!("{:>10}  {name}", "-"),
        }
    }
    Ok(true)
}

fn cat(path: &str, name: &str) -> Result<bool, Error> {
    let data = Archive::open_path(path)?.read(name)?;
    std::io::stdout().write_all(&data)?;
    Ok(true)
}

/// Reads every listed file and summarises failures by kind. Archives without
/// a `(listfile)`, such as patch archives, are probed with the names listed
/// by the archives checked before them.
fn check(paths: &[&str]) -> Result<bool, Error> {
    let mut all_ok = true;
    let mut known: BTreeSet<String> = BTreeSet::new();
    for path in paths {
        let mut archive = Archive::open_path(path)?;
        let (names, source) = match archive.list() {
            Ok(names) => (names, "listed"),
            Err(Error::FileNotFound(_)) => (known.iter().cloned().collect(), "probed"),
            Err(err) => return Err(err),
        };
        let (mut ok, mut missing) = (0, 0);
        let mut failures: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for name in &names {
            match archive.read(name) {
                Ok(_) => ok += 1,
                Err(Error::FileNotFound(_)) => missing += 1,
                Err(err) => failures
                    .entry(err.to_string())
                    .or_default()
                    .push(name.clone()),
            }
        }
        println!(
            "{path}: {} {source}, {ok} read, {missing} absent",
            names.len()
        );
        known.extend(names);
        for (error, files) in &failures {
            all_ok = false;
            println!("  {} x {error} (e.g. {})", files.len(), files[0]);
        }
    }
    Ok(all_ok)
}

/// Writes every listed file whose name ends with `suffix` (case-insensitive)
/// under `dir`, turning `\` separators into directories.
fn extract(path: &str, suffix: &str, dir: &str) -> Result<bool, Error> {
    let mut archive = Archive::open_path(path)?;
    let suffix = suffix.to_ascii_lowercase();
    let mut count = 0;
    for name in archive.list()? {
        if !name.to_ascii_lowercase().ends_with(&suffix) {
            continue;
        }
        let target = std::path::Path::new(dir).join(name.replace('\\', "/"));
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, archive.read(&name)?)?;
        count += 1;
    }
    println!("extracted {count} files");
    Ok(true)
}
