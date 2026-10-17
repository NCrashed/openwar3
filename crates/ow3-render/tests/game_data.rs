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

/// The ramps of Lost Temple as SPEC-0014 counts them: 58 ramp models with
/// 14 names, among them `LABH` on tiles (33, 30) and (33, 31) and `ALHB`
/// on (24, 32) and (24, 33). Every model exists.
#[test]
fn lost_temple_ramps() {
    let map = game_data().join("Maps/FrozenThrone/(4)LostTemple.w3x");
    if !map.exists() {
        return;
    }
    let mut chain = ArchiveChain::open(&game_data(), &map).unwrap();
    let terrain = Terrain::parse(&chain.read_map_file("war3map.w3e").unwrap()).unwrap();
    let mut ramps = Vec::new();
    for y in 0..terrain.height - 1 {
        for x in 0..terrain.width - 1 {
            if let Some(ramp) = ow3_render::cliffs::ramp_tile(&terrain, x, y) {
                ramps.push(((x, y), ramp));
            }
        }
    }
    let names: std::collections::BTreeSet<&str> =
        ramps.iter().map(|(_, r)| r.letters.as_str()).collect();
    for name in &names {
        let path = ow3_render::cliffs::model_path("CliffTrans", name, 0);
        assert!(chain.read(&path).is_some(), "{path}");
    }
    let at = |x, y| {
        ramps
            .iter()
            .find(|(t, _)| *t == (x, y))
            .map(|(_, r)| r.clone())
    };
    let labh = at(33, 30).expect("ramp at (33, 30)");
    assert_eq!(
        (labh.letters.as_str(), labh.origin, labh.size),
        ("LABH", (33, 30), (1, 2))
    );
    let alhb = at(24, 32).expect("ramp at (24, 32)");
    assert_eq!(
        (alhb.letters.as_str(), alhb.origin, alhb.size),
        ("ALHB", (24, 32), (1, 2))
    );
    assert_eq!((ramps.len(), names.len()), (58, 14), "{names:?}");
    // Pond edges with ramp points H or L get cliff models.
    let cliff = |x, y| ow3_render::cliffs::cliff_tile(&terrain, x, y).map(|c| c.letters);
    assert_eq!(cliff(127, 110).as_deref(), Some("BACB"));
    assert_eq!(cliff(127, 111).as_deref(), Some("BBAB"));
    // Every cliff model of the map exists.
    for y in 0..terrain.height - 1 {
        for x in 0..terrain.width - 1 {
            if let Some(tile) = ow3_render::cliffs::cliff_tile(&terrain, x, y) {
                let exists = ["Cliffs", "CityCliffs"].iter().any(|dir| {
                    let path = ow3_render::cliffs::model_path(dir, &tile.letters, 0);
                    chain.contains(&path)
                });
                assert!(exists, "{} at ({x}, {y})", tile.letters);
            }
        }
    }
}
