//! Decodes textures from a real Warcraft III 1.27b installation. See
//! `game_data_mpq.rs` for where the game data is expected; each test passes
//! without checking anything when it is absent.

use std::path::PathBuf;

use ow3_formats::blp::{self, Content};
use ow3_formats::mpq::Archive;

fn texture(name: &str) -> Option<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b/War3.mpq");
    if !path.exists() {
        eprintln!("skipping: {} not found", path.display());
        return None;
    }
    Some(Archive::open_path(&path).unwrap().read(name).unwrap())
}

#[test]
fn jpeg_icon_with_full_mip_chain() {
    let Some(data) = texture("ReplaceableTextures\\CommandButtons\\BTNFootman.blp") else {
        return;
    };
    assert_eq!(blp::read_header(&data).unwrap().content, Content::Jpeg);
    let levels = blp::decode_mipmaps(&data).unwrap();
    assert_eq!(levels.len(), 7, "64x64 down to 1x1");
    assert_eq!((levels[0].width, levels[0].height), (64, 64));
    assert!(
        levels[0].rgba.chunks_exact(4).all(|p| p[3] == 255),
        "icons are opaque"
    );
}

#[test]
fn paletted_shadow_with_alpha() {
    let Some(data) = texture("ReplaceableTextures\\Shadows\\BuildingShadowXLarge.blp") else {
        return;
    };
    let header = blp::read_header(&data).unwrap();
    assert_eq!((header.content, header.alpha_bits), (Content::Paletted, 8));
    let image = blp::decode(&data).unwrap();
    assert_eq!((image.width, image.height), (18, 18));
    let alpha: Vec<u8> = image.rgba.iter().skip(3).step_by(4).copied().collect();
    assert!(
        alpha.iter().any(|&a| a != alpha[0]),
        "a shadow has varying alpha"
    );
}
