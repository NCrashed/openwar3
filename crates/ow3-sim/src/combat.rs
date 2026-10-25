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
//! `ReactionDelay` of SPEC-0019 comes from `[Misc]` of `Units\MiscData.txt`.
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
    /// The longest reaction delay of attack orders in seconds
    /// (`ReactionDelay` of `[Misc]` in `Units\MiscData.txt`, SPEC-0019).
    pub reaction_delay: u32,
    /// How far idle units hear a call for help, about this plus 180
    /// (`CallForHelp` of `[Misc]` in `Units\MiscGame.txt`, SPEC-0019).
    pub call_for_help: u32,
    /// Creeps (SPEC-0030): the reach of their call for help, the distance
    /// from home after which a chase can end and the seconds until then
    /// (`CreepCallForHelp`, `GuardDistance`, `GuardReturnTime`).
    pub creep_call_for_help: u32,
    pub guard_distance: u32,
    pub guard_return_time: u32,
    /// Within this of its home a creep's return timer restarts at a call
    /// (`MaxGuardDistance`, SPEC-0030).
    pub max_guard_distance: u32,
    /// The least a slow leaves of a unit's speed (`MinUnitSpeed`,
    /// SPEC-0037).
    pub min_unit_speed: u32,
    /// Hero experience and attributes (SPEC-0033, SPEC-0034).
    pub heroes: Heroes,
}

