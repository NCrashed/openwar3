//! Fog of war (SPEC-0032): what each player's units reveal.
//!
//! Every player has a grid of fog cells, the 128 x 128 tiles of the
//! terrain, each masked (never seen), fogged (seen before) or visible.
//! Every 80 ticks (ticks 81, 161, ...) the visible cells become fogged and
//! each unit reveals the cells around the cell it stands in: a flying unit
//! the whole circle of its radius, a ground unit the cells of its mask
//! that higher ground and trees do not hide. A unit made reveals at once.
//! A unit's cells are revealed to its owner and to the players its owner
//! gives shared vision. Scripts set cells with fog modifiers and
//! `SetFogState...`.
//!
//! PROVISIONAL: a unit made between updates sees past the trees and
//! cliffs of the last update; the clock of day and night is that of the
//! `night` regeneration type; dead and hidden units reveal nothing; a
//! destructable blocks sight while it is alive. `FogEnable` and
//! `FogMaskEnable` are kept for the viewer and do not change the grid.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use ow3_jass::semantics::Value;
use ow3_jass::vm::{NativeFn, Vm};
use ow3_math::{add, floor, mul, r2i};

use super::{Object, State, handle, real};

/// The states of a fog cell.
pub const MASKED: u8 = 0;
pub const FOGGED: u8 = 1;
pub const VISIBLE: u8 = 2;

/// Ticks between two updates of the fog (0.4 s), and the residue of the
/// ticks they come at.
const UPDATE_TICKS: u64 = 80;
const PHASE: u64 = 1;
/// The largest radius in cells.
const MOST: i32 = 14;
/// 1 / 128 and 0.5 as bit patterns of reals.
const PER_CELL: u32 = 0x3C00_0000;
const HALF: u32 = 0x3F00_0000;

/// The fog of all players.
#[derive(Clone, Debug)]
pub struct Fog {
    /// The cells across and up.
    size: (u32, u32),
    /// Each player's cells, row by row from the bottom.
    grids: BTreeMap<i32, Vec<u8>>,
    /// The masks of ground units and the circles of flying ones, by radius.
    masks: Vec<Vec<(i32, i32)>>,
    circles: Vec<Vec<(i32, i32)>>,
    /// The height of each cell at the last update.
    height: Vec<i32>,
    /// Fog modifiers in the order they were made.
    modifiers: Vec<u32>,
    /// `FogEnable` and `FogMaskEnable`.
    pub enabled: bool,
    pub mask_enabled: bool,
}

impl Default for Fog {
    fn default() -> Fog {
        Fog {
            size: (0, 0),
            grids: BTreeMap::new(),
            masks: (0..=MOST).map(mask).collect(),
            circles: (0..=MOST).map(circle).collect(),
            height: Vec::new(),
            modifiers: Vec::new(),
            enabled: true,
            mask_enabled: true,
        }
    }
}

/// The cells a fog modifier or `SetFogState...` covers: round a point
/// with a radius, or a rectangle (min x, min y, max x, max y), reals as
/// bit patterns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    Radius { x: u32, y: u32, radius: u32 },
    Rect([u32; 4]),
}

/// A fog modifier of a script (SPEC-0032, "Fog modifiers and fog states
/// from scripts").
///
/// PROVISIONAL: a running modifier sets its cells to its state at every
/// update, before the units' reveals or, with `afterUnits`, after them,
/// and at once when it starts; with `useSharedVision` it acts for the
/// players its player shares vision with as well; a rectangle covers the
/// cells from that of its lower left corner to that of its upper right
/// one (SPEC-0032 open question 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Modifier {
    player: i32,
    state: u8,
    area: Area,
    shared: bool,
    after_units: bool,
    running: bool,
}

impl Fog {
    pub fn new(size: (u32, u32)) -> Fog {
        Fog {
            size,
            ..Fog::default()
        }
    }

    /// The cells across and up.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// The state of cell (`i`, `j`) for `player`.
    pub fn state(&self, player: i32, i: u32, j: u32) -> u8 {
        let (w, h) = self.size;
        if i >= w || j >= h {
            return MASKED;
        }
        self.grids
            .get(&player)
            .map_or(MASKED, |g| g[(j * w + i) as usize])
    }

    /// The cells of `player`, row by row from the bottom.
    pub fn grid(&self, player: i32) -> Option<&[u8]> {
        self.grids.get(&player).map(Vec::as_slice)
    }
}

