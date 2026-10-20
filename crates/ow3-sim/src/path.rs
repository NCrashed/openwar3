//! Path finding around the static obstacles of the pathing map
//! (SPEC-0020, part 1).
//!
//! Points are in cells of 32 world units from the lower left corner of
//! the map, as bit patterns of reals; blocks are 2 x 2 cells. A local
//! search finds a path of cell centres with A*, a high-level search a list
//! of waypoints over a quadtree of free blocks. Both use one binary heap
//! with the game's rules for equal keys.
//!
//! PROVISIONAL: the iteration limit of the high-level search is 400 (open
//! question 1); a start block that is not free gives no list.

use std::collections::HashMap;

use ow3_math::{add, div, i2r, mul, sub};

/// A real as its bit pattern.
fn real(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// The floor of a real, as an integer.
fn floor(bits: u32) -> i32 {
    real(bits).floor() as i32
}

const HALF: u32 = 0x3F00_0000;

/// Which cells a ground unit may stand on: the pathing map, with the
/// footprints of buildings and destructables on top (SPEC-0022).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    width: i32,
    height: i32,
    /// Cells of the pathing map without 0x02 (walkable) and without 0x04
    /// (flyable).
    walk: Vec<bool>,
    fly: Vec<bool>,
    /// Cells of the pathing map without 0x08 (buildable, SPEC-0026).
    build: Vec<bool>,
    /// How many footprints block each cell for walking (red) and flying
    /// (green).
    red: Vec<u16>,
    green: Vec<u16>,
    /// Block states of levels 0 to 3.
    levels: [Level; 4],
}

/// The states of the squares of one level of the quadtree.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Level {
    /// Squares along x and y, the last one inclusive.
    columns: i32,
    rows: i32,
    states: Vec<State>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Free,
    Blocked,
    Mixed,
}

/// The state of a square from those of its four quarters.
fn combined(children: [State; 4]) -> State {
    let blocked = children.iter().filter(|&&s| s == State::Blocked).count();
    if children.contains(&State::Mixed) || (1..=3).contains(&blocked) {
        State::Mixed
    } else if blocked == 4 {
        State::Blocked
    } else {
        State::Free
    }
}

impl Grid {
    /// The grid of a `war3map.wpm` (SPEC-0020): 0x02 closes a cell for
    /// walking, 0x04 for flying, which the high-level search also heeds.
    pub fn new(pathing: &ow3_formats::map::pathing::Pathing) -> Grid {
        let walk = pathing.cells.iter().map(|&v| v & 0x02 == 0).collect();
        let fly = pathing.cells.iter().map(|&v| v & 0x04 == 0).collect();
        let mut grid = Grid::from_layers(pathing.width as i32, pathing.height as i32, walk, fly);
        grid.build = pathing.cells.iter().map(|&v| v & 0x08 == 0).collect();
        grid
    }

    /// A grid of `width` x `height` walkable or closed cells, row by row
    /// from the bottom.
    pub fn from_cells(width: i32, height: i32, passable: Vec<bool>) -> Grid {
        let fly = vec![true; passable.len()];
        Grid::from_layers(width, height, passable, fly)
    }

    fn from_layers(width: i32, height: i32, walk: Vec<bool>, fly: Vec<bool>) -> Grid {
        let cells = walk.len();
        let mut grid = Grid {
            width,
            height,
            walk,
            fly,
            build: vec![true; cells],
            red: vec![0; cells],
            green: vec![0; cells],
            levels: [(); 4].map(|_| Level {
                columns: 0,
                rows: 0,
                states: Vec::new(),
            }),
        };
        // Level 0 up to the block beyond the edge, then the larger squares.
        for l in 0..4 {
            let (columns, rows) = (((width / 2) >> l) + 1, ((height / 2) >> l) + 1);
            grid.levels[l as usize] = Level {
                columns,
                rows,
                states: vec![State::Free; (columns * rows) as usize],
            };
            for v in 0..rows {
                for u in 0..columns {
                    let state = grid.computed(l, u, v);
                    grid.levels[l as usize].states[(v * columns + u) as usize] = state;
                }
            }
        }
        grid
    }

    /// The state of square (`u`, `v`) of level `l` from the cells or from
    /// the squares of the level below.
    fn computed(&self, l: i32, u: i32, v: i32) -> State {
        if l == 0 {
            let blocked = [(0, 0), (1, 0), (0, 1), (1, 1)]
                .iter()
                .filter(|(dx, dy)| !self.quad_free(2 * u + dx, 2 * v + dy))
                .count();
            match blocked {
                0 => State::Free,
                4 => State::Blocked,
                _ => State::Mixed,
            }
        } else {
            combined(
                [(0, 0), (1, 0), (0, 1), (1, 1)]
                    .map(|(du, dv)| self.state(l - 1, 2 * u + du, 2 * v + dv)),
            )
        }
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        ((0..self.width).contains(&x) && (0..self.height).contains(&y))
            .then(|| (y * self.width + x) as usize)
    }

