//! Path finding of SPEC-0020 against the path lists read from the game
//! on test map c-PF2. Skipped without game data.

use std::path::PathBuf;

use ow3_sim::path::{Grid, high_level, local};

fn grid() -> Option<Grid> {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../game-data/1.27b");
    let map = data.join("Maps/Test-ow3/c-PF2.w3x");
    if !map.exists() {
        return None;
    }
    ow3_sim::map::Map::load(&data, &map).unwrap().pathing
}

fn point(x: f32, y: f32) -> [u32; 2] {
    [x.to_bits(), y.to_bits()]
}

/// Cells of a world point of the test maps, whose corner is -12288.
fn cells(x: f32, y: f32) -> [u32; 2] {
    point((x + 12288.0) / 32.0, (y + 12288.0) / 32.0)
}

fn close(a: [u32; 2], b: [f32; 2], tolerance: f32) -> bool {
    (f32::from_bits(a[0]) - b[0]).abs() <= tolerance
        && (f32::from_bits(a[1]) - b[1]).abs() <= tolerance
}

/// The collision 8 unit of PF2 (class 0) at (-2500, -9000), ordered to
/// (-1500, -8900): its first high-level list, in blocks.
#[test]
fn high_level_list_of_pf2() {
    let Some(grid) = grid() else {
        return;
    };
    let found = high_level(&grid, cells(-2500.0, -9000.0), cells(-1500.0, -8900.0), 0).unwrap();
    let expected = [
        [152.9375, 51.375],
        [152.75, 56.75],
        [160.75, 58.75],
        [162.75, 58.75],
        [162.75, 57.75],
        [164.75, 55.75],
        [168.5625, 52.9375],
    ];
    let got: Vec<[f32; 2]> = found.points.iter().map(|p| p.map(f32::from_bits)).collect();
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (a, b) in found.points.iter().zip(expected) {
        assert!(close(*a, b, 1e-4), "{got:?}");
    }
}

/// Its local path searched at tick 508 from its position to the order
/// point: 17 points in 66 iterations.
#[test]
fn local_path_of_pf2() {
    let Some(grid) = grid() else {
        return;
    };
    let found = local(
        &grid,
        point(321.145, 117.1653),
        cells(-1500.0, -8900.0),
        0,
        700,
    );
    let mut expected = vec![[321.145, 117.1653]];
    expected.extend(
        [
            (322.5, 116.5),
            (323.5, 116.5),
            (324.5, 115.5),
            (325.5, 114.5),
            (326.5, 113.5),
            (327.5, 112.5),
            (328.5, 111.5),
            (329.5, 110.5),
            (330.5, 109.5),
            (331.5, 109.5),
            (332.5, 108.5),
            (333.5, 108.5),
            (334.5, 108.5),
            (335.5, 107.5),
            (336.5, 106.5),
            (337.125, 105.875),
        ]
        .map(|(x, y)| [x, y]),
    );
    let got: Vec<[f32; 2]> = found.points.iter().map(|p| p.map(f32::from_bits)).collect();
    assert_eq!(found.iterations, 66, "{got:?}");
    assert_eq!(got.len(), expected.len(), "{got:?}");
    for (a, b) in found.points.iter().zip(expected) {
        assert!(close(*a, b, 1e-3), "{got:?}");
    }
}