/// `S(0)` to `S(r)`: the half widths of the circle of radius `r` by row,
/// from Bresenham's midpoint circle (SPEC-0032, "The circle").
pub fn half_widths(r: i32) -> Vec<i32> {
    let mut s = vec![0; r.max(0) as usize + 1];
    let (mut x, mut y, mut e) = (0, r, 3 - 2 * r);
    while x <= y {
        for (a, b) in [(x, y), (y, x)] {
            if let Some(w) = s.get_mut(b as usize) {
                *w = (*w).max(a);
            }
        }
        if e < 0 {
            e += 4 * x + 6;
        } else {
            e += 4 * (x - y) + 10;
            y -= 1;
        }
        x += 1;
    }
    s
}

/// The ring of a cell: how many steps out from the centre it is.
fn ring((dx, dy): (i32, i32)) -> i32 {
    dx.abs().max(dy.abs())
}

/// Cells ordered by ring and then by row and column, so that parents come
/// before their children.
fn ordered(cells: BTreeSet<(i32, i32)>) -> Vec<(i32, i32)> {
    let mut cells: Vec<(i32, i32)> = cells.into_iter().collect();
    cells.sort_by_key(|&(dx, dy)| (ring((dx, dy)), dy, dx));
    cells
}

/// The mask of a ground unit of radius `r` (SPEC-0032, "The mask of a
/// ground unit"). PROVISIONAL: radius 0 has the 3 x 3 cells of radius 1.
pub fn mask(r: i32) -> Vec<(i32, i32)> {
    let mut cells = BTreeSet::new();
    for dy in -1..=1 {
        for dx in -1..=1 {
            cells.insert((dx, dy));
        }
    }
    if r == 2 {
        cells.extend([(2, 0), (0, 2), (-2, 0), (0, -2)]);
    } else if r >= 3 {
        let s = half_widths(r);
        for k in 1..=r {
            let l = k.min(s[k as usize]);
            let side = (0..=l).map(|b| (k, b)).chain((1..l).map(|b| (k, -b)));
            for (mut dx, mut dy) in side {
                for _ in 0..4 {
                    cells.insert((dx, dy));
                    (dx, dy) = (-dy, dx);
                }
            }
        }
    }
    ordered(cells)
}

/// The circle a flying unit of radius `r` reveals.
pub fn circle(r: i32) -> Vec<(i32, i32)> {
    let s = half_widths(r);
    let mut cells = BTreeSet::new();
    for dy in -r..=r {
        let w = s[dy.unsigned_abs() as usize];
        for dx in -w..=w {
            cells.insert((dx, dy));
        }
    }
    ordered(cells)
}

/// The two parents of a cell outside the 3 x 3 centre, one ring closer
/// to the unit (SPEC-0032, "Blocking: heights and parents").
pub fn parents((dx, dy): (i32, i32)) -> [(i32, i32); 2] {
    let (sx, sy) = (dx.signum(), dy.signum());
    if dx == 0 || dy == 0 || dx.abs() == dy.abs() {
        let p = (dx - sx, dy - sy);
        [p, p]
    } else if dx.abs() > dy.abs() {
        [(dx - sx, dy), (dx - sx, dy - sy)]
    } else {
        [(dx, dy - sy), (dx - sx, dy - sy)]
    }
}

/// The radius in cells of a sight (a real): divided by 128, rounded to
/// the nearest with halves up, at most 14.
pub fn radius(sight: u32) -> i32 {
    r2i(floor(add(mul(sight, PER_CELL), HALF))).clamp(0, MOST)
}

/// The height of each cell: 128 per cliff level, and the occlusion of a
/// destructable standing in it.
fn heights(state: &State) -> Vec<i32> {
    let (w, h) = state.fog.size;
    let mut height = vec![0i32; (w * h) as usize];
    for j in 0..h {
        for i in 0..w {
            height[(j * w + i) as usize] = 128 * i32::from(state.heights.cell_layer(i, j));
        }
    }
    let mut occlusion = vec![0i32; (w * h) as usize];
    for d in &state.destructables {
        let Some(Object::Destructable(d)) = state.objects.get(d) else {
            continue;
        };
        let Some(&occ) = state.data.occlusion.get(&d.type_id) else {
            continue;
        };
        if let (true, Some((i, j))) = (d.alive, state.heights.cell_of(d.x, d.y)) {
            let o = &mut occlusion[(j * w + i) as usize];
            *o = (*o).max(f32::from_bits(occ) as i32);
        }
    }
    for (h, o) in height.iter_mut().zip(&occlusion) {
        *h += o;
    }
    height
}