    /// Whether cell (`x`, `y`) is walkable: by the pathing map and no red
    /// footprint (the local search, seeing and the look-ahead check).
    /// Cells outside are not.
    pub fn passable(&self, x: i32, y: i32) -> bool {
        self.index(x, y)
            .is_some_and(|i| self.walk[i] && self.red[i] == 0)
    }

    /// Whether a building may stand on cell (`x`, `y`) by the pathing map
    /// (no 0x08, SPEC-0026). Cells outside may not.
    pub fn buildable(&self, x: i32, y: i32) -> bool {
        self.index(x, y).is_some_and(|i| self.build[i])
    }

    /// Whether cell (`x`, `y`) is free for the high-level search: also
    /// flyable and without a green footprint.
    fn quad_free(&self, x: i32, y: i32) -> bool {
        self.index(x, y)
            .is_some_and(|i| self.walk[i] && self.fly[i] && self.red[i] == 0 && self.green[i] == 0)
    }

    /// Adds (`add`) or removes the footprint with cells `red` and `green`
    /// (SPEC-0022), and brings the quadtree up to date.
    pub fn stamp(&mut self, red: &[(i32, i32)], green: &[(i32, i32)], add: bool) {
        let mut blocks = std::collections::BTreeSet::new();
        for (cells, counts) in [(red, 0), (green, 1)] {
            for &(x, y) in cells {
                let Some(i) = self.index(x, y) else {
                    continue;
                };
                let count = if counts == 0 {
                    &mut self.red[i]
                } else {
                    &mut self.green[i]
                };
                *count = if add {
                    count.saturating_add(1)
                } else {
                    count.saturating_sub(1)
                };
                blocks.insert((x >> 1, y >> 1));
            }
        }
        for l in 0..4 {
            let squares: std::collections::BTreeSet<(i32, i32)> =
                blocks.iter().map(|&(u, v)| (u >> l, v >> l)).collect();
            for (u, v) in squares {
                let level = &self.levels[l as usize];
                if u >= level.columns || v >= level.rows {
                    continue;
                }
                let state = self.computed(l, u, v);
                let columns = self.levels[l as usize].columns;
                self.levels[l as usize].states[(v * columns + u) as usize] = state;
            }
        }
    }

    /// The state of square (`u`, `v`) of level `l` (in squares of that
    /// level). `None` below 0; squares beyond the stored ones are free.
    fn state_at(&self, l: i32, u: i32, v: i32) -> Option<State> {
        if u < 0 || v < 0 {
            return None;
        }
        let level = &self.levels[l as usize];
        if u >= level.columns || v >= level.rows {
            return Some(State::Free);
        }
        Some(level.states[(v * level.columns + u) as usize])
    }

    fn state(&self, l: i32, u: i32, v: i32) -> State {
        self.state_at(l, u, v).unwrap_or(State::Blocked)
    }

    /// The state of the level-`l` square holding block (`u`, `v`).
    fn holding(&self, l: i32, u: i32, v: i32) -> Option<State> {
        if u < 0 || v < 0 {
            return None;
        }
        self.state_at(l, u >> l, v >> l)
    }

    /// `F(u, v)`: whether block (`u`, `v`) is free.
    fn block_free(&self, u: i32, v: i32) -> bool {
        self.holding(0, u, v) == Some(State::Free)
    }
}

/// The size class of a unit of collision size `collision`.
pub fn size_class(collision: u32) -> u8 {
    let r = real(div(collision, 0x4200_0000));
    if r < 0.5 {
        0
    } else if r < 1.0 {
        1
    } else if r < 1.5 {
        2
    } else {
        3
    }
}

/// The cells of the footprint of class `class` at cell `c`: x and y
/// ranges, inclusive.
fn footprint(c: (i32, i32), class: u8) -> ((i32, i32), (i32, i32)) {
    let (low, high) = match class {
        0 => (0, 0),
        1 => (1, 0),
        2 => (1, 1),
        _ => (2, 1),
    };
    ((c.0 - low, c.0 + high), (c.1 - low, c.1 + high))
}

fn covers(f: ((i32, i32), (i32, i32)), x: i32, y: i32) -> bool {
    (f.0.0..=f.0.1).contains(&x) && (f.1.0..=f.1.1).contains(&y)
}

/// The cells the footprint at `q` covers and the one at `p` does not (all
/// of them when `p` is `None`), in rows from the bottom.
fn newly_covered(p: Option<(i32, i32)>, q: (i32, i32), class: u8) -> Vec<(i32, i32)> {
    let old = p.map(|p| footprint(p, class));
    let new = footprint(q, class);
    let mut cells = Vec::new();
    for y in new.1.0..=new.1.1 {
        for x in new.0.0..=new.0.1 {
            if !old.is_some_and(|o| covers(o, x, y)) {
                cells.push((x, y));
            }
        }
    }
    cells
}

/// The cells a step of a unit of class `class` from cell `p` to the
/// neighbouring cell `q` checks: the ones it newly covers, and on a
/// diagonal also the ones of the two corner cells (no cutting corners).
pub fn step_cells(p: (i32, i32), q: (i32, i32), class: u8) -> Vec<(i32, i32)> {
    let mut cells = newly_covered(Some(p), q, class);
    if p.0 != q.0 && p.1 != q.1 {
        cells.extend(newly_covered(Some(p), (q.0, p.1), class));
        cells.extend(newly_covered(Some(p), (p.0, q.1), class));
    }
    cells
}

