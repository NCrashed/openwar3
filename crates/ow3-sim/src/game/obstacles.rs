//! Buildings and destructables as obstacles (SPEC-0022).
//!
//! A building's `pathTex` and a destructable's `pathTex` (`pathTexDeath`
//! once dead) cover cells of the pathing grid: red pixels close them for
//! walking, green ones for the high-level search. The texture turns by 90
//! degrees counter-clockwise, 180 degrees or 90 degrees clockwise when the
//! object's angle is exactly 0, 90 or 180 degrees, and the object moves to
//! the grid of 64, or of 64 plus 32 for textures whose half size is odd.
//! Doodads need nothing: their footprints are part of the `.wpm`.
//!
//! PROVISIONAL: the grid of 64 is that of world coordinates (open
//! question 1); a building's footprint goes when it dies (open question
//! 2); a destructable's texture changes at once when it dies.

use super::{Object, State};
use crate::data::PathTexture;

/// The cells a footprint covers: red (no walking) and green (no flying).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footprint {
    red: Vec<(i32, i32)>,
    green: Vec<(i32, i32)>,
    /// The cells the whole texture covers, also its unblocked border:
    /// `X0`, `X1`, `Y0`, `Y1` (SPEC-0027).
    pub(super) rect: (i32, i32, i32, i32),
}

impl Footprint {
    /// The red cells, which block walkers.
    pub(super) fn red(&self) -> &[(i32, i32)] {
        &self.red
    }

    /// The green cells, which block building and flying.
    pub(super) fn green(&self) -> &[(i32, i32)] {
        &self.green
    }

    /// Whether one of `cells` is a red cell or a neighbour of one, also
    /// diagonally (SPEC-0029, "Reaching a building").
    pub(super) fn touches(&self, cells: &[(i32, i32)]) -> bool {
        self.red.iter().any(|r| {
            cells
                .iter()
                .any(|c| (r.0 - c.0).abs() <= 1 && (r.1 - c.1).abs() <= 1)
        })
    }
}

/// How a texture turns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    None,
    Left,
    Half,
    Right,
}

/// The turn of a texture for an angle in radians as stored: only exactly
/// 0, 90 and 180 degrees turn it.
fn turn_of(angle: u32) -> Turn {
    let at = |degrees: f32| angle == super::facing::from_degrees(degrees.to_bits());
    if at(0.0) || angle == 0x8000_0000 {
        Turn::Left
    } else if at(90.0) {
        Turn::Half
    } else if at(180.0) {
        Turn::Right
    } else {
        Turn::None
    }
}

/// The size of a texture after `turn`.
fn turned_size(t: &PathTexture, turn: Turn) -> (i32, i32) {
    match turn {
        Turn::Left | Turn::Right => (t.height, t.width),
        _ => (t.width, t.height),
    }
}

/// Coordinate `p` (world) moved to the grid for a texture `w` cells wide
/// along that axis.
fn snapped(p: f32, w: i32) -> f32 {
    let base = 64.0 * (p / 64.0).trunc();
    if (w / 2) % 2 == 0 {
        base
    } else if p < 0.0 {
        base - 32.0
    } else {
        base + 32.0
    }
}

/// Where an object of texture `texture` turned by `turn` at (`x`, `y`)
/// stands after moving to the grid.
fn snap(texture: &PathTexture, turn: Turn, x: u32, y: u32) -> (u32, u32) {
    let (w, h) = turned_size(texture, turn);
    (
        snapped(f32::from_bits(x), w).to_bits(),
        snapped(f32::from_bits(y), h).to_bits(),
    )
}

/// The cells `texture` turned by `turn` covers at (`x`, `y`), a position
/// on the grid, in a world whose lower left corner is `min`.
fn cells(texture: &PathTexture, turn: Turn, x: u32, y: u32, min: [u32; 2]) -> Footprint {
    let centre = [x, y]
        .iter()
        .zip(min)
        .map(|(&p, m)| ((f32::from_bits(p) - f32::from_bits(m)) / 32.0).round() as i32)
        .collect::<Vec<_>>();
    let (w, h) = (texture.width, texture.height);
    let mut footprint = Footprint::default();
    let mut rect = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for py in 0..h {
        for px in 0..w {
            let k = (py * w + px) as usize;
            let (red, green) = (texture.red[k], texture.green[k]);
            let (i, j) = (px - w / 2, py - h / 2);
            let (i, j) = match turn {
                Turn::None => (i, j),
                Turn::Left => (-j - 1, i),
                Turn::Half => (-i - 1, -j - 1),
                Turn::Right => (j, -i - 1),
            };
            let cell = (centre[0] + i, centre[1] + j);
            rect = (
                rect.0.min(cell.0),
                rect.1.max(cell.0),
                rect.2.min(cell.1),
                rect.3.max(cell.1),
            );
            if red {
                footprint.red.push(cell);
            }
            if green {
                footprint.green.push(cell);
            }
        }
    }
    footprint.rect = rect;
    footprint
}