/// The fog update of this tick, if there is one: in the ticks `k` with
/// `k - 1` a multiple of 80. Visible cells turn fogged, then the running
/// modifiers set their cells, before and after the units' reveals.
pub(super) fn tick(state: &mut State) {
    if state.ticks % UPDATE_TICKS != PHASE {
        return;
    }
    let (w, h) = state.fog.size;
    if w == 0 || h == 0 {
        return;
    }
    state.fog.height = heights(state);
    for grid in state.fog.grids.values_mut() {
        for c in grid.iter_mut() {
            if *c == VISIBLE {
                *c = FOGGED;
            }
        }
    }
    apply_modifiers(state, false);
    for i in 0..state.units.len() {
        let u = state.units[i];
        reveal(state, u);
    }
    apply_modifiers(state, true);
}

/// The running modifiers that come before the units' reveals, or after.
fn apply_modifiers(state: &mut State, after_units: bool) {
    for i in 0..state.fog.modifiers.len() {
        let m = match state.objects.get(&state.fog.modifiers[i]) {
            Some(Object::FogModifier(m)) if m.running && m.after_units == after_units => m.clone(),
            _ => continue,
        };
        set_area(state, m.player, m.shared, m.state, m.area);
    }
}

/// A unit made reveals at once (SPEC-0032, "Updates").
pub(super) fn born(state: &mut State, unit: u32) {
    let (w, h) = state.fog.size;
    if w == 0 || h == 0 {
        return;
    }
    if state.fog.height.is_empty() {
        state.fog.height = heights(state);
    }
    reveal(state, unit);
}

/// Sets `cells` to `value` for `players`.
fn set_cells(state: &mut State, players: &[i32], cells: &[usize], value: u8) {
    let (w, h) = state.fog.size;
    for &p in players {
        let grid = state
            .fog
            .grids
            .entry(p)
            .or_insert_with(|| vec![MASKED; (w * h) as usize]);
        for &c in cells {
            grid[c] = value;
        }
    }
}

/// The cells `unit` reveals to its owner and those it shares vision
/// with, with the cell heights of the last update.
fn reveal(state: &mut State, unit: u32) {
    let (w, h) = state.fog.size;
    let Some(Object::Unit(unit)) = state.objects.get(&unit) else {
        return;
    };
    if !unit.alive || unit.hidden {
        return;
    }
    let Some(data) = state.data.units.get(&unit.type_id) else {
        return;
    };
    let Some((ci, cj)) = state.heights.cell_of(unit.x, unit.y) else {
        return;
    };
    let owner = unit.owner;
    let r = radius(if state.night {
        data.night_sight
    } else {
        data.sight
    });
    let fog = &state.fog;
    let height = &fog.height;
    let at = |dx: i32, dy: i32| {
        let (i, j) = (ci as i32 + dx, cj as i32 + dy);
        (i >= 0 && j >= 0 && i < w as i32 && j < h as i32)
            .then(|| (j as u32 * w + i as u32) as usize)
    };
    let mut seen = Vec::new();
    if data.flying {
        seen.extend(
            fog.circles[r as usize]
                .iter()
                .filter_map(|&(dx, dy)| at(dx, dy)),
        );
    } else {
        let eye = height[(cj * w + ci) as usize];
        let side = 2 * r + 3;
        let index = |(dx, dy): (i32, i32)| ((dy + r + 1) * side + dx + r + 1) as usize;
        let mut hidden = vec![false; (side * side) as usize];
        for &(dx, dy) in &fog.masks[r as usize] {
            let Some(c) = at(dx, dy) else {
                // Off the map: skipped, and so are the cells behind it.
                hidden[index((dx, dy))] = true;
                continue;
            };
            if ring((dx, dy)) > 1 {
                let blocked = parents((dx, dy))
                    .into_iter()
                    .any(|p| hidden[index(p)] || at(p.0, p.1).is_some_and(|pc| height[pc] > eye));
                if blocked {
                    hidden[index((dx, dy))] = true;
                    continue;
                }
            }
            seen.push(c);
        }
    }
    let players = state.setup.vision_receivers(owner);
    set_cells(state, &players, &seen, VISIBLE);
}