/// The cells of the look-ahead check of a unit at `s` walking to `target`
/// (cells): those of the step to the cell one cell ahead, or the whole
/// footprint when that is the unit's own cell.
pub fn ahead_cells(s: [u32; 2], target: [u32; 2], class: u8) -> Vec<(i32, i32)> {
    let d = [sub(target[0], s[0]), sub(target[1], s[1])];
    let len = real(add(mul(d[0], d[0]), mul(d[1], d[1]))).sqrt();
    let ahead = if len > 1.0 {
        let step = d.map(|c| div(c, len.to_bits()));
        [add(s[0], step[0]), add(s[1], step[1])]
    } else {
        target
    };
    let (c0, c1) = (cell_of(s), cell_of(ahead));
    if c0 == c1 {
        newly_covered(None, c1, class)
    } else {
        step_cells(c0, c1, class)
    }
}

/// Cells a unit may or may not stand on: the pathing map, possibly with
/// cells that units occupy (SPEC-0021).
pub trait Cells {
    /// Whether cell (`x`, `y`) is free; cells outside the map are not.
    fn free(&self, x: i32, y: i32) -> bool;
    /// Whether cell (`x`, `y`) is passable on the pathing map alone,
    /// whatever units stand on it.
    fn passable_alone(&self, x: i32, y: i32) -> bool {
        self.free(x, y)
    }
    /// The size of the map in cells.
    fn size(&self) -> (i32, i32);

    /// Whether a unit of class `class` may step from cell `p` to the
    /// neighbouring cell `q`, not cutting corners on a diagonal.
    fn step(&self, p: (i32, i32), q: (i32, i32), class: u8) -> bool {
        step_cells(p, q, class)
            .into_iter()
            .all(|(x, y)| self.free(x, y))
    }

    /// Whether point `p` can be seen from `s` (cells) by a unit of class
    /// `class`: the cells sampled along the line one cell apart change only
    /// by allowed steps, the first one checked as a step by (+1, +1) from
    /// the cell before it.
    fn sees(&self, s: [u32; 2], p: [u32; 2], class: u8) -> bool {
        let d = [sub(p[0], s[0]), sub(p[1], s[1])];
        let len = real(add(mul(d[0], d[0]), mul(d[1], d[1]))).sqrt();
        let direction = if len > 1.0 {
            d.map(|c| div(c, len.to_bits()))
        } else {
            d
        };
        let mut previous = (0, 0);
        let mut t = 1;
        while (t as f32) < len {
            let at = [
                add(s[0], mul(i2r(t), direction[0])),
                add(s[1], mul(i2r(t), direction[1])),
            ];
            let cell = cell_of(at);
            if cell != previous {
                let step = (
                    (cell.0 - previous.0).signum(),
                    (cell.1 - previous.1).signum(),
                );
                let from = (cell.0 - step.0, cell.1 - step.1);
                if !self.step(from, cell, class) {
                    return false;
                }
                previous = cell;
            }
            t += 1;
        }
        true
    }

    /// The look-ahead check: the cells of `ahead_cells` are free.
    fn ahead_free(&self, s: [u32; 2], target: [u32; 2], class: u8) -> bool {
        ahead_cells(s, target, class)
            .into_iter()
            .all(|(x, y)| self.free(x, y))
    }
}

impl Cells for Grid {
    fn free(&self, x: i32, y: i32) -> bool {
        self.passable(x, y)
    }

    fn size(&self) -> (i32, i32) {
        (self.width, self.height)
    }
}

/// The pathing map with the cells some units occupy (SPEC-0021).
pub struct WithUnits<'a> {
    pub grid: &'a Grid,
    pub occupied: &'a std::collections::HashSet<(i32, i32)>,
}

impl Cells for WithUnits<'_> {
    fn free(&self, x: i32, y: i32) -> bool {
        self.grid.passable(x, y) && !self.occupied.contains(&(x, y))
    }

    fn passable_alone(&self, x: i32, y: i32) -> bool {
        self.grid.passable(x, y)
    }

    fn size(&self) -> (i32, i32) {
        self.grid.size()
    }
}

/// The cells a unit of class `class` occupies when it stands at `p`
/// (cells): its footprint at the cell of `p` (SPEC-0021).
pub fn occupied_cells(p: [u32; 2], class: u8) -> Vec<(i32, i32)> {
    newly_covered(None, cell_of(p), class)
}

/// The open list of both searches: a binary heap in an array from index
/// 1, with the game's rules for equal keys (SPEC-0020).
struct Heap<T> {
    entries: Vec<(u32, T)>,
}

