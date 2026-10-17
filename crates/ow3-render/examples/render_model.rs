//! Writes a still image of a model from the game archives.
//!
//! `cargo run --release -p ow3-render --example render_model -- <model path> <out.png> [yaw] [pitch] [size] [--replace <id>=<texture path>]...`
//!
//! For example `Units\Human\Footman\Footman.mdx`. Replaceable textures
//! other than team colour and glow come from the object using the model;
//! `--replace 31=ReplaceableTextures\LordaeronTree\LordaeronSummerTree.blp`
//! gives one for a tree. The game archives are
//! read from `game-data/1.27b`, or from the directory in `OW3_GAME_DATA`.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;

use ow3_formats::mdx::Model;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::model::View;

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut replacements = Vec::new();
    while let Some(at) = args.iter().position(|a| a == "--replace") {
        let value = args.get(at + 1).cloned().unwrap_or_default();
        args.drain(at..(at + 2).min(args.len()));
        if let Some((id, path)) = value.split_once('=')
            && let Ok(id) = id.parse::<u32>()
        {
            replacements.push((id, path.to_owned()));
        }
    }
    let [path, out, rest @ ..] = &args[..] else {
        eprintln!("usage: render_model <model path> <out.png> [yaw] [pitch] [size]");
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
    let textures = ow3_render::model::textures(&mut chain, &model, &replacements);
    let image = ow3_render::model::render(&model, &textures, view);
    let file = BufWriter::new(File::create(out).expect("create output"));
    image.write_png(file).expect("write PNG");
    println!(
        "{out}: {} geosets, {} sequences, {} bones",
        model.geosets.len(),
        model.sequences.len(),
        model.bones.len()
    );
    ExitCode::SUCCESS
}