/// The cells of `area` (SPEC-0032, "Fog modifiers and fog states from
/// scripts"): round a point, `r = floor(radius / 128)`: none for 0, the
/// 2 x 2 cells below and left of the point's cell for 1, those and four
/// cells two away on the axes for 2, else the rows `-r .. r - 1` of the
/// circle; or the cells of a rectangle.
pub fn area_cells(heights: &crate::heights::Heights, size: (u32, u32), area: Area) -> Vec<usize> {
    let (w, h) = (size.0 as i32, size.1 as i32);
    let mut cells = Vec::new();
    let mut add = |i: i32, j: i32| {
        if i >= 0 && j >= 0 && i < w && j < h {
            cells.push((j * w + i) as usize);
        }
    };
    match area {
        Area::Radius { x, y, radius } => {
            let (ci, cj) = heights.cell_index(x, y);
            let r = r2i(floor(mul(radius, PER_CELL))).min(MOST);
            match r {
                ..=0 => {}
                1 | 2 => {
                    for (i, j) in [(-1, -1), (0, -1), (-1, 0), (0, 0)] {
                        add(ci + i, cj + j);
                    }
                    if r == 2 {
                        for (i, j) in [(-2, 0), (2, 0), (0, -2), (0, 2)] {
                            add(ci + i, cj + j);
                        }
                    }
                }
                _ => {
                    let s = half_widths(r);
                    for dy in -r..r {
                        let half = s[dy.unsigned_abs() as usize];
                        for dx in -half..=half {
                            add(ci + dx, cj + dy);
                        }
                    }
                }
            }
        }
        Area::Rect([x0, y0, x1, y1]) => {
            let (i0, j0) = heights.cell_index(x0, y0);
            let (i1, j1) = heights.cell_index(x1, y1);
            for j in j0.max(0)..=j1.min(h - 1) {
                for i in i0.max(0)..=i1.min(w - 1) {
                    add(i, j);
                }
            }
        }
    }
    cells
}

/// Sets the cells of `area` to `value` for `player`, and with `shared`
/// for those it shares vision with.
fn set_area(state: &mut State, player: i32, shared: bool, value: u8, area: Area) {
    let cells = area_cells(&state.heights, state.fog.size, area);
    let players = if shared {
        state.setup.vision_receivers(player)
    } else {
        vec![player]
    };
    set_cells(state, &players, &cells, value);
}

/// The state of a `fogstate` (`FOG_OF_WAR_MASKED` 1, `FOGGED` 2,
/// `VISIBLE` 4).
fn fog_state(value: &Value) -> Option<u8> {
    match handle(value)? {
        1 => Some(MASKED),
        2 => Some(FOGGED),
        4 => Some(VISIBLE),
        _ => None,
    }
}

fn rect(state: &State, value: &Value) -> Option<[u32; 4]> {
    match state.objects.get(&handle(value)?)? {
        Object::Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        } => Some([*min_x, *min_y, *max_x, *max_y]),
        _ => None,
    }
}

fn player(state: &State, value: &Value) -> Option<i32> {
    state.player_id(handle(value))
}

/// The state of the cell of point (`x`, `y`) for the player of `p`.
fn point_state(state: &State, x: u32, y: u32, p: &Value) -> u8 {
    match (player(state, p), state.heights.cell_of(x, y)) {
        (Some(p), Some((i, j))) => state.fog.state(p, i, j),
        _ => MASKED,
    }
}

fn location(state: &State, value: &Value) -> Option<(u32, u32)> {
    match state.objects.get(&handle(value)?)? {
        Object::Location { x, y } => Some((*x, *y)),
        _ => None,
    }
}