impl<T: Copy> Heap<T> {
    fn new() -> Heap<T> {
        Heap {
            entries: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Up past every parent whose key is greater than or equal to it.
    fn push(&mut self, key: u32, value: T) {
        self.entries.push((key, value));
        let mut i = self.entries.len();
        while i > 1 && self.entries[i / 2 - 1].0 >= key {
            self.entries.swap(i - 1, i / 2 - 1);
            i /= 2;
        }
    }

    /// The entry at the top; the last one sinks from the top, to the right
    /// child when its key is not above the left one's, and stops when its
    /// key is not above the chosen child's.
    fn pop(&mut self) -> Option<(u32, T)> {
        let last = self.entries.pop()?;
        if self.entries.is_empty() {
            return Some(last);
        }
        let top = self.entries[0];
        let n = self.entries.len();
        let mut i = 1;
        loop {
            let left = 2 * i;
            if left > n {
                break;
            }
            let mut child = left;
            if left < n && self.entries[left].0 <= self.entries[left - 1].0 {
                child = left + 1;
            }
            if last.0 <= self.entries[child - 1].0 {
                break;
            }
            self.entries[i - 1] = self.entries[child - 1];
            i = child;
        }
        self.entries[i - 1] = last;
        Some(top)
    }
}

/// The estimate of SPEC-0020 from cell (`x`, `y`) to (`gx`, `gy`).
fn estimate(x: i32, y: i32, gx: i32, gy: i32) -> u32 {
    let a = 15u32.wrapping_mul(x.abs_diff(gx));
    let b = 15u32.wrapping_mul(y.abs_diff(gy));
    let (big, mut small) = (a.max(b), a.min(b));
    if small > big >> 2 {
        small = small.wrapping_add(small >> 1);
        big.wrapping_add(big >> 6)
            .wrapping_add(small >> 2)
            .wrapping_add(small >> 7)
            .wrapping_sub(big >> 4)
    } else {
        big
    }
}

/// The neighbours of a cell in the order they are tried.
const NEIGHBOURS: [(i32, i32); 8] = [
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
];

/// A node of a search: its cost, its parent and whether it is closed.
#[derive(Clone, Copy, Debug)]
struct Node<K> {
    g: u32,
    parent: Option<K>,
    closed: bool,
}

/// What a search found.
#[derive(Clone, Debug, PartialEq)]
pub struct Found {
    /// The points from the start to the end, in cells (local) or blocks
    /// (high level).
    pub points: Vec<[u32; 2]>,
    /// Whether the goal was reached; otherwise the path ends at the node
    /// closest to it.
    pub reached: bool,
    pub iterations: u32,
    /// Whether the search tested no cell that was not passable (local
    /// search only; the unit then walks straight to the end, SPEC-0020).
    pub clear: bool,
}

/// The cell of a point.
pub fn cell_of(p: [u32; 2]) -> (i32, i32) {
    (floor(p[0]), floor(p[1]))
}

fn centre(c: (i32, i32)) -> [u32; 2] {
    [add(i2r(c.0), HALF), add(i2r(c.1), HALF)]
}

/// The local search from `s` to `g` (cells) for a unit of class `class`,
/// with at most `limit` iterations.
pub fn local(grid: &impl Cells, s: [u32; 2], g: [u32; 2], class: u8, limit: u32) -> Found {
    search(grid, s, g, class, limit, None)
}

/// The local search of a chase (SPEC-0019): to `g`, but ending at the
/// first expanded cell one of whose steps to its neighbours tests a cell
/// of the target's footprint `target` (a cell passable on the pathing
/// map), at that cell's centre.
pub fn local_touching(
    grid: &impl Cells,
    s: [u32; 2],
    g: [u32; 2],
    class: u8,
    limit: u32,
    target: &[(i32, i32)],
) -> Found {
    search(grid, s, g, class, limit, Some(target))
}

fn search(
    grid: &impl Cells,
    s: [u32; 2],
    g: [u32; 2],
    class: u8,
    limit: u32,
    target: Option<&[(i32, i32)]>,
) -> Found {
    // Whether a step from `c` to `n` tests a cell of the target.
    let touches = |c: (i32, i32), n: (i32, i32)| {
        target.is_some_and(|t| {
            step_cells(c, n, class)
                .into_iter()
                .any(|(x, y)| grid.passable_alone(x, y) && t.contains(&(x, y)))
        })
    };
    let (start, goal) = (cell_of(s), cell_of(g));
    if target.is_none() && start == goal {
        return Found {
            points: vec![s, g],
            reached: true,
            iterations: 0,
            clear: true,
        };
    }
    let mut clear = true;
    let mut nodes: HashMap<(i32, i32), Node<(i32, i32)>> = HashMap::new();
    let mut heap = Heap::new();
    let distance = |c: (i32, i32)| {
        let (dx, dy) = (i64::from(c.0 - goal.0), i64::from(c.1 - goal.1));
        dx * dx + dy * dy
    };
    nodes.insert(
        start,
        Node {
            g: 0,
            parent: None,
            closed: false,
        },
    );
    heap.push(estimate(start.0, start.1, goal.0, goal.1), (start, 0u32));
    let (mut closest, mut best) = (start, distance(start));
    let mut iterations = 0;
    let mut reached = false;
    let mut end_near = None;
    while let Some((_, (c, g_then))) = heap.pop() {
        iterations += 1;
        let node = nodes[&c];
        // An entry left behind when the cell got a cheaper cost.
        if node.g == g_then && !node.closed {
            nodes.get_mut(&c).unwrap().closed = true;
            if c == goal {
                reached = true;
                break;
            }
            for (dx, dy) in NEIGHBOURS {
                let n = (c.0 + dx, c.1 + dy);
                let (width, height) = grid.size();
                if !(0..width).contains(&n.0) || !(0..height).contains(&n.1) {
                    continue;
                }
                // A chase touching its target ends here (SPEC-0019).
                if touches(c, n) {
                    end_near = Some(c);
                    break;
                }
                if !grid.step(c, n, class) {
                    clear = false;
                    continue;
                }
                let cost = node.g + if dx != 0 && dy != 0 { 21 } else { 15 };
                match nodes.get_mut(&n) {
                    None => {
                        nodes.insert(
                            n,
                            Node {
                                g: cost,
                                parent: Some(c),
                                closed: false,
                            },
                        );
                        if distance(n) < best {
                            (closest, best) = (n, distance(n));
                        }
                    }
                    Some(other) if cost < other.g => {
                        *other = Node {
                            g: cost,
                            parent: Some(c),
                            closed: false,
                        };
                    }
                    Some(_) => continue,
                }
                let key = cost.wrapping_add(estimate(n.0, n.1, goal.0, goal.1));
                heap.push(key, (n, cost));
            }
        }
        if end_near.is_some() {
            break;
        }
        if iterations >= limit || heap.is_empty() {
            break;
        }
    }
    let end = match end_near {
        Some(c) => {
            // PROVISIONAL: SPEC-0019 counts such a path as clear; that was
            // seen on open ground, and a chase round a wall would walk into
            // it, so here only a search that met nothing in the way is.
            reached = true;
            c
        }
        None if reached => goal,
        None => closest,
    };
    let mut cells = vec![end];
    while let Some(parent) = nodes[cells.last().unwrap()].parent {
        cells.push(parent);
    }
    cells.reverse();
    let last = cells.len() - 1;
    let points = cells
        .iter()
        .enumerate()
        .map(|(i, &c)| match i {
            0 => s,
            _ if i == last && reached && end_near.is_none() => g,
            _ => centre(c),
        })
        .collect();
    Found {
        points,
        reached,
        iterations,
        clear,
    }
}

/// A node of the high-level search: the level and the square of that
/// level.
type Square = (i32, i32, i32);

/// The iteration limit of the high-level search (open question 1).
pub const HIGH_LEVEL_LIMIT: u32 = 400;

/// A high-level search in progress.
struct HighLevel<'a> {
    grid: &'a Grid,
    /// 1 for size classes 0 and 1, 2 for 2 and 3.
    granularity: i32,
    goal: (i32, i32),
    nodes: HashMap<Square, (Node<Square>, (i32, i32))>,
    /// The position of every node found so far.
    positions: HashMap<Square, (i32, i32)>,
    heap: Heap<(Square, u32)>,
    closest: (Square, i64),
}

/// The cost between nodes at blocks `a` and `b`.
fn block_cost(a: (i32, i32), b: (i32, i32)) -> u32 {
    let (du, dv) = (
        24 * i64::from(a.0.abs_diff(b.0)),
        24 * i64::from(a.1.abs_diff(b.1)),
    );
    ((du * du + dv * dv) as u64).isqrt() as u32
}

/// Sides of a square, in the order they are scanned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    Bottom,
    Right,
    Top,
    Left,
}

