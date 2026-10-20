//! The local player's selection and control groups (SPEC-0028,
//! "Selection").
//!
//! At most 12 units are selected. They form one subgroup per unit type,
//! ordered by the type's `prio` (higher first); the active subgroup's
//! first unit leads: its command card and portrait show, and Tab moves to
//! the next subgroup. Keys 1 to 9 and 0 are the control groups 0 to 9.
//!
//! PROVISIONAL: which 12 of more units a rectangle takes is open (SPEC-0028
//! question 4); here the 12 made first. Subgroups of equal `prio` keep the
//! order in which their first units were selected.

/// The most units selected at once.
pub const MOST: usize = 12;

/// What the selection needs to know of a unit: its type and the `prio`
/// of the type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Kind {
    pub type_id: i32,
    pub prio: i32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// In the order they were selected.
    units: Vec<u32>,
    /// The index of the active subgroup.
    active: usize,
    groups: [Vec<u32>; 10],
}

impl Selection {
    pub fn units(&self) -> &[u32] {
        &self.units
    }

    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Whether `h` is selected.
    pub fn contains(&self, h: u32) -> bool {
        self.units.contains(&h)
    }

    /// The selection is `units` (at most 12, the first ones kept), its
    /// first subgroup active.
    pub fn set(&mut self, units: &[u32]) {
        self.units = Vec::new();
        for &u in units {
            if !self.units.contains(&u) && self.units.len() < MOST {
                self.units.push(u);
            }
        }
        self.active = 0;
    }

    /// A click on unit `h`: it alone is selected.
    pub fn click(&mut self, h: u32) {
        self.set(&[h]);
    }

    /// Shift and a click on `h`: added, or taken out if it is selected.
    pub fn shift_click(&mut self, h: u32) {
        if self.contains(h) {
            self.units.retain(|&u| u != h);
            self.active = 0;
        } else if self.units.len() < MOST {
            self.units.push(h);
        }
    }

    /// Units added to the selection, as many as fit.
    pub fn add(&mut self, units: &[u32]) {
        for &u in units {
            if !self.units.contains(&u) && self.units.len() < MOST {
                self.units.push(u);
            }
        }
    }

    /// Forgets the units that `keep` says are gone.
    pub fn retain(&mut self, keep: impl Fn(u32) -> bool) {
        self.units.retain(|&u| keep(u));
        for group in &mut self.groups {
            group.retain(|&u| keep(u));
        }
    }

    /// The subgroups: the units of each type, the types by `prio` from the
    /// highest.
    pub fn subgroups(&self, kind: impl Fn(u32) -> Option<Kind>) -> Vec<(Kind, Vec<u32>)> {
        let mut groups: Vec<(Kind, Vec<u32>)> = Vec::new();
        for &u in &self.units {
            let Some(k) = kind(u) else {
                continue;
            };
            match groups.iter_mut().find(|(g, _)| g.type_id == k.type_id) {
                Some((_, members)) => members.push(u),
                None => groups.push((k, vec![u])),
            }
        }
        // A stable sort: equal priorities keep their order.
        groups.sort_by_key(|(k, _)| std::cmp::Reverse(k.prio));
        groups
    }

    /// The units of the active subgroup.
    pub fn active_units(&self, kind: impl Fn(u32) -> Option<Kind>) -> Vec<u32> {
        let groups = self.subgroups(kind);
        let i = self.active.min(groups.len().saturating_sub(1));
        groups
            .into_iter()
            .nth(i)
            .map(|(_, u)| u)
            .unwrap_or_default()
    }

    /// The unit whose card and portrait show: the first of the active
    /// subgroup.
    pub fn lead(&self, kind: impl Fn(u32) -> Option<Kind>) -> Option<u32> {
        self.active_units(kind).first().copied()
    }

    /// Tab: the next subgroup, after the last the first.
    pub fn next_subgroup(&mut self, kind: impl Fn(u32) -> Option<Kind>) {
        let n = self.subgroups(kind).len();
        self.active = if n == 0 { 0 } else { (self.active + 1) % n };
    }

    /// Ctrl and a digit: the selection becomes control group `group`.
    pub fn assign(&mut self, group: usize) {
        if let Some(g) = self.groups.get_mut(group) {
            *g = self.units.clone();
        }
    }

    /// Shift and a digit: the selection is added to group `group`.
    pub fn add_to_group(&mut self, group: usize) {
        if let Some(g) = self.groups.get_mut(group) {
            for &u in &self.units {
                if !g.contains(&u) && g.len() < MOST {
                    g.push(u);
                }
            }
        }
    }

    /// A digit: control group `group` is selected; whether it has units.
    pub fn recall(&mut self, group: usize) -> bool {
        let units = self.groups.get(group).cloned().unwrap_or_default();
        if units.is_empty() {
            return false;
        }
        self.set(&units);
        true
    }

    /// The units of control group `group`.
    pub fn group(&self, group: usize) -> &[u32] {
        self.groups.get(group).map_or(&[], |g| g.as_slice())
    }
}

/// The control group of a digit key: 1 is group 0, ..., 9 is 8, 0 is 9.
pub fn group_of_digit(digit: u32) -> Option<usize> {
    match digit {
        1..=9 => Some(digit as usize - 1),
        0 => Some(9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(h: u32) -> Option<Kind> {
        // 1..=3 footmen (prio 6), 4 a rifleman (2), 5 a knight (3).
        Some(match h {
            1..=3 => Kind {
                type_id: 10,
                prio: 6,
            },
            4 => Kind {
                type_id: 20,
                prio: 2,
            },
            _ => Kind {
                type_id: 30,
                prio: 3,
            },
        })
    }

    #[test]
    fn subgroups_by_priority_and_tab() {
        let mut s = Selection::default();
        s.set(&[4, 1, 5, 2]);
        let groups = s.subgroups(kinds);
        let types: Vec<i32> = groups.iter().map(|(k, _)| k.type_id).collect();
        assert_eq!(types, [10, 30, 20]);
        assert_eq!(s.lead(kinds), Some(1));
        s.next_subgroup(kinds);
        assert_eq!(s.lead(kinds), Some(5));
        s.next_subgroup(kinds);
        s.next_subgroup(kinds);
        assert_eq!(s.lead(kinds), Some(1));
    }

    #[test]
    fn at_most_twelve_and_shift() {
        let mut s = Selection::default();
        s.set(&(1..=20).collect::<Vec<_>>());
        assert_eq!(s.units().len(), MOST);
        s.shift_click(3);
        assert!(!s.contains(3));
        s.shift_click(3);
        assert!(s.contains(3));
    }

    #[test]
    fn control_groups() {
        let mut s = Selection::default();
        s.set(&[1, 2]);
        s.assign(group_of_digit(1).unwrap());
        s.click(4);
        s.add_to_group(0);
        assert_eq!(s.group(0), [1, 2, 4]);
        s.click(5);
        assert!(s.recall(0));
        assert_eq!(s.units(), [1, 2, 4]);
        assert!(!s.recall(group_of_digit(0).unwrap()));
    }
}
