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
