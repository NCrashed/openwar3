//! Terrain images of installed maps. Passes without checking anything when
//! the game data is absent.

use std::path::PathBuf;

use ow3_formats::map::terrain::Terrain;
use ow3_formats::mpq::ArchiveChain;

fn game_data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b")
}

#[test]
fn renders_a_melee_map() {
    let map = game_data().join("Maps/(2)BootyBay.w3m");
    if !map.exists() {
        return;
    }
    let mut chain = ArchiveChain::open(&game_data(), &map).unwrap();
    let terrain = Terrain::parse(&chain.read_map_file("war3map.w3e").unwrap()).unwrap();
    let textures = ow3_render::terrain::ground_textures(&mut chain, &terrain);
    assert!(textures.iter().all(Option::is_some));
    let image = ow3_render::terrain::top_down(&terrain, &textures, 4);
    assert_eq!(
        (image.width, image.height),
        ((terrain.width - 1) * 4, (terrain.height - 1) * 4)
    );
    assert!(image.rgba.chunks(4).all(|p| p[3] == 255));
}

#[test]
fn renders_a_unit_model() {
    let Ok(mut chain) = ArchiveChain::game(&game_data()) else {
        return;
    };
    let Some(data) = chain.read("Units\\Human\\Footman\\Footman.mdx") else {
        return;
    };
    let model = ow3_formats::mdx::Model::parse(&data).unwrap();
    let textures = ow3_render::model::textures(&mut chain, &model, &[]);
    assert!(textures.iter().all(Option::is_some));
    let view = ow3_render::model::View {
        yaw: -30.0,
        pitch: 25.0,
        size: 128,
    };
    let image = ow3_render::model::render(&model, &textures, view);
    let covered = image.rgba.chunks(4).filter(|p| p[3] > 0).count();
    // The footman fills a good part of the square, not all of it.
    assert!((1500..12000).contains(&covered), "{covered}");
}

/// The footman's walk moves its bones, and the rest of the frame stays
/// the same: frames of a loop come back to where they started.
#[test]
fn animates_a_unit_model() {
    use ow3_render::animation::{Moment, Pose};
    let Ok(mut chain) = ArchiveChain::game(&game_data()) else {
        return;
    };
    let Some(data) = chain.read("Units\\Human\\Footman\\Footman.mdx") else {
        return;
    };
    let model = ow3_formats::mdx::Model::parse(&data).unwrap();
    let walk = model
        .sequences
        .iter()
        .position(|s| s.name == "Walk")
        .unwrap();
    let length = model.sequences[walk].interval[1] - model.sequences[walk].interval[0];
    let geoset = &model.geosets[0];
    let skin = |elapsed: i32| {
        Pose::new(&model, Moment::at(&model, walk, elapsed))
            .skin(geoset)
            .0
    };
    let (start, middle) = (skin(0), skin(length as i32 / 2));
    let moved = start
        .iter()
        .zip(&middle)
        .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| (x - y).abs() > 1.0))
        .count();
    assert!(
        moved > start.len() / 10,
        "{moved} of {} vertices moved",
        start.len()
    );
    let end = skin(length as i32);
    for (a, b) in start.iter().zip(&end) {
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 0.5, "the loop does not close: {a:?} {b:?}");
        }
    }
    assert_eq!(ow3_render::model::stand_index(&model), 0);
}
