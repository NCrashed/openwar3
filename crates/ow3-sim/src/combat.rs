//! Damage, armor and attack types (SPEC-0015).
//!
//! The constants come from `[Misc]` of a `Units\MiscGame.txt` imported
//! into the map, else of the game's `Units\MiscGame.txt` for melee maps and
//! `Custom_V1\Units\MiscGame.txt` for the others, read with `s2r`. Damage
//! of `UnitDamageTarget` is the amount times the armor factor (for
//! `DAMAGE_TYPE_NORMAL` only) times the multiplier of the attack type
//! against the armor type (1 for armor types outside the lists), and at
//! least the smaller of the amount and 1.
//!
//! PROVISIONAL: game data sets other than the default are not
//! considered. Without the file every multiplier is 1, `DefenseArmor`
//! 0.06 and the agility constants 0.

use ow3_math::{add, div, i2r, mul, r2i, s2r, sub};

const ONE: u32 = 0x3F80_0000;
/// `DAMAGE_TYPE_NORMAL`.
pub const DAMAGE_TYPE_NORMAL: i32 = 4;

/// Armor types in the order of the data lists.
pub const ARMOR_TYPES: [&str; 8] = [
    "small", "medium", "large", "fort", "normal", "hero", "divine", "none",
];

/// The data list of each attack type, by its JASS value.
const ATTACK_LISTS: [&str; 7] = [
    "DamageBonusSpells",
    "DamageBonusNormal",
    "DamageBonusPierce",
    "DamageBonusSiege",
    "DamageBonusMagic",
    "DamageBonusChaos",
    "DamageBonusHero",
];

/// The combat constants of a map.
#[derive(Clone, Debug, PartialEq)]
pub struct Combat {
    pub defense_armor: u32,
    /// Multipliers by attack type, then armor type.
    pub bonus: [[u32; 8]; 7],
    pub agi_defense_base: u32,
    pub agi_defense_bonus: u32,
}

impl Default for Combat {
    fn default() -> Combat {
        Combat {
            defense_armor: s2r("0.06"),
            bonus: [[ONE; 8]; 7],
            agi_defense_base: 0,
            agi_defense_bonus: 0,
        }
    }
}

impl Combat {
    /// The constants of `[Misc]` in a `MiscGame.txt`; missing ones keep
    /// their defaults.
    pub fn parse(text: &str) -> Combat {
        let mut combat = Combat::default();
        let mut in_misc = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_misc = line.eq_ignore_ascii_case("[Misc]");
                continue;
            }
            let Some((key, value)) = line.split_once('=').filter(|_| in_misc) else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            match key {
                "DefenseArmor" => combat.defense_armor = s2r(value),
                "AgiDefenseBase" => combat.agi_defense_base = s2r(value),
                "AgiDefenseBonus" => combat.agi_defense_bonus = s2r(value),
                _ => {
                    if let Some(attack) = ATTACK_LISTS.iter().position(|l| *l == key) {
                        for (slot, v) in combat.bonus[attack].iter_mut().zip(value.split(',')) {
                            *slot = s2r(v.trim());
                        }
                    }
                }
            }
        }
        combat
    }

    /// The constants for a map, melee or not: the map's own
    /// `Units\MiscGame.txt`, else the game's copy. `Custom_V1` is looked up
    /// in the game archives only.
    pub fn load(archives: &mut crate::map::Archives, melee: bool) -> Combat {
        let own = archives.read_map_file("Units\\MiscGame.txt");
        let path = if melee {
            "Units\\MiscGame.txt"
        } else {
            "Custom_V1\\Units\\MiscGame.txt"
        };
        own.or_else(|| archives.read_game_file(path))
            .map(|d| Combat::parse(&String::from_utf8_lossy(&d)))
            .unwrap_or_default()
    }

    /// The armor of a hero with `armor` (`def`) and `agility`.
    pub fn hero_armor(&self, armor: u32, agility: u32) -> u32 {
        add(
            add(armor, self.agi_defense_base),
            mul(self.agi_defense_bonus, agility),
        )
    }

    /// The share of damage that armor `armor` takes away.
    pub fn reduction(&self, armor: u32) -> u32 {
        if f32::from_bits(armor) >= 0.0 {
            let x = mul(armor, self.defense_armor);
            div(x, add(ONE, x))
        } else {
            let n = r2i(armor).unsigned_abs().min(20) as usize;
            NEGATIVE[n]
        }
    }

    /// The damage of `UnitDamageTarget` against armor `armor` of type
    /// `armor_type` (an index into [`ARMOR_TYPES`], or `None` for a type
    /// outside the lists, which takes the multiplier 1).
    pub fn damage(
        &self,
        amount: u32,
        armor: u32,
        armor_type: Option<usize>,
        attack_type: i32,
        damage_type: i32,
    ) -> u32 {
        let multiplier = usize::try_from(attack_type)
            .ok()
            .and_then(|a| self.bonus.get(a))
            .zip(armor_type)
            .and_then(|(list, t)| list.get(t))
            .copied()
            .unwrap_or(ONE);
        let factor = if damage_type == DAMAGE_TYPE_NORMAL {
            sub(ONE, self.reduction(armor))
        } else {
            ONE
        };
        let damage = mul(mul(amount, factor), multiplier);
        let least = if f32::from_bits(amount) < 1.0 {
            amount
        } else {
            ONE
        };
        if f32::from_bits(damage) < f32::from_bits(least) {
            least
        } else {
            damage
        }
    }
}