#[allow(unused_braces)]
pub(super) fn register(add: &mut impl FnMut(&str, NativeFn), state: &Rc<RefCell<State>>) {
    for (name, wanted) in [
        ("IsVisibleToPlayer", VISIBLE),
        ("IsFoggedToPlayer", FOGGED),
        ("IsMaskedToPlayer", MASKED),
    ] {
        let declaration =
            format!("native {name} takes real x, real y, player whichPlayer returns boolean");
        native!(add, state, &declaration, |vm, state, args| {
            let state = state.borrow();
            let s = point_state(&state, real(&args[0]), real(&args[1]), &args[2]);
            Ok(Some(Value::Boolean(s == wanted)))
        });
        let declaration = format!(
            "native IsLocation{} takes location whichLocation, player whichPlayer returns boolean",
            name.trim_start_matches("Is")
        );
        native!(add, state, &declaration, |vm, state, args| {
            let state = state.borrow();
            let s = location(&state, &args[0])
                .map_or(MASKED, |(x, y)| point_state(&state, x, y, &args[1]));
            Ok(Some(Value::Boolean(s == wanted)))
        });
    }
    for (name, wanted) in [
        ("IsUnitVisible", VISIBLE),
        ("IsUnitFogged", FOGGED),
        ("IsUnitMasked", MASKED),
    ] {
        let declaration =
            format!("native {name} takes unit whichUnit, player whichPlayer returns boolean");
        native!(add, state, &declaration, |vm, state, args| {
            let state = state.borrow();
            let s = state
                .unit(handle(&args[0]))
                .map_or(MASKED, |u| point_state(&state, u.x, u.y, &args[1]));
            Ok(Some(Value::Boolean(s == wanted)))
        });
    }
    native!(
        add,
        state,
        "native FogEnable takes boolean enable returns nothing",
        |vm, state, args| {
            state.borrow_mut().fog.enabled = args[0] == Value::Boolean(true);
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native IsFogEnabled takes nothing returns boolean",
        |vm, state, args| { Ok(Some(Value::Boolean(state.borrow().fog.enabled))) }
    );
    native!(
        add,
        state,
        "native FogMaskEnable takes boolean enable returns nothing",
        |vm, state, args| {
            state.borrow_mut().fog.mask_enabled = args[0] == Value::Boolean(true);
            Ok(None)
        }
    );
    native!(
        add,
        state,
        "native IsFogMaskEnabled takes nothing returns boolean",
        |vm, state, args| { Ok(Some(Value::Boolean(state.borrow().fog.mask_enabled))) }
    );

    // Fog states set once, and fog modifiers (SPEC-0032).
    // `SetFogStateRect` and `SetFogStateRadius(Loc)`: the area's
    // arguments start at index 2, the last argument is `useSharedVision`.
    type AreaOf = fn(&State, &[Value]) -> Option<Area>;
    let radius_at = |_: &State, args: &[Value]| -> Option<Area> {
        Some(Area::Radius {
            x: real(&args[2]),
            y: real(&args[3]),
            radius: real(&args[4]),
        })
    };
    let radius_loc = |state: &State, args: &[Value]| -> Option<Area> {
        let (x, y) = location(state, &args[2])?;
        Some(Area::Radius {
            x,
            y,
            radius: real(&args[3]),
        })
    };
    let rect_area =
        |state: &State, args: &[Value]| -> Option<Area> { rect(state, &args[2]).map(Area::Rect) };
    let kinds: [(&str, &str, usize, AreaOf); 3] = [
        ("Rect", "rect where", 3, rect_area),
        (
            "Radius",
            "real centerx, real centerY, real radius",
            5,
            radius_at,
        ),
        ("RadiusLoc", "location center, real radius", 4, radius_loc),
    ];
    for (suffix, params, shared_at, area_of) in kinds {
        let declaration = format!(
            "native SetFogState{suffix} takes player forWhichPlayer, fogstate whichState, {params}, boolean useSharedVision returns nothing"
        );
        native!(add, state, &declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let (Some(p), Some(value), Some(area)) = (
                player(&state, &args[0]),
                fog_state(&args[1]),
                area_of(&state, args),
            ) else {
                return Ok(None);
            };
            let shared = args[shared_at] == Value::Boolean(true);
            set_area(&mut state, p, shared, value, area);
            Ok(None)
        });
        let declaration = format!(
            "native CreateFogModifier{suffix} takes player forWhichPlayer, fogstate whichState, {params}, boolean useSharedVision, boolean afterUnits returns fogmodifier"
        );
        native!(add, state, &declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let (Some(p), Some(value), Some(area)) = (
                player(&state, &args[0]),
                fog_state(&args[1]),
                area_of(&state, args),
            ) else {
                return Ok(Some(Value::Handle(None)));
            };
            let h = state.create(Object::FogModifier(Modifier {
                player: p,
                state: value,
                area,
                shared: args[shared_at] == Value::Boolean(true),
                after_units: args[shared_at + 1] == Value::Boolean(true),
                running: false,
            }));
            state.fog.modifiers.push(h);
            Ok(Some(Value::Handle(Some(h))))
        });
    }
    for (name, running) in [("FogModifierStart", true), ("FogModifierStop", false)] {
        let declaration =
            format!("native {name} takes fogmodifier whichFogModifier returns nothing");
        native!(add, state, &declaration, |vm, state, args| {
            let mut state = state.borrow_mut();
            let Some(Object::FogModifier(m)) =
                handle(&args[0]).and_then(|h| state.objects.get_mut(&h))
            else {
                return Ok(None);
            };
            m.running = running;
            let m = m.clone();
            if running {
                set_area(&mut state, m.player, m.shared, m.state, m.area);
            }
            Ok(None)
        });
    }
    native!(
        add,
        state,
        "native DestroyFogModifier takes fogmodifier whichFogModifier returns nothing",
        |vm, state, args| {
            let mut state = state.borrow_mut();
            if let Some(h) = handle(&args[0])
                && matches!(state.objects.get(&h), Some(Object::FogModifier(_)))
            {
                state.objects.remove(&h);
                state.fog.modifiers.retain(|&m| m != h);
            }
            Ok(None)
        }
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_widths_of_the_spec() {
        assert_eq!(half_widths(3), [3, 3, 2, 1]);
        assert_eq!(half_widths(6), [6, 6, 6, 5, 4, 3, 2]);
        assert_eq!(half_widths(8), [8, 8, 8, 7, 7, 6, 5, 4, 2]);
        assert_eq!(half_widths(11), [11, 11, 11, 11, 10, 10, 9, 8, 8, 6, 5, 3]);
        assert_eq!(
            half_widths(14),
            [14, 14, 14, 14, 13, 13, 13, 12, 11, 11, 10, 9, 7, 6, 3]
        );
    }

    #[test]
    fn radii_of_the_spec() {
        for (sight, r) in [
            (350.0f32, 3),
            (600.0, 5),
            (800.0, 6),
            (900.0, 7),
            (1000.0, 8),
            (1100.0, 9),
            (1200.0, 9),
            (1400.0, 11),
            (1600.0, 13),
            (1800.0, 14),
            (2200.0, 14),
        ] {
            assert_eq!(radius(sight.to_bits()), r, "{sight}");
        }
    }

    /// The masks match `SPEC-0032-masks.tsv` row by row (each row one run
    /// of cells), and radius 6 has 121 cells.
    #[test]
    fn masks_of_the_table() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../specs/SPEC-0032-masks.tsv"
        );
        let table = std::fs::read_to_string(path).unwrap();
        // Per radius: the count of cells and the runs (dy, first, last).
        type Runs = (usize, Vec<(i32, i32, i32)>);
        let mut rows: BTreeMap<i32, Runs> = BTreeMap::new();
        for line in table.lines().filter(|l| !l.starts_with('#')) {
            let v: Vec<i32> = line.split('\t').map(|f| f.parse().unwrap()).collect();
            let entry = rows.entry(v[0]).or_default();
            entry.0 = v[1] as usize;
            entry.1.push((v[2], v[3], v[4]));
        }
        assert_eq!(rows.len(), 14);
        for (r, (count, runs)) in rows {
            let m = mask(r);
            assert_eq!(m.len(), count, "radius {r}");
            for (dy, lo, hi) in runs {
                let mut row: Vec<i32> = m.iter().filter(|c| c.1 == dy).map(|c| c.0).collect();
                row.sort_unstable();
                let run: Vec<i32> = (lo..=hi).collect();
                assert_eq!(row, run, "radius {r} row {dy}");
            }
        }
        assert_eq!(mask(6).len(), 121);
    }

    const PEASANT: i32 = 1;
    const FLYER: i32 = 2;
    const TREE: i32 = 3;

    /// A world of 32 x 32 cells from (0, 0) with the cliff level of each
    /// point, a ground unit and a flying one of sight 800 (radius 6) and
    /// trees of `occH` 230.
    fn world(layer: impl Fn(u32, u32) -> u8) -> State {
        use ow3_formats::binary::RawCode;
        use ow3_formats::map::terrain::{Terrain, TerrainPoint};
        let terrain = Terrain {
            format_version: 11,
            tileset: b'L',
            custom_tilesets: false,
            ground_tilesets: vec![RawCode(*b"Ldrt")],
            cliff_tilesets: Vec::new(),
            width: 33,
            height: 33,
            origin_x: 0.0,
            origin_y: 0.0,
            points: (0..33 * 33)
                .map(|p| TerrainPoint {
                    ground_height: 8192,
                    water_and_edge: 0,
                    flags_and_texture: 0,
                    texture_details: 0,
                    cliff_and_layer: layer(p % 33, p / 33),
                })
                .collect(),
        };
        let mut state = State::new();
        state.heights = crate::heights::Heights::new(&terrain);
        state.fog = Fog::new(state.heights.cells());
        for (id, flying) in [(PEASANT, false), (FLYER, true)] {
            let unit = crate::data::UnitType {
                hp: 100f32.to_bits(),
                sight: 800f32.to_bits(),
                night_sight: 600f32.to_bits(),
                flying,
                ..Default::default()
            };
            state.data.units.insert(id, unit);
        }
        state.data.occlusion.insert(TREE, 230f32.to_bits());
        state
    }

    /// A point inside cell (`i`, `j`), off its centre.
    fn inside(i: u32, j: u32) -> [u32; 3] {
        [(i * 128 + 104) as f32, (j * 128 + 84) as f32, 0.0].map(f32::to_bits)
    }

    fn unit(state: &mut State, type_id: i32, i: u32, j: u32) -> u32 {
        super::super::widgets::create_unit(state, 0, type_id, inside(i, j)).unwrap()
    }

    fn tree(state: &mut State, i: u32, j: u32) {
        let [x, y, _] = inside(i, j);
        let h = state.create(Object::Destructable(super::super::Destructable {
            type_id: TREE,
            x,
            y,
            life: 1,
            max_life: 1,
            alive: true,
            facing: 0,
            footprint: Default::default(),
            hit_at: None,
            died_at: None,
        }));
        state.destructables.push(h);
    }

    fn update(state: &mut State) {
        state.ticks = (state.ticks / UPDATE_TICKS + 1) * UPDATE_TICKS + PHASE;
        tick(state);
    }

    /// The cells of player 0 in `wanted`, relative to (`i`, `j`).
    fn cells(state: &State, wanted: u8, (i, j): (i32, i32)) -> BTreeSet<(i32, i32)> {
        let (w, h) = state.fog.size();
        let mut out = BTreeSet::new();
        for b in 0..h {
            for a in 0..w {
                if state.fog.state(0, a, b) == wanted {
                    out.insert((a as i32 - i, b as i32 - j));
                }
            }
        }
        out
    }

    /// Verification 1 (RVV): on flat ground the 121 cells of the radius 6
    /// mask, wherever the unit stands inside its cell.
    #[test]
    fn a_peasant_on_flat_ground() {
        let mut state = world(|_, _| 2);
        unit(&mut state, PEASANT, 16, 16);
        // It sees as soon as it is made, and the same at the update.
        let made = cells(&state, VISIBLE, (16, 16));
        update(&mut state);
        let seen = cells(&state, VISIBLE, (16, 16));
        assert_eq!(made, seen);
        assert_eq!(seen, mask(6).into_iter().collect());
        assert_eq!(seen.len(), 121);
    }

    /// A tree wall 3 cells east: the trees are seen, the cells behind them
    /// are not (verification 3).
    #[test]
    fn trees_hide_what_is_behind_them() {
        let mut state = world(|_, _| 2);
        for j in 14..=18 {
            tree(&mut state, 19, j);
        }
        unit(&mut state, PEASANT, 16, 16);
        update(&mut state);
        let seen = cells(&state, VISIBLE, (16, 16));
        for dy in -2..=2 {
            assert!(seen.contains(&(3, dy)), "the tree at dy {dy}");
            assert!(!seen.contains(&(4, dy)), "behind the tree at dy {dy}");
        }
        assert!(seen.contains(&(-6, 0)) && seen.contains(&(0, 6)));
    }

    /// A block raised by one cliff level from x 19 on: from below its edge
    /// is seen and nothing behind it; from on top all of the mask, the
    /// low ground included.
    #[test]
    fn a_cliff_hides_the_high_ground() {
        let mut state = world(|i, _| if i >= 19 { 3 } else { 2 });
        unit(&mut state, PEASANT, 16, 16);
        update(&mut state);
        let seen = cells(&state, VISIBLE, (16, 16));
        // Cell 18 has corners on the block: the edge.
        assert!(seen.contains(&(2, 0)));
        assert!(!seen.contains(&(3, 0)) && !seen.contains(&(5, 0)));
        assert!(seen.len() < 121);

        let mut state = world(|i, _| if i >= 19 { 3 } else { 2 });
        unit(&mut state, PEASANT, 25, 16);
        update(&mut state);
        assert_eq!(cells(&state, VISIBLE, (25, 16)).len(), 121);
    }

    /// Updates come only every 80 ticks; the cells left behind become
    /// fogged (verification 4).
    #[test]
    fn cells_left_behind_are_fogged() {
        let mut state = world(|_, _| 2);
        let h = unit(&mut state, PEASANT, 10, 16);
        update(&mut state);
        let [x, _, _] = inside(20, 16);
        if let Some(Object::Unit(u)) = state.objects.get_mut(&h) {
            u.x = x;
        }
        // A tick that is not an update changes nothing.
        state.ticks += 1;
        tick(&mut state);
        assert_eq!(cells(&state, VISIBLE, (10, 16)).len(), 121);
        update(&mut state);
        assert_eq!(
            cells(&state, VISIBLE, (20, 16)),
            mask(6).into_iter().collect()
        );
        let fogged = cells(&state, FOGGED, (10, 16));
        assert!(fogged.contains(&(-6, 0)) && !fogged.contains(&(6, 0)));
        assert_eq!(state.fog.state(0, 31, 31), MASKED);
    }

    /// A flying unit sees its whole circle over the trees.
    #[test]
    fn a_flying_unit_sees_its_circle() {
        let mut state = world(|_, _| 2);
        for j in 14..=18 {
            tree(&mut state, 19, j);
        }
        unit(&mut state, FLYER, 16, 16);
        update(&mut state);
        let seen = cells(&state, VISIBLE, (16, 16));
        assert_eq!(seen, circle(6).into_iter().collect());
    }

    /// The cells of a modifier round a point: none below 128, 2 x 2 below
    /// and left of the point's cell, the axes two away for 2, and for a
    /// radius of 800 the rows 6 below to 5 above (SPEC-0032).
    #[test]
    fn modifier_cells() {
        let state = world(|_, _| 2);
        let size = state.fog.size();
        let cells = |radius: f32| -> BTreeSet<(i32, i32)> {
            let [x, y, _] = inside(16, 16);
            let area = Area::Radius {
                x,
                y,
                radius: radius.to_bits(),
            };
            area_cells(&state.heights, size, area)
                .into_iter()
                .map(|c| {
                    (
                        (c as u32 % size.0) as i32 - 16,
                        (c as u32 / size.0) as i32 - 16,
                    )
                })
                .collect()
        };
        assert!(cells(127.0).is_empty());
        let two: BTreeSet<_> = [(-1, -1), (0, -1), (-1, 0), (0, 0)].into();
        assert_eq!(cells(192.0), two);
        assert_eq!(cells(300.0).len(), 8);
        let c = cells(800.0);
        let rows: BTreeSet<i32> = c.iter().map(|p| p.1).collect();
        assert_eq!(rows, (-6..=5).collect());
        let s = half_widths(6);
        for dy in -6..=5 {
            let n = c.iter().filter(|p| p.1 == dy).count() as i32;
            assert_eq!(n, 2 * s[dy.unsigned_abs() as usize] + 1);
        }
    }

    /// A running modifier keeps its cells visible at each update; a state
    /// set once fades at the next one.
    #[test]
    fn modifiers_and_states() {
        let mut state = world(|_, _| 2);
        let [x, y, _] = inside(10, 10);
        let area = Area::Radius {
            x,
            y,
            radius: 800f32.to_bits(),
        };
        set_area(&mut state, 0, false, VISIBLE, area);
        assert_eq!(state.fog.state(0, 10, 10), VISIBLE);
        update(&mut state);
        assert_eq!(state.fog.state(0, 10, 10), FOGGED);
        let h = state.create(Object::FogModifier(Modifier {
            player: 0,
            state: VISIBLE,
            area,
            shared: false,
            after_units: true,
            running: true,
        }));
        state.fog.modifiers.push(h);
        update(&mut state);
        update(&mut state);
        assert_eq!(state.fog.state(0, 10, 10), VISIBLE);
        assert_eq!(state.fog.state(0, 10, 15), VISIBLE);
        // The top row is not the modifier's, and was never seen.
        assert_eq!(state.fog.state(0, 10, 16), MASKED);
    }

    /// A player sees with another's units only with its shared vision.
    #[test]
    fn shared_vision() {
        let mut state = world(|_, _| 2);
        super::super::widgets::create_unit(&mut state, 1, PEASANT, inside(16, 16)).unwrap();
        update(&mut state);
        assert_eq!(state.fog.state(0, 16, 16), MASKED);
        state.setup.alliances.insert((1, 0, 5));
        update(&mut state);
        assert_eq!(cells(&state, VISIBLE, (16, 16)).len(), 121);
    }

    /// Parents are one ring closer, and lie in the mask.
    #[test]
    fn parents_are_in_the_mask() {
        for r in 3..=MOST {
            let m = mask(r);
            for &c in m.iter().filter(|&&c| ring(c) > 1) {
                for p in parents(c) {
                    assert_eq!(ring(p), ring(c) - 1, "{c:?}");
                    assert!(m.contains(&p), "radius {r}: {p:?} of {c:?}");
                }
            }
        }
    }
}
