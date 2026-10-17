//! Writes an image of a model from the game archives, still or as a strip
//! of animation frames.
//!
//! `cargo run --release -p ow3-render --example render_model -- <model path> <out.png> [yaw] [pitch] [size] [options]`
//!
//! Options:
//! - `--sequence <name>`: the sequence to draw (default: Stand);
//! - `--frames <n>`: draw `n` frames spread over the sequence side by side;
//! - `--replace <id>=<texture path>`: a replaceable texture, for example
//!   `--replace 31=ReplaceableTextures\LordaeronTree\LordaeronSummerTree.blp`
//!   for a tree (team colour and glow need none).
//!
//! The game archives are read from `game-data/1.27b`, or from the
//! directory in `OW3_GAME_DATA`.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;

use ow3_formats::mdx::Model;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::animation::Moment;
use ow3_render::image::Image;
use ow3_render::model::View;

/// Removes `--name value` pairs from `args` and returns the values.
fn take_option(args: &mut Vec<String>, name: &str) -> Vec<String> {
    let mut values = Vec::new();
    while let Some(at) = args.iter().position(|a| a == name) {
        values.push(args.get(at + 1).cloned().unwrap_or_default());
        args.drain(at..(at + 2).min(args.len()));
    }
    values
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let replacements: Vec<(u32, String)> = take_option(&mut args, "--replace")
        .iter()
        .filter_map(|v| {
            let (id, path) = v.split_once('=')?;
            Some((id.parse().ok()?, path.to_owned()))
        })
        .collect();
    let sequence_name = take_option(&mut args, "--sequence").pop();
    let frames: usize = take_option(&mut args, "--frames")
        .pop()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1)
        .max(1);
    let [path, out, rest @ ..] = &args[..] else {
        eprintln!("usage: render_model <model path> <out.png> [yaw] [pitch] [size] [options]");
        return ExitCode::FAILURE;
    };
    let number =
        |i: usize, default: f32| rest.get(i).and_then(|s| s.parse().ok()).unwrap_or(default);
    let view = View {
        yaw: number(0, -30.0),
        pitch: number(1, 25.0),
        size: number(2, 512.0) as u32,
    };
    let game_data = std::env::var_os("OW3_GAME_DATA")
        .map_or_else(|| PathBuf::from("game-data/1.27b"), PathBuf::from);
    let mut chain = ArchiveChain::game(&game_data).expect("game archives");
    let Some(data) = chain.read(path) else {
        eprintln!("{path} not found");
        return ExitCode::FAILURE;
    };
    let model = match Model::parse(&data) {
        Ok(model) => model,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let index = match &sequence_name {
        None => ow3_render::model::stand_index(&model),
        Some(name) => {
            let Some(i) = model
                .sequences
                .iter()
                .position(|s| s.name.eq_ignore_ascii_case(name))
            else {
                let names: Vec<&str> = model.sequences.iter().map(|s| s.name.as_str()).collect();
                eprintln!("no sequence {name}; the model has {names:?}");
                return ExitCode::FAILURE;
            };
            i
        }
    };
    let textures = ow3_render::model::textures(&mut chain, &model, &replacements);
    let start = Moment::start(&model, index);
    let length = start.interval[1] - start.interval[0];
    let moments: Vec<Moment> = (0..frames)
        .map(|f| Moment::at(&model, index, length * f as i32 / frames as i32))
        .collect();
    // One frame for all images, so that the model does not jump.
    let frame = moments
        .iter()
        .map(|&m| ow3_render::model::bounds(&model, view, m))
        .reduce(ow3_render::model::union)
        .expect("at least one frame");
    let mut strip = Image::new(view.size * frames as u32, view.size);
    for (f, &moment) in moments.iter().enumerate() {
        let image = ow3_render::model::render_at(&model, &textures, view, moment, frame);
        for y in 0..view.size {
            for x in 0..view.size {
                strip.set(f as u32 * view.size + x, y, image.pixel(x, y));
            }
        }
    }
    let file = BufWriter::new(File::create(out).expect("create output"));
    strip.write_png(file).expect("write PNG");
    println!(
        "{out}: {} ({} ms), {frames} frames; {} geosets, {} bones",
        model
            .sequences
            .get(index)
            .map_or("rest pose", |s| s.name.as_str()),
        length,
        model.geosets.len(),
        model.bones.len()
    );
    ExitCode::SUCCESS
}