/// `floor(100000 * (1 - 0.94^n))`, computed exactly: 0.94^n is
/// 94^n / 10^(2n), kept as decimal digits.
fn negative_step(n: u32) -> i32 {
    // Digits of 94^n, least significant first.
    let mut digits = vec![1u32];
    for _ in 0..n {
        let mut carry = 0;
        for d in &mut digits {
            let v = *d * 94 + carry;
            *d = v % 10;
            carry = v / 10;
        }
        while carry > 0 {
            digits.push(carry % 10);
            carry /= 10;
        }
    }
    // 100000 * 94^n / 10^(2n): shift the digits by 2n - 5.
    let shift = 2 * n as i64 - 5;
    let value = |from: usize| -> i64 {
        digits[from.min(digits.len())..]
            .iter()
            .rev()
            .fold(0i64, |v, &d| v * 10 + i64::from(d))
    };
    let scaled_ceil = if shift <= 0 {
        value(0) * 10i64.pow((-shift) as u32)
    } else {
        let s = shift as usize;
        let whole = value(s);
        whole + i64::from(digits[..s.min(digits.len())].iter().any(|&d| d != 0))
    };
    (100_000 - scaled_ceil) as i32
}

/// The reduction of armor `-n`, for `n` up to 20.
static NEGATIVE: std::sync::LazyLock<[u32; 21]> = std::sync::LazyLock::new(|| {
    std::array::from_fn(|n| div(i2r(-negative_step(n as u32)), i2r(100_000)))
});

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(v: f32) -> u32 {
        v.to_bits()
    }

    #[test]
    fn negative_armor_table() {
        let steps: Vec<i32> = (0..4).map(negative_step).collect();
        assert_eq!(steps, [0, 6000, 11640, 16941]);
        assert_eq!(NEGATIVE[0], 0);
        assert_eq!(NEGATIVE[1], div(i2r(-6000), i2r(100_000)));
        // -3.5 counts as -3; below -20 as -20.
        let combat = Combat::default();
        assert_eq!(combat.reduction(bits(-3.5)), NEGATIVE[3]);
        assert_eq!(combat.reduction(bits(-50.0)), NEGATIVE[20]);
        assert!(negative_step(20) > negative_step(19));
    }

    /// The example of SPEC-0015: 100 pierce damage against medium armor
    /// 10 in a custom map.
    #[test]
    fn spec_example() {
        let combat = Combat::parse(
            "[Misc]\nDefenseArmor=0.06\nDamageBonusPierce=2.00,0.75,1.00,0.35,1.00,0.50,0.05,1.50\n",
        );
        assert_eq!(combat.defense_armor, 0x3D75_C290);
        let medium = Some(1);
        let normal = combat.damage(bits(100.0), bits(10.0), medium, 2, DAMAGE_TYPE_NORMAL);
        // 0x423B7FFE: the 0.06 of s2r is a little more than 0.06, so the
        // factor is a little less than 0.625; R2SW prints 46.88.
        assert_eq!(f32::from_bits(normal), 46.874_992);
        let magic = combat.damage(bits(100.0), bits(10.0), medium, 2, 14);
        assert_eq!(f32::from_bits(magic), 75.0);
    }

    #[test]
    fn least_damage() {
        let combat = Combat::default();
        // 1.5 against armor 10 is 1; 0.7 stays 0.7.
        let d = combat.damage(bits(1.5), bits(10.0), Some(1), 1, DAMAGE_TYPE_NORMAL);
        assert_eq!(d, ONE);
        let small = combat.damage(bits(0.7), bits(100.0), Some(1), 1, DAMAGE_TYPE_NORMAL);
        assert_eq!(small, bits(0.7));
    }

    /// An armor type outside the lists (a tree's `Wood`) takes 1 from every
    /// attack type; `none` takes 1.5 from pierce.
    #[test]
    fn armor_type_outside_the_lists() {
        let combat =
            Combat::parse("[Misc]\nDamageBonusPierce=2.00,0.75,1.00,0.35,1.00,0.50,0.05,1.50\n");
        let none = Some(7);
        let pierce = 2;
        let d = |t| combat.damage(bits(10.0), 0, t, pierce, DAMAGE_TYPE_NORMAL);
        assert_eq!(d(None), bits(10.0));
        assert_eq!(d(none), bits(15.0));
    }

    #[test]
    fn hero_armor() {
        let combat = Combat::parse("[Misc]\nAgiDefenseBase=-2\nAgiDefenseBonus=0.30\n");
        // The paladin: 2 - 2 + 0.3 * 13.
        let armor = combat.hero_armor(bits(2.0), bits(13.0));
        assert!((f32::from_bits(armor) - 3.9).abs() < 1e-6);
    }
}