fn stamp(state: &mut State, footprint: &Footprint, add: bool) {
    if let Some(grid) = state.grid.as_mut() {
        grid.stamp(&footprint.red, &footprint.green, add);
    }
}

/// A new building of type `type_id` at (`x`, `y`) facing `facing`
/// (radians as stored): where it stands after moving to the grid, and its
/// footprint, already in the grid.
pub(super) fn place_building(
    state: &mut State,
    type_id: i32,
    x: u32,
    y: u32,
    facing: u32,
) -> ((u32, u32), Option<Footprint>) {
    let texture = state
        .data
        .units
        .get(&type_id)
        .filter(|t| t.building)
        .and_then(|t| t.path_texture.as_ref())
        .and_then(|name| state.data.path_textures.get(name))
        .cloned();
    let Some(texture) = texture else {
        return ((x, y), None);
    };
    let turn = turn_of(facing);
    let (x, y) = snap(&texture, turn, x, y);
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let footprint = cells(&texture, turn, x, y, min);
    stamp(state, &footprint, true);
    ((x, y), Some(footprint))
}

/// Where a building of type `type_id` put at (`x`, `y`) facing `facing`
/// (radians as stored) stands: on the grid of its pathing texture.
pub(super) fn site(state: &State, type_id: i32, x: u32, y: u32, facing: u32) -> (u32, u32) {
    let texture = state
        .data
        .units
        .get(&type_id)
        .filter(|t| t.building)
        .and_then(|t| t.path_texture.as_ref())
        .and_then(|name| state.data.path_textures.get(name));
    match texture {
        Some(texture) => snap(texture, turn_of(facing), x, y),
        None => (x, y),
    }
}

/// The cells the whole pathing texture of a building of type `type_id`
/// covers at (`x`, `y`), a position on its grid, facing `facing`:
/// `X0`, `X1`, `Y0`, `Y1`. `None` for a type without a texture.
pub(super) fn texture_rect(
    state: &State,
    type_id: i32,
    x: u32,
    y: u32,
    facing: u32,
) -> Option<(i32, i32, i32, i32)> {
    let texture = state
        .data
        .units
        .get(&type_id)
        .filter(|t| t.building)
        .and_then(|t| t.path_texture.as_ref())
        .and_then(|name| state.data.path_textures.get(name))?;
    let min = [state.world_bounds[0], state.world_bounds[1]];
    Some(cells(texture, turn_of(facing), x, y, min).rect)
}

/// Whether the cells `rect` (`X0`, `X1`, `Y0`, `Y1`) overlap the texture
/// of a living building of any player or of a living destructable
/// (SPEC-0026, "Where a building may be placed", rule 2).
pub(super) fn overlaps_objects(state: &State, rect: (i32, i32, i32, i32)) -> bool {
    let meets = |f: &Footprint| {
        (!f.red.is_empty() || !f.green.is_empty() || f.rect != (0, 0, 0, 0))
            && f.rect.0 <= rect.1
            && rect.0 <= f.rect.1
            && f.rect.2 <= rect.3
            && rect.2 <= f.rect.3
    };
    state.objects.values().any(|o| match o {
        Object::Unit(u) => u.alive && u.footprint.as_deref().is_some_and(meets),
        Object::Destructable(d) => d.alive && meets(&d.footprint),
        _ => false,
    })
}

/// Widget `h` died or is removed: a building's footprint goes, a
/// destructable's changes to its texture of death.
pub(super) fn died(state: &mut State, h: u32) {
    match state.objects.get_mut(&h) {
        Some(Object::Unit(u)) => {
            if let Some(footprint) = u.footprint.take() {
                stamp(state, &footprint, false);
            }
        }
        Some(Object::Destructable(_)) => refresh_destructable(state, h),
        _ => {}
    }
}

/// Destructable `h` is removed: its footprint goes.
pub(super) fn gone(state: &mut State, h: u32) {
    if let Some(Object::Destructable(d)) = state.objects.get_mut(&h) {
        let footprint = std::mem::take(&mut d.footprint);
        stamp(state, &footprint, false);
    }
}

/// The angle a destructable of type `type_id` facing `facing` (radians
/// as stored) turns its texture by: its `fixedRot` unless negative.
fn destructable_angle(state: &State, type_id: i32, facing: u32) -> u32 {
    match state.data.destructable_paths.get(&type_id) {
        Some(p) if f32::from_bits(p.fixed_rotation) >= 0.0 => {
            super::facing::from_degrees(p.fixed_rotation)
        }
        _ => facing,
    }
}

/// Where a new destructable of type `type_id` at (`x`, `y`) facing
/// `facing` (radians as stored) stands after moving to the grid, by its
/// texture alive or dead.
pub(super) fn place_destructable(
    state: &State,
    type_id: i32,
    x: u32,
    y: u32,
    facing: u32,
    alive: bool,
) -> (u32, u32) {
    let turn = turn_of(destructable_angle(state, type_id, facing));
    match texture_of(state, type_id, alive) {
        Some(texture) => snap(texture, turn, x, y),
        None => (x, y),
    }
}