impl Side {
    /// Whether the side runs along x.
    fn along_x(self) -> bool {
        matches!(self, Side::Bottom | Side::Top)
    }
}

impl<'a> HighLevel<'a> {
    /// The node found from block `q` at level `l`: the free square holding
    /// it, climbed while the next larger square is free. The first query
    /// that finds a node gives its position.
    fn node_from(&mut self, q: (i32, i32), l: i32) -> Option<Square> {
        if self.grid.holding(l, q.0, q.1)? != State::Free {
            return None;
        }
        let mut level = l;
        while level < 3 && self.grid.holding(level + 1, q.0, q.1) == Some(State::Free) {
            level += 1;
        }
        let square = (level, q.0 >> level, q.1 >> level);
        self.positions.entry(square).or_insert(q);
        Some(square)
    }

    fn free(&self, u: i32, v: i32) -> bool {
        self.grid.block_free(u, v)
    }

    /// `C(a, b)` of SPEC-0020.
    fn c(&self, a: i32, b: i32) -> bool {
        self.free(a + 1, b) && self.free(a, b + 1) && self.free(a + 1, b + 1)
    }

    /// The position of a node: the block of its first query.
    fn position(&self, n: Square) -> (i32, i32) {
        self.positions[&n]
    }

    /// Relaxes node `n`, found from block `q`, as a neighbour of `from`.
    fn relax(&mut self, from: Square, n: Square, q: (i32, i32)) {
        let base = self.nodes[&from].0.g;
        let from_position = self.position(from);
        let position = self.position(n);
        let cost = base + block_cost(from_position, position);
        match self.nodes.get_mut(&n) {
            None => {
                self.nodes.insert(
                    n,
                    (
                        Node {
                            g: cost,
                            parent: Some(from),
                            closed: false,
                        },
                        q,
                    ),
                );
                let (du, dv) = (i64::from(q.0 - self.goal.0), i64::from(q.1 - self.goal.1));
                if du * du + dv * dv < self.closest.1 {
                    self.closest = (n, du * du + dv * dv);
                }
            }
            Some((node, _)) if cost < node.g => {
                *node = Node {
                    g: cost,
                    parent: Some(from),
                    closed: false,
                };
            }
            Some(_) => return,
        }
        let key = cost.wrapping_add(block_cost(position, self.goal));
        self.heap.push(key, (n, cost));
    }

