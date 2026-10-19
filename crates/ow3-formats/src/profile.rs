//! Profile files: the `.txt` data of the game in sections (`[hpea]`) of
//! `Key=Value` lines, such as `Units\HumanUnitFunc.txt`,
//! `Units\HumanUnitStrings.txt` and `UI\war3skins.txt`.
//!
//! Lines starting with `//` are comments. A value may be quoted, and a
//! quoted value may hold commas; lists are separated by commas.

use std::collections::BTreeMap;

/// The sections of one or more profile files, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Profile {
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

impl Profile {
    /// Adds the sections of `text`. A key already read keeps its first
    /// value, so files are added from the one that wins first.
    pub fn add(&mut self, text: &str) {
        let mut section = String::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.to_owned();
                continue;
            }
            if let Some((key, value)) = line.split_once('=') {
                self.sections
                    .entry(section.clone())
                    .or_default()
                    .entry(key.trim().to_owned())
                    .or_insert_with(|| value.trim().to_owned());
            }
        }
    }

    /// The raw value of `key` in `section`, without the quotes of a quoted
    /// value. Keys are matched without regard to case.
    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        let s = self.sections.get(section)?;
        let value = s.get(key).or_else(|| {
            s.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v)
        })?;
        Some(
            value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value),
        )
    }

    /// The comma separated items of `key` in `section`, each without
    /// quotes; empty when the key is missing.
    pub fn list(&self, section: &str, key: &str) -> Vec<String> {
        self.get(section, key).map_or_else(Vec::new, split)
    }

    /// Whether there is a section `name`.
    pub fn has(&self, name: &str) -> bool {
        self.sections.contains_key(name)
    }
}

/// The items of a comma separated list; commas inside quotes belong to
/// the item.
pub fn split(value: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut item = String::new();
    let mut quoted = false;
    for c in value.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => items.push(std::mem::take(&mut item)),
            _ => item.push(c),
        }
    }
    if !item.is_empty() || !items.is_empty() {
        items.push(item);
    }
    items.into_iter().map(|i| i.trim().to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_and_lists() {
        let mut p = Profile::default();
        p.add(
            "// comment\n[hpea]\nBuilds=htow,hhou\nButtonpos=0,0\n\
             Ubertip=\"Builds, mines, repairs.\"\n[htow]\nTrains=hpea\n",
        );
        p.add("[hpea]\nBuilds=other\nName=Peasant\n");
        assert_eq!(p.list("hpea", "Builds"), ["htow", "hhou"]);
        assert_eq!(p.list("hpea", "buttonpos"), ["0", "0"]);
        assert_eq!(p.get("hpea", "Ubertip"), Some("Builds, mines, repairs."));
        assert_eq!(p.get("hpea", "Name"), Some("Peasant"));
        assert_eq!(p.list("htow", "Trains"), ["hpea"]);
        assert!(p.list("htow", "Builds").is_empty());
        assert_eq!(split("a,\"b,c\",d"), ["a", "b,c", "d"]);
    }
}