fn texture_of(state: &State, type_id: i32, alive: bool) -> Option<&PathTexture> {
    let path = state.data.destructable_paths.get(&type_id)?;
    let name = if alive { &path.alive } else { &path.dead };
    state.data.path_textures.get(name.as_ref()?)
}

/// Puts the footprint of destructable `h` in the grid as it is now, alive,
/// dead or gone, in place of the one it had.
pub(super) fn refresh_destructable(state: &mut State, h: u32) {
    let min = [state.world_bounds[0], state.world_bounds[1]];
    let (old, new) = match state.objects.get(&h) {
        Some(Object::Destructable(d)) => {
            let turn = turn_of(destructable_angle(state, d.type_id, d.facing));
            let new = texture_of(state, d.type_id, d.alive)
                .map(|t| cells(t, turn, d.x, d.y, min))
                .unwrap_or_default();
            (d.footprint.clone(), new)
        }
        _ => return,
    };
    stamp(state, &old, false);
    stamp(state, &new, true);
    if let Some(Object::Destructable(d)) = state.objects.get_mut(&h) {
        d.footprint = new;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture(w: i32, h: i32) -> PathTexture {
        let n = (w * h) as usize;
        PathTexture {
            width: w,
            height: h,
            red: vec![true; n],
            green: vec![false; n],
        }
    }

    /// The farm, town hall and destructable rows of SPEC-0022's
    /// verification: given position, moved position and red cells.
    #[test]
    fn snapping_and_cells() {
        let min = [(-12288.0f32).to_bits(); 2];
        let r = |v: f32| v.to_bits();
        let farm = texture(4, 4);
        let (x, y) = snap(&farm, Turn::Left, r(-7999.5), r(-5000.5));
        assert_eq!((f32::from_bits(x), f32::from_bits(y)), (-7936.0, -4992.0));
        let f = cells(&farm, Turn::Left, x, y, min);
        let xs: Vec<i32> = f.red.iter().map(|c| c.0).collect();
        let ys: Vec<i32> = f.red.iter().map(|c| c.1).collect();
        assert_eq!(
            (*xs.iter().min().unwrap(), *xs.iter().max().unwrap()),
            (134, 137)
        );
        assert_eq!(
            (*ys.iter().min().unwrap(), *ys.iter().max().unwrap()),
            (226, 229)
        );
        let (x, _) = snap(&farm, Turn::Left, r(465.0), r(-5065.0));
        assert_eq!(f32::from_bits(x), 448.0);
        // An 8 x 2 texture turned at 0 degrees: 2 wide, 8 high.
        let bar = texture(8, 2);
        let (x, y) = snap(&bar, Turn::Left, r(-8000.0), r(-6000.0));
        assert_eq!((f32::from_bits(x), f32::from_bits(y)), (-8032.0, -5952.0));
        let f = cells(&bar, Turn::Left, x, y, min);
        let xs: Vec<i32> = f.red.iter().map(|c| c.0).collect();
        let ys: Vec<i32> = f.red.iter().map(|c| c.1).collect();
        assert_eq!(
            (*xs.iter().min().unwrap(), *xs.iter().max().unwrap()),
            (132, 133)
        );
        assert_eq!(
            (*ys.iter().min().unwrap(), *ys.iter().max().unwrap()),
            (194, 201)
        );
        // At 30 degrees it stays 8 wide.
        let (x, y) = snap(&bar, Turn::None, r(-6800.0), r(-6000.0));
        assert_eq!((f32::from_bits(x), f32::from_bits(y)), (-6784.0, -5984.0));
    }

    /// The angles of `war3map.doo` in radians, as the floats of 0, pi / 2,
    /// pi and 3 pi / 2, turn as 0, 90, 180 and 270 degrees (SPEC-0022).
    #[test]
    fn angles_of_the_doodad_file() {
        use super::super::facing::{from_degrees, to_degrees};
        let stored = |radians: u32| from_degrees(to_degrees(radians));
        assert_eq!(turn_of(stored(0)), Turn::Left);
        assert_eq!(turn_of(stored(0x3FC9_0FDB)), Turn::Half);
        assert_eq!(turn_of(stored(0x4049_0FDB)), Turn::Right);
        assert_eq!(turn_of(stored(0x4096_CBE4)), Turn::None);
    }

    #[test]
    fn only_exact_angles_turn() {
        let deg = |d: f32| super::super::facing::from_degrees(d.to_bits());
        assert_eq!(turn_of(deg(0.0)), Turn::Left);
        assert_eq!(turn_of(deg(90.0)), Turn::Half);
        assert_eq!(turn_of(deg(180.0)), Turn::Right);
        for d in [270.0, 360.0, 0.5, 45.0, 89.5] {
            assert_eq!(turn_of(deg(d)), Turn::None, "{d}");
        }
    }
}