    /// The neighbours of a node whose square is a single block.
    fn block_neighbours(&mut self, n: Square) {
        let (x, y) = self.position(n);
        let gran2 = self.granularity == 2;
        let mut accepted = [false; 4];
        let sides = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        for (i, (dx, dy)) in sides.into_iter().enumerate() {
            let q = (x + dx, y + dy);
            let Some(found) = self.node_from(q, 0) else {
                continue;
            };
            let above = found.0 > 0;
            let ok = !gran2
                || match i {
                    0 => {
                        self.free(x + 1, y - 1)
                            && (!(x % 2 != 0 && above)
                                || self.free(x + 1, y - 3)
                                || self.free(x - 1, y - 1))
                    }
                    1 => self.free(x + 2, y) && self.free(x + 2, y + 1),
                    2 => self.free(x, y + 2) && self.free(x + 1, y + 2),
                    _ => {
                        self.free(x - 1, y + 1)
                            && (!(y % 2 != 0 && above)
                                || self.free(x - 3, y + 1)
                                || self.free(x - 1, y - 1))
                    }
                };
            if ok {
                accepted[i] = true;
                self.relax(n, found, q);
            }
        }
        // SE, NE, NW, SW, after both of their sides.
        let diagonals = [
            ((1, -1), (0, 1), gran2.then(|| self.free(x + 2, y - 1))),
            ((1, 1), (1, 2), gran2.then(|| self.free(x + 2, y + 2))),
            ((-1, 1), (2, 3), gran2.then(|| self.free(x - 1, y + 2))),
            ((-1, -1), (3, 0), None),
        ];
        for ((dx, dy), (a, b), extra) in diagonals {
            if !(accepted[a] && accepted[b]) || extra == Some(false) {
                continue;
            }
            let q = (x + dx, y + dy);
            if let Some(found) = self.node_from(q, 0) {
                self.relax(n, found, q);
            }
        }
    }

    /// The gran 2 check of a side scan's query `q` against `e`, the last
    /// block of the free square along the side.
    fn side_check(&self, side: Side, q: (i32, i32), at_end: bool) -> bool {
        let (qx, qy) = q;
        match (side, at_end) {
            (Side::Bottom, false) => self.free(qx + 1, qy),
            (Side::Bottom, true) => {
                self.free(qx + 1, qy)
                    && self.free(qx + 1, qy + 1)
                    && (self.free(qx + 1, qy + 2) || self.free(qx - 1, qy))
            }
            (Side::Right, false) => self.c(qx, qy),
            (Side::Right, true) => {
                self.c(qx, qy)
                    && ((self.free(qx, qy - 1) && self.free(qx + 1, qy - 1))
                        || (self.free(qx - 2, qy + 1) && self.free(qx - 1, qy + 1))
                        || (self.free(qx - 1, qy + 1) && self.free(qx, qy - 1)))
            }
            (Side::Top, false) => self.c(qx, qy),
            (Side::Top, true) => {
                self.c(qx, qy)
                    && ((self.free(qx - 1, qy) && self.free(qx - 1, qy + 1))
                        || (self.free(qx + 1, qy - 2) && self.free(qx + 1, qy - 1))
                        || (self.free(qx + 1, qy - 1) && self.free(qx - 1, qy)))
            }
            (Side::Left, false) => self.free(qx, qy + 1),
            (Side::Left, true) => {
                self.free(qx, qy + 1)
                    && self.free(qx + 1, qy + 1)
                    && (self.free(qx + 2, qy + 1) || self.free(qx, qy - 1))
            }
        }
    }

    /// Scans `side` of node `n` at level `l` from query block `q`, the
    /// piece being at the low and/or high end of the side, and sets the
    /// end flags its free squares give in `flags`.
    fn scan(
        &mut self,
        n: Square,
        side: Side,
        l: i32,
        q: (i32, i32),
        (low_end, high_end): (bool, bool),
        flags: &mut [bool; 2],
    ) {
        let Some(state) = self.grid.holding(l, q.0, q.1) else {
            return;
        };
        match state {
            State::Blocked => {}
            State::Mixed if l == 0 => {}
            State::Mixed => {
                let along = |b: (i32, i32)| if side.along_x() { b.0 } else { b.1 };
                let with = |b: (i32, i32), a: i32| if side.along_x() { (a, b.1) } else { (b.0, a) };
                let first = (along(q) >> l) << l;
                let m = first + (1 << (l - 1));
                if along(q) >= m {
                    let lower = if l - 1 == 0 {
                        m - 1
                    } else {
                        m - self.granularity
                    };
                    self.scan(n, side, l - 1, with(q, lower), (low_end, false), flags);
                    self.scan(n, side, l - 1, q, (false, high_end), flags);
                } else {
                    self.scan(n, side, l - 1, q, (low_end, false), flags);
                    self.scan(n, side, l - 1, with(q, m), (false, high_end), flags);
                }
            }
            State::Free => {
                let Some(found) = self.node_from(q, l) else {
                    return;
                };
                if self.granularity == 2 {
                    let along = if side.along_x() { q.0 } else { q.1 };
                    let e = ((along >> l) << l) + (1 << l) - 1;
                    if !self.side_check(side, q, along >= e) {
                        return;
                    }
                }
                self.relax(n, found, q);
                if l > 0 {
                    flags[0] |= low_end;
                    flags[1] |= high_end;
                }
            }
        }
    }

