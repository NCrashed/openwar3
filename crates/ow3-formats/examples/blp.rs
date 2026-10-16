//! Inspect BLP textures and compare the decoder with reference PNG files.
//!
//!     cargo run --example blp -- info    <file.blp>
//!     cargo run --example blp -- compare <blp-dir> <png-dir>
//!
//! `compare` decodes every `.blp` under `blp-dir` and compares it with the
//! PNG of the same relative path (extension `.png`) under `png-dir`, as
//! written by another decoder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ow3_formats::blp::{self, Content};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["info", path] => info(path),
        ["compare", blp_dir, png_dir] => compare(Path::new(blp_dir), Path::new(png_dir)),
        _ => {
            eprintln!("usage: blp info <file.blp> | compare <blp-dir> <png-dir>");
            ExitCode::FAILURE
        }
    }
}

fn info(path: &str) -> ExitCode {
    let data = std::fs::read(path).expect("readable file");
    match blp::read_header(&data) {
        Ok(header) => {
            println!("{header:#?}");
            match blp::decode_mipmaps(&data) {
                Ok(levels) => println!("decoded {} levels", levels.len()),
                Err(err) => println!("decode error: {err}"),
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Default)]
struct Stats {
    files: usize,
    exact: usize,
    /// Histogram of the largest per-channel difference in a file.
    max_diff: BTreeMap<u8, usize>,
    examples: BTreeMap<u8, PathBuf>,
}

fn compare(blp_dir: &Path, png_dir: &Path) -> ExitCode {
    let mut stats: BTreeMap<String, Stats> = BTreeMap::new();
    let mut problems = Vec::new();
    for blp_path in files_with_extension(blp_dir, "blp") {
        let relative = blp_path.strip_prefix(blp_dir).unwrap();
        let png_path = png_dir.join(relative).with_extension("png");
        let data = std::fs::read(&blp_path).unwrap();
        let kind = match blp::read_header(&data) {
            Ok(h) if h.content == Content::Jpeg => format!("jpeg alpha={}", h.alpha_bits),
            Ok(h) => format!("paletted alpha={}", h.alpha_bits),
            Err(_) => "invalid".to_owned(),
        };
        let shown = relative.display();
        match (blp::decode(&data), read_png(&png_path)) {
            (Ok(ours), Some((width, height, theirs))) => {
                if (ours.width, ours.height) != (width, height) {
                    problems.push(format!("{shown}: size differs"));
                    continue;
                }
                let diff = ours
                    .rgba
                    .iter()
                    .zip(&theirs)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                let entry = stats.entry(kind).or_default();
                entry.files += 1;
                entry.exact += usize::from(diff == 0);
                *entry.max_diff.entry(diff).or_default() += 1;
                entry
                    .examples
                    .entry(diff)
                    .or_insert_with(|| relative.to_path_buf());
            }
            (Ok(_), None) => problems.push(format!("{shown}: only we decode it")),
            (Err(err), Some(_)) => problems.push(format!("{shown}: only they decode it ({err})")),
            (Err(err), None) => problems.push(format!("{shown}: neither decodes it ({err})")),
        }
    }
    for (kind, s) in &stats {
        println!("{kind}: {} files, {} identical", s.files, s.exact);
        for (diff, count) in &s.max_diff {
            let example = s.examples[diff].display();
            println!("  max channel diff {diff:>3}: {count:>5} files (e.g. {example})");
        }
    }
    for problem in &problems {
        println!("{problem}");
    }
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn files_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
            {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Reads a PNG as 8-bit RGBA, or `None` when there is no such file.
fn read_png(path: &Path) -> Option<(u32, u32, Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let frame = reader.next_frame(&mut buf).ok()?;
    let pixels = &buf[..frame.buffer_size()];
    let rgba = match frame.color_type {
        png::ColorType::Rgba => pixels.to_vec(),
        png::ColorType::Rgb => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => pixels.iter().flat_map(|&l| [l, l, l, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    Some((frame.width, frame.height, rgba))
}
