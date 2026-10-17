//! Draws a map's ground without a window. Passes without checking anything
//! when the game data or a graphics adapter is absent.

use std::path::PathBuf;

use ow3_viewer::camera::Camera;
use ow3_viewer::renderer::{self, Renderer};
use ow3_viewer::scene::Scene;

#[test]
fn renders_a_map_offscreen() {
    let game_data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let map = game_data.join("Maps/(2)BootyBay.w3m");
    if !map.exists() {
        eprintln!("skipping: {} not found", map.display());
        return;
    }
    let scene = Scene::load(&game_data, &map).unwrap();
    let (device, queue) = match pollster::block_on(renderer::headless_device()) {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("skipping: {e}");
            return;
        }
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let renderer = Renderer::new(&device, &queue, format, &scene.layers, &scene.textures);
    let camera = Camera::new(ow3_viewer::terrain_mesh::centre(&scene.terrain));
    let image = renderer::render_to_image(&device, &queue, &renderer, &camera, 160, 90);
    assert_eq!((image.width, image.height), (160, 90));
    // The ground fills the lower half of the view with colour; the middle
    // of the map is water and grass, so blue and green both show.
    let lower = &image.rgba[image.rgba.len() / 2..];
    let lit = lower
        .chunks(4)
        .filter(|p| p[0].max(p[1]).max(p[2]) > 40)
        .count();
    assert!(lit > lower.len() / 4 * 9 / 10, "{lit} lit pixels");
    let green = lower
        .chunks(4)
        .filter(|p| p[1] > p[2] && p[1] > p[0])
        .count();
    let blue = lower
        .chunks(4)
        .filter(|p| p[2] > p[0] && p[2] >= p[1])
        .count();
    assert!(green > 100 && blue > 100, "green {green}, blue {blue}");
}