impl Default for Combat {
    fn default() -> Combat {
        Combat {
            defense_armor: s2r("0.06"),
            bonus: [[ONE; 8]; 7],
            agi_defense_base: 0,
            agi_defense_bonus: 0,
            reaction_delay: s2r("0.25"),
            call_for_help: s2r("600"),
            creep_call_for_help: s2r("600"),
            guard_distance: s2r("600"),
            guard_return_time: s2r("5"),
            max_guard_distance: s2r("1000"),
            min_unit_speed: s2r("150"),
            heroes: Heroes::default(),
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
                "ReactionDelay" => combat.reaction_delay = s2r(value),
                "CallForHelp" => combat.call_for_help = s2r(value),
                "CreepCallForHelp" => combat.creep_call_for_help = s2r(value),
                "GuardDistance" => combat.guard_distance = s2r(value),
                "GuardReturnTime" => combat.guard_return_time = s2r(value),
                "MaxGuardDistance" => combat.max_guard_distance = s2r(value),
                "MinUnitSpeed" => combat.min_unit_speed = s2r(value),
                _ if combat.heroes.set(key, value) => {}
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
        let mut combat = own
            .or_else(|| archives.read_game_file(path))
            .map(|d| Combat::parse(&String::from_utf8_lossy(&d)))
            .unwrap_or_default();
        // PROVISIONAL: `MiscData.txt` is looked up like `MiscGame.txt`,
        // with the game's plain copy last.
        let data = archives
            .read_map_file("Units\\MiscData.txt")
            .or_else(|| {
                (!melee)
                    .then(|| archives.read_game_file("Custom_V1\\Units\\MiscData.txt"))
                    .flatten()
            })
            .or_else(|| archives.read_game_file("Units\\MiscData.txt"));
        if let Some(d) = data {
            combat.reaction_delay = Combat::parse(&String::from_utf8_lossy(&d)).reaction_delay;
        }
        combat
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

/// The amount of one hit of `weapon` (SPEC-0016): its base damage, plus
/// `primary` (a hero's primary attribute, a whole number as a real), plus
/// its dice rolled from `dice` (game generator 2).
pub fn hit_amount(
    weapon: &crate::data::Weapon,
    primary: u32,
    dice: &mut crate::random::Random,
) -> u32 {
    let mut total = weapon.damage.wrapping_add(f32::from_bits(primary) as i32);
    for _ in 0..weapon.dice.max(0) {
        total = total.wrapping_add(dice.integer(1, weapon.sides));
    }
    i2r(total)
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

/// A table of `[Misc]` whose later values follow a formula (SPEC-0033):
/// the listed values from index `first` on, then
/// `f(x) = A * f(x - 1) + B * x + C`.
#[derive(Clone, Debug, PartialEq)]
pub struct XpTable {
    pub listed: Vec<i64>,
    pub formula: [i64; 3],
    /// The index of the first listed value.
    pub first: i64,
}

impl XpTable {
    /// The value at index `x`; 0 below the first index.
    pub fn value(&self, x: i64) -> i64 {
        if x < self.first || self.listed.is_empty() {
            return 0;
        }
        let at = (x - self.first) as usize;
        if let Some(&v) = self.listed.get(at) {
            return v;
        }
        let [a, b, c] = self.formula;
        let mut v = *self.listed.last().unwrap();
        for i in self.first + self.listed.len() as i64..=x {
            v = a
                .saturating_mul(v)
                .saturating_add(b.saturating_mul(i))
                .saturating_add(c);
        }
        v
    }
}

/// The hero constants of `[Misc]` (SPEC-0033, SPEC-0034).
#[derive(Clone, Debug, PartialEq)]
pub struct Heroes {
    /// XP to reach a level (`NeedHeroXP`, indexed by that level from 2).
    pub need_xp: XpTable,
    /// XP for the death of a hero or another unit of a level.
    pub grant_hero_xp: XpTable,
    pub grant_normal_xp: XpTable,
    /// Percent of a creep's XP by the hero's level (`HeroFactorXP`).
    pub factor_xp: Vec<i64>,
    pub max_level: i64,
    /// `HeroExpRange`, as a real.
    pub exp_range: u32,
    /// `StrHitPointBonus`, `IntManaBonus`, `AgiAttackSpeedBonus`,
    /// `StrAttackBonus`, as reals.
    pub str_life: u32,
    pub int_mana: u32,
    pub agi_attack_speed: u32,
    pub str_attack: u32,
    /// The hero levels between two levels of an ability whose own
    /// `levelSkip` is 0 (`HeroAbilityLevelSkip`, SPEC-0035).
    pub ability_level_skip: i64,
}

impl Default for Heroes {
    fn default() -> Heroes {
        Heroes {
            need_xp: XpTable {
                listed: vec![200],
                formula: [1, 100, 0],
                first: 2,
            },
            grant_hero_xp: XpTable {
                listed: vec![100, 120, 160, 220, 300],
                formula: [1, 0, 100],
                first: 1,
            },
            grant_normal_xp: XpTable {
                listed: vec![25],
                formula: [1, 5, 5],
                first: 1,
            },
            factor_xp: vec![80, 70, 60, 50, 0],
            max_level: 10,
            exp_range: s2r("1200"),
            str_life: s2r("25"),
            int_mana: s2r("15"),
            agi_attack_speed: s2r("0.02"),
            str_attack: s2r("1.0"),
            ability_level_skip: 2,
        }
    }
}

impl Heroes {
    /// Takes `key = value` if it is one of the hero constants.
    fn set(&mut self, key: &str, value: &str) -> bool {
        let list = || -> Vec<i64> {
            value
                .split(',')
                .filter_map(|v| v.trim().parse::<f64>().ok())
                .map(|v| v as i64)
                .collect()
        };
        let number = || value.trim().parse::<f64>().map_or(0, |v| v as i64);
        let formula = |table: &mut XpTable, letter: &str| match letter {
            "A" => table.formula[0] = number(),
            "B" => table.formula[1] = number(),
            _ => table.formula[2] = number(),
        };
        match key {
            "NeedHeroXP" => self.need_xp.listed = list(),
            "GrantHeroXP" => self.grant_hero_xp.listed = list(),
            "GrantNormalXP" => self.grant_normal_xp.listed = list(),
            "HeroFactorXP" => self.factor_xp = list(),
            "MaxHeroLevel" => self.max_level = number(),
            "HeroExpRange" => self.exp_range = s2r(value),
            "StrHitPointBonus" => self.str_life = s2r(value),
            "IntManaBonus" => self.int_mana = s2r(value),
            "AgiAttackSpeedBonus" => self.agi_attack_speed = s2r(value),
            "StrAttackBonus" => self.str_attack = s2r(value),
            "HeroAbilityLevelSkip" => self.ability_level_skip = number(),
            _ => {
                let tables = [
                    ("NeedHeroXPFormula", &mut self.need_xp),
                    ("GrantHeroXPFormula", &mut self.grant_hero_xp),
                    ("GrantNormalXPFormula", &mut self.grant_normal_xp),
                ];
                for (prefix, table) in tables {
                    if let Some(letter) = key.strip_prefix(prefix) {
                        formula(table, letter);
                        return true;
                    }
                }
                return false;
            }
        }
        true
    }

    /// The XP a hero needs for level `level` (0 for level 1).
    pub fn xp_for_level(&self, level: i64) -> i64 {
        self.need_xp.value(level)
    }

    /// The highest level reached with `xp`.
    pub fn level_of(&self, xp: i64) -> i64 {
        let mut level = 1;
        while level < self.max_level && xp >= self.xp_for_level(level + 1) {
            level += 1;
        }
        level
    }
}

#[cfg(test)]
mod hero_tests {
    use super::*;

    /// SPEC-0033, the tables beyond their listed values.
    #[test]
    fn xp_tables() {
        let h = Heroes::default();
        let need: Vec<i64> = (2..=10).map(|l| h.xp_for_level(l)).collect();
        assert_eq!(need, [200, 500, 900, 1400, 2000, 2700, 3500, 4400, 5400]);
        let normal: Vec<i64> = (1..=5).map(|l| h.grant_normal_xp.value(l)).collect();
        assert_eq!(normal, [25, 40, 60, 85, 115]);
        let hero: Vec<i64> = (1..=10).map(|l| h.grant_hero_xp.value(l)).collect();
        assert_eq!(hero, [100, 120, 160, 220, 300, 400, 500, 600, 700, 800]);
        assert_eq!(h.level_of(199), 1);
        assert_eq!(h.level_of(200), 2);
        assert_eq!(h.level_of(6000), 10);
    }
}