    /// The neighbours of a node above level 0.
    fn square_neighbours(&mut self, n: Square) {
        let (l, sx, sy) = n;
        let size = 1 << l;
        let (x0, y0) = (sx << l, sy << l);
        let (mut x, mut y) = self.position(n);
        if self.granularity == 2 {
            x = x.min(x0 + size - 2);
            y = y.min(y0 + size - 2);
        }
        let mut ends = [[false; 2]; 4];
        let sides = [
            (Side::Bottom, (x, y0 - 1)),
            (Side::Right, (x0 + size, y)),
            (Side::Top, (x, y0 + size)),
            (Side::Left, (x0 - 1, y)),
        ];
        for (i, (side, q)) in sides.into_iter().enumerate() {
            let mut flags = [false; 2];
            self.scan(n, side, l, q, (true, true), &mut flags);
            ends[i] = flags;
        }
        let gran2 = self.granularity == 2;
        // (query, needs, gran 2 check)
        let corners = [
            ((x0 + size, y0 - 1), ends[0][1] && ends[1][0], 0),
            ((x0 + size, y0 + size), ends[1][1] && ends[2][1], 1),
            ((x0 - 1, y0 + size), ends[2][0] && ends[3][1], 2),
            ((x0 - 1, y0 - 1), ends[3][0] && ends[0][0], 3),
        ];
        for (q, needs, kind) in corners {
            if !needs {
                continue;
            }
            if gran2 {
                let ok = match kind {
                    0 => self.free(q.0 + 1, q.1),
                    1 => self.c(q.0, q.1),
                    2 => self.free(q.0, q.1 + 1),
                    _ => true,
                };
                if !ok {
                    continue;
                }
            }
            let mut level = l;
            loop {
                match self.grid.holding(level, q.0, q.1) {
                    Some(State::Free) => {
                        if let Some(found) = self.node_from(q, level) {
                            self.relax(n, found, q);
                        }
                        break;
                    }
                    Some(State::Mixed) if level > 0 => level -= 1,
                    _ => break,
                }
            }
        }
    }
}

/// The high-level search from `s` to `g` (cells) for a unit of class
/// `class`: waypoints in blocks from the start to the end, or `None` when
/// the start block is not free.
pub fn high_level(grid: &Grid, s: [u32; 2], g: [u32; 2], class: u8) -> Option<Found> {
    let block = |p: [u32; 2]| (floor(p[0]) >> 1, floor(p[1]) >> 1);
    let to_blocks = |p: [u32; 2]| p.map(|c| mul(c, HALF));
    let (sb, gb) = (block(s), block(g));
    let granularity = if class >= 2 { 2 } else { 1 };
    let mut search = HighLevel {
        grid,
        granularity,
        goal: gb,
        nodes: HashMap::new(),
        positions: HashMap::new(),
        heap: Heap::new(),
        closest: ((0, 0, 0), i64::MAX),
    };
    // A start block that is not free gives way to the free neighbouring
    // block nearest to the start (SPEC-0019, a chase next to a wall).
    // PROVISIONAL: nearest by the centre of the block, the first of the
    // neighbours in their search order on a tie (open question 6).
    let sb = if search.grid.holding(0, sb.0, sb.1) == Some(State::Free) {
        sb
    } else {
        let distance = |b: (i32, i32)| {
            let c = [i2r(2 * b.0 + 1), i2r(2 * b.1 + 1)];
            let (dx, dy) = (sub(c[0], s[0]), sub(c[1], s[1]));
            real(add(mul(dx, dx), mul(dy, dy)))
        };
        NEIGHBOURS
            .iter()
            .map(|(dx, dy)| (sb.0 + dx, sb.1 + dy))
            .filter(|&(u, v)| search.grid.holding(0, u, v) == Some(State::Free))
            .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))?
    };
    let start = search.node_from(sb, 0)?;
    let goal = search.node_from(gb, 0);
    if goal == Some(start) {
        return Some(Found {
            points: vec![to_blocks(s), to_blocks(g)],
            reached: true,
            iterations: 0,
            clear: false,
        });
    }
    search.nodes.insert(
        start,
        (
            Node {
                g: 0,
                parent: None,
                closed: false,
            },
            sb,
        ),
    );
    let (du, dv) = (i64::from(sb.0 - gb.0), i64::from(sb.1 - gb.1));
    search.closest = (start, du * du + dv * dv);
    search.heap.push(block_cost(sb, gb), (start, 0u32));
    let mut iterations = 0;
    let mut reached = false;
    while let Some((_, (n, g_then))) = search.heap.pop() {
        iterations += 1;
        let node = search.nodes[&n].0;
        if node.g == g_then && !node.closed {
            search.nodes.get_mut(&n).unwrap().0.closed = true;
            if Some(n) == goal {
                reached = true;
                break;
            }
            if n.0 == 0 {
                search.block_neighbours(n);
            } else {
                search.square_neighbours(n);
            }
        }
        if iterations >= HIGH_LEVEL_LIMIT || search.heap.is_empty() {
            break;
        }
    }
    let end = if reached { goal? } else { search.closest.0 };
    let mut chain = vec![end];
    while let Some(parent) = search.nodes[chain.last().unwrap()].0.parent {
        chain.push(parent);
    }
    chain.reverse();
    let offset = if granularity == 2 {
        0x3FA0_0000
    } else {
        0x3F40_0000
    };
    let last = chain.len() - 1;
    let points = chain
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            if i == 0 {
                return to_blocks(s);
            }
            if i == last && reached {
                return to_blocks(g);
            }
            let (mut px, mut py) = search.position(n);
            let (l, sx, sy) = n;
            if granularity == 2 && l > 0 {
                let edge = |s: i32| (s << l) + (1 << l) - 1;
                if px == edge(sx) {
                    px -= 1;
                }
                if py == edge(sy) {
                    py -= 1;
                }
            }
            [add(i2r(px), offset), add(i2r(py), offset)]
        })
        .collect();
    Some(Found {
        points,
        reached,
        iterations,
        clear: false,
    })
}

