//! Writes a top-down image of a map's terrain, and its minimap for
//! comparison.
//!
//! `cargo run --release -p ow3-render --example render_terrain -- <map> <out.png> [pixels per tile]`
//!
//! The game archives are read from `game-data/1.27b`, or from the
//! directory in `OW3_GAME_DATA`. The minimap of the map, if it has one, is
//! written next to the image as `<out>.minimap.png`.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::ExitCode;

use ow3_formats::map::terrain::Terrain;
use ow3_formats::mpq::ArchiveChain;
use ow3_render::image::Image;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [map, out, rest @ ..] = &args[..] else {
        eprintln!("usage: render_terrain <map> <out.png> [pixels per tile]");
        return ExitCode::FAILURE;
    };
    let tile: u32 = rest.first().and_then(|s| s.parse().ok()).unwrap_or(16);
    let game_data = std::env::var_os("OW3_GAME_DATA")
        .map_or_else(|| PathBuf::from("game-data/1.27b"), PathBuf::from);
    let mut chain = match ArchiveChain::open(&game_data, map.as_ref()) {
        Ok(chain) => chain,
        Err(e) => {
            eprintln!("{map}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(terrain) = chain
        .read_map_file("war3map.w3e")
        .and_then(|data| Terrain::parse(&data).ok())
    else {
        eprintln!("{map}: no readable war3map.w3e");
        return ExitCode::FAILURE;
    };
    let textures = ow3_render::terrain::ground_textures(&mut chain, &terrain);
    let tiles = ow3_render::terrain::ground_tiles(&mut chain, &terrain);
    for (id, texture) in tiles.iter().zip(&textures) {
        if texture.is_none() {
            eprintln!("no texture for {id}");
        }
    }
    let image = ow3_render::terrain::top_down(&terrain, &textures, tile);
    let save = |image: &Image, path: &str| {
        let file = BufWriter::new(File::create(path).expect("create output"));
        image.write_png(file).expect("write PNG");
        println!("{path}: {}x{}", image.width, image.height);
    };
    save(&image, out);
    if let Some(minimap) = chain
        .read_map_file("war3mapMap.blp")
        .and_then(|data| ow3_formats::blp::decode(&data).ok())
    {
        save(&Image::from(minimap), &format!("{out}.minimap.png"));
    }
    ExitCode::SUCCESS
}