/// The length of the leg from `a` to `b`, in their units.
pub fn length(a: [u32; 2], b: [u32; 2]) -> u32 {
    let (dx, dy) = (sub(b[0], a[0]), sub(b[1], a[1]));
    real(add(mul(dx, dx), mul(dy, dy))).sqrt().to_bits()
}

/// Points in blocks as points in cells.
pub fn blocks_to_cells(p: [u32; 2]) -> [u32; 2] {
    p.map(|c| mul(c, 0x4000_0000))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32) -> u32 {
        x.to_bits()
    }

    /// A 64 x 64 grid with a wall of cells x = 30, y = 22 .. 38.
    fn walled() -> Grid {
        let mut cells = vec![true; 64 * 64];
        for y in 22..38 {
            cells[y * 64 + 30] = false;
        }
        Grid::from_cells(64, 64, cells)
    }

    #[test]
    fn classes_and_footprints() {
        assert_eq!(
            [8.0f32, 16.0, 31.0, 32.0, 48.0, 0.0].map(|c| size_class(c.to_bits())),
            [0, 1, 1, 2, 3, 0]
        );
        assert_eq!(footprint((5, 5), 3), ((3, 6), (3, 6)));
    }

    #[test]
    fn heap_ties() {
        let mut h = Heap::new();
        for (k, v) in [(5, 'a'), (3, 'b'), (5, 'c'), (3, 'd'), (4, 'e')] {
            h.push(k, v);
        }
        let mut out = Vec::new();
        while let Some((k, _)) = h.pop() {
            out.push(k);
        }
        assert_eq!(out, [3, 3, 4, 5, 5]);
    }

    #[test]
    fn estimate_formula() {
        assert_eq!(estimate(0, 0, 10, 0), 150);
        // a = b = 150: small 225, h = 150 + 2 + 56 + 1 - 9.
        assert_eq!(estimate(0, 0, 10, 10), 200);
    }

    /// The local path goes round the wall and ends at the goal.
    #[test]
    fn local_path_round_a_wall() {
        let grid = walled();
        let found = local(&grid, [r(25.3), r(30.2)], [r(36.7), r(30.4)], 1, 700);
        assert!(found.reached, "{} iterations", found.iterations);
        assert_eq!(found.points[0], [r(25.3), r(30.2)]);
        assert_eq!(*found.points.last().unwrap(), [r(36.7), r(30.4)]);
        for w in found.points.windows(2) {
            let (a, b) = (cell_of(w[0]), cell_of(w[1]));
            assert!(grid.step(a, b, 1), "{a:?} -> {b:?}");
        }
        assert!(
            found
                .points
                .iter()
                .any(|p| real(p[1]) < 22.0 || real(p[1]) > 38.0)
        );
    }

    #[test]
    fn seeing_through_and_past_walls() {
        let grid = walled();
        assert!(!grid.sees([r(25.5), r(30.5)], [r(35.5), r(30.5)], 1));
        assert!(grid.sees([r(25.5), r(5.5)], [r(35.5), r(5.5)], 1));
    }

    #[test]
    fn high_level_round_a_wall() {
        let grid = walled();
        let found = high_level(&grid, [r(25.3), r(30.2)], [r(36.7), r(30.4)], 1).unwrap();
        assert!(found.reached, "{found:?}");
        assert!(found.points.len() > 2);
    }
}
