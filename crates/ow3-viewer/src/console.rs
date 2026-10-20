//! The frames of the game's interface laid out from its definition files
//! (`UI\FrameDef\UI\*.fdf`), with the race's files from `UI\war3skins.txt`
//! and the texts of `UI\FrameDef\GlobalStrings.fdf`.
//!
//! A frame's place comes from its `Width`, `Height` and `Anchor` or
//! `SetPoint` against its parent or a named frame; textures and strings
//! inside it the same way. Definitions inherit the properties of the one
//! they name with `INHERITS`.

use std::collections::{BTreeMap, HashMap};

use ow3_formats::fdf::{Definition, Value, definitions, string_lists};
use ow3_formats::mpq::ArchiveChain;

use crate::ui::{Blend, Frame, Justify, Quad, Text};

/// What a layout fills in: the text of a string or button and the file
/// of a texture, by name.
pub struct Values<'a> {
    pub texts: &'a dyn Fn(&str) -> Option<String>,
    pub files: &'a dyn Fn(&str) -> Option<String>,
}

/// Left, bottom, right, top in interface coordinates.
pub type Rect = [f32; 4];

/// The interface's definitions, skins and texts.
pub struct Interface {
    templates: HashMap<String, Definition>,
    skins: BTreeMap<String, BTreeMap<String, String>>,
    strings: HashMap<String, String>,
    /// The race whose skin is used (`Human`, `Orc`, `NightElf`, `Undead`).
    pub race: String,
}

/// A point of a rectangle (`TOPLEFT`, `CENTER`, ...), as fractions of its
/// width and height from the lower left corner.
fn point(name: &str) -> Option<[f32; 2]> {
    Some(match name {
        "TOPLEFT" => [0.0, 1.0],
        "TOP" => [0.5, 1.0],
        "TOPRIGHT" => [1.0, 1.0],
        "LEFT" => [0.0, 0.5],
        "CENTER" => [0.5, 0.5],
        "RIGHT" => [1.0, 0.5],
        "BOTTOMLEFT" => [0.0, 0.0],
        "BOTTOM" => [0.5, 0.0],
        "BOTTOMRIGHT" => [1.0, 0.0],
        _ => return None,
    })
}

fn at(rect: Rect, p: [f32; 2]) -> [f32; 2] {
    [
        rect[0] + (rect[2] - rect[0]) * p[0],
        rect[1] + (rect[3] - rect[1]) * p[1],
    ]
}

/// The rectangle of size `size` whose point `p` is at `spot`.
fn placed(size: [f32; 2], p: [f32; 2], spot: [f32; 2]) -> Rect {
    let x0 = spot[0] - size[0] * p[0];
    let y0 = spot[1] - size[1] * p[1];
    [x0, y0, x0 + size[0], y0 + size[1]]
}

fn numbers(values: &[Value]) -> Vec<f32> {
    values.iter().filter_map(Value::number).collect()
}

/// A text with colour codes (`|cAARRGGBB` ... `|r`) as plain text; the
/// colour of its first code, if any.
pub fn plain(text: &str) -> (String, Option<[f32; 4]>) {
    let mut out = String::new();
    let mut colour = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '|' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('c' | 'C') => {
                chars.next();
                let hex: String = chars.by_ref().take(8).collect();
                if colour.is_none()
                    && let Ok(v) = u32::from_str_radix(&hex, 16)
                {
                    colour = Some([
                        ((v >> 16) & 0xFF) as f32 / 255.0,
                        ((v >> 8) & 0xFF) as f32 / 255.0,
                        (v & 0xFF) as f32 / 255.0,
                        ((v >> 24) & 0xFF) as f32 / 255.0,
                    ]);
                }
            }
            Some('r' | 'R' | 'n' | 'N') => {
                chars.next();
            }
            _ => out.push(c),
        }
    }
    (out, colour)
}

impl Interface {
    /// Reads the interface's files from `chain`.
    pub fn load(chain: &mut ArchiveChain, race: &str) -> Interface {
        let mut templates = HashMap::new();
        let toc = chain
            .read("UI\\FrameDef\\FrameDef.toc")
            .map(|d| String::from_utf8_lossy(&d).into_owned())
            .unwrap_or_default();
        for name in toc.lines().map(str::trim) {
            if !name.to_ascii_lowercase().starts_with("ui\\framedef\\ui\\") {
                continue;
            }
            let Some(data) = chain.read(name) else {
                continue;
            };
            for d in definitions(&String::from_utf8_lossy(&data)) {
                if let Some(n) = d.name.clone() {
                    templates.entry(n).or_insert(d);
                }
            }
        }
        let mut strings = HashMap::new();
        // Every copy, the localized one first: it lacks some keys.
        for data in chain.read_all("UI\\FrameDef\\GlobalStrings.fdf") {
            for (k, v) in string_lists(&String::from_utf8_lossy(&data)) {
                strings.entry(k).or_insert(v);
            }
        }
        let mut skins: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        if let Some(data) = chain.read("UI\\war3skins.txt") {
            let mut section = String::new();
            for line in String::from_utf8_lossy(&data).lines() {
                let line = line.trim();
                if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    section = name.to_owned();
                } else if let Some((k, v)) = line.split_once('=')
                    && !line.starts_with("//")
                {
                    skins
                        .entry(section.clone())
                        .or_default()
                        .entry(k.trim().to_owned())
                        .or_insert_with(|| v.trim().to_owned());
                }
            }
        }
        Interface {
            templates,
            skins,
            strings,
            race: race.to_owned(),
        }
    }

    /// The file a decorated name stands for: the race's entry of the skins,
    /// else the default one, else the name itself.
    pub fn skin(&self, name: &str) -> String {
        [self.race.as_str(), "Default"]
            .iter()
            .find_map(|s| self.skins.get(*s).and_then(|m| m.get(name)))
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    }

    /// The text of string key `key`, or the key itself.
    pub fn string(&self, key: &str) -> String {
        self.strings
            .get(key)
            .cloned()
            .unwrap_or_else(|| key.to_owned())
    }

    /// `d` with the properties of the definitions it inherits from, its
    /// own coming last (and so overriding them for [`Definition::property`]
    /// when searched from the end).
    fn resolved(&self, d: &Definition) -> Vec<(String, Vec<Value>)> {
        let mut props = Vec::new();
        if let Some(base) = d.inherits.as_ref().and_then(|b| self.templates.get(b)) {
            props.extend(self.resolved(base));
        }
        props.extend(d.properties.iter().cloned());
        props
    }

    /// Lays out the top-level frame `name` with its rectangle `rect` into
    /// `out`. `texts` gives the text of a string or button by name.
    pub fn lay_out(
        &self,
        name: &str,
        rect: Rect,
        values: &Values,
        out: &mut Frame,
    ) -> HashMap<String, Rect> {
        let mut named = HashMap::new();
        if let Some(d) = self.templates.get(name) {
            named.insert(name.to_owned(), rect);
            let props = self.resolved(d);
            let decorate = has(&props, "DecorateFileNames");
            self.children(d, rect, decorate, values, out, &mut named);
        }
        named
    }

    fn children(
        &self,
        d: &Definition,
        rect: Rect,
        decorate: bool,
        values: &Values,
        out: &mut Frame,
        named: &mut HashMap<String, Rect>,
    ) {
        for child in &d.children {
            let props = self.resolved(child);
            match child.kind.as_str() {
                "Texture" => {
                    let r = self.rect_of(&props, rect, named, None);
                    if let Some(n) = &child.name {
                        named.insert(n.clone(), r);
                    }
                    let file = child.name.as_deref().and_then(values.files);
                    if let Some(mut q) = self.texture(&props, r, decorate) {
                        if let Some(file) = file {
                            q.texture = file;
                        }
                        out.quads.push(q);
                    }
                }
                "String" => {
                    // Without a size a string is a line of its font's
                    // height at its anchor.
                    let size = last(&props, "Font")
                        .and_then(|v| v.get(1)?.number())
                        .unwrap_or(0.01);
                    let r = self.rect_of(&props, rect, named, Some([0.0, size]));
                    if let Some(n) = &child.name {
                        named.insert(n.clone(), r);
                    }
                    // Its `Text` only when it is a key of the strings: the
                    // others are placeholders the game fills in.
                    let text = child.name.as_deref().and_then(values.texts).or_else(|| {
                        let key = last(&props, "Text")?.first()?.text().to_owned();
                        self.strings.get(&key).cloned()
                    });
                    if let Some(text) = text {
                        // A string as wide as its text grows from its
                        // anchor point: centred on a TOP, CENTER or BOTTOM
                        // anchor, to the left of a right one.
                        let mut props = props.clone();
                        if !has(&props, "Width") && !has(&props, "FontJustificationH") {
                            let anchored = ["SetPoint", "Anchor"].iter().find_map(|k| {
                                last(&props, k)?.first().and_then(|p| point(p.text()))
                            });
                            let justify = match anchored.map(|p| p[0]) {
                                Some(x) if x == 0.5 => Some("JUSTIFYCENTER"),
                                Some(x) if x == 1.0 => Some("JUSTIFYRIGHT"),
                                _ => None,
                            };
                            if let Some(j) = justify {
                                props.push((
                                    "FontJustificationH".to_owned(),
                                    vec![Value::Word(j.to_owned())],
                                ));
                            }
                        }
                        out.texts.push(self.text(&props, r, &text, decorate));
                    }
                }
                "Frame" => {
                    let r = self.rect_of(&props, rect, named, None);
                    if let Some(n) = &child.name {
                        named.insert(n.clone(), r);
                    }
                    self.button(child, &props, r, decorate, values.texts, out);
                    let inner = decorate || has(&props, "DecorateFileNames");
                    self.children(child, r, inner, values, out, named);
                }
                _ => {}
            }
        }
    }

    /// The textures and text of a `SIMPLEBUTTON` in its normal state.
    fn button(
        &self,
        d: &Definition,
        props: &[(String, Vec<Value>)],
        rect: Rect,
        decorate: bool,
        texts: &dyn Fn(&str) -> Option<String>,
        out: &mut Frame,
    ) {
        let decorate = decorate || has(props, "DecorateFileNames");
        if let Some(t) = last(props, "NormalTexture")
            .and_then(|v| v.first())
            .and_then(|v| self.templates.get(v.text()))
        {
            let tprops = self.resolved(t);
            if let Some(q) = self.texture(&tprops, rect, decorate) {
                out.quads.push(q);
            }
        }
        if let Some(v) = last(props, "NormalText") {
            let template = v.first().and_then(|t| self.templates.get(t.text()));
            let key = v.get(1).map(Value::text).unwrap_or_default();
            let text = d
                .name
                .as_deref()
                .and_then(texts)
                .unwrap_or_else(|| self.string(key));
            if let Some(t) = template {
                let mut tprops = self.resolved(t);
                tprops.push((
                    "FontJustificationH".to_owned(),
                    vec![Value::Word("JUSTIFYCENTER".to_owned())],
                ));
                out.texts.push(self.text(&tprops, rect, &text, decorate));
            }
        }
    }

    /// The rectangle of a texture, string or frame inside `parent`. Without
    /// a size, an anchored one is `empty` (a point) and one without any
    /// placement fills its parent.
    fn rect_of(
        &self,
        props: &[(String, Vec<Value>)],
        parent: Rect,
        named: &HashMap<String, Rect>,
        empty: Option<[f32; 2]>,
    ) -> Rect {
        let width = last(props, "Width").and_then(|v| v.first()?.number());
        let height = last(props, "Height").and_then(|v| v.first()?.number());
        let size = match (width, height) {
            (None, None) if empty.is_none() && !has(props, "Anchor") && !has(props, "SetPoint") => {
                return parent;
            }
            (w, h) => [
                w.unwrap_or(empty.map_or(parent[2] - parent[0], |e| e[0])),
                h.unwrap_or(empty.map_or(parent[3] - parent[1], |e| e[1])),
            ],
        };
        if let Some(v) = last(props, "SetPoint") {
            // SetPoint POINT, "Relative", RELPOINT, dx, dy.
            let p = v.first().and_then(|p| point(p.text()));
            let other = v
                .get(1)
                .and_then(|r| named.get(r.text()))
                .copied()
                .unwrap_or(parent);
            let rp = v.get(2).and_then(|p| point(p.text()));
            let d = numbers(&v[3.min(v.len())..]);
            if let (Some(p), Some(rp)) = (p, rp) {
                let spot = at(other, rp);
                let spot = [
                    spot[0] + d.first().unwrap_or(&0.0),
                    spot[1] + d.get(1).unwrap_or(&0.0),
                ];
                return placed(size, p, spot);
            }
        }
        if let Some(v) = last(props, "Anchor") {
            // Anchor POINT, dx, dy: the point at the parent's same point.
            if let Some(p) = v.first().and_then(|p| point(p.text())) {
                let d = numbers(&v[1..]);
                let spot = at(parent, p);
                let spot = [
                    spot[0] + d.first().unwrap_or(&0.0),
                    spot[1] + d.get(1).unwrap_or(&0.0),
                ];
                return placed(size, p, spot);
            }
        }
        placed(size, [0.5, 0.5], at(parent, [0.5, 0.5]))
    }

    fn texture(&self, props: &[(String, Vec<Value>)], rect: Rect, decorate: bool) -> Option<Quad> {
        let file = last(props, "File")?.first()?.text().to_owned();
        let file = if decorate { self.skin(&file) } else { file };
        let uv = last(props, "TexCoord")
            .map(numbers)
            .filter(|n| n.len() == 4);
        let blend = match last(props, "AlphaMode")
            .and_then(|v| v.first())
            .map(Value::text)
        {
            Some("ALPHAKEY") => Blend::Key,
            Some("ADD") => Blend::Add,
            _ => Blend::Alpha,
        };
        Some(Quad {
            rect,
            uv: uv.map_or([0.0, 1.0, 0.0, 1.0], |n| [n[0], n[1], n[2], n[3]]),
            texture: file,
            colour: [1.0; 4],
            blend,
        })
    }

    fn text(&self, props: &[(String, Vec<Value>)], rect: Rect, text: &str, decorate: bool) -> Text {
        let font = last(props, "Font");
        let font_name = font.and_then(|v| v.first()).map(|v| v.text().to_owned());
        let size = font.and_then(|v| v.get(1)?.number()).unwrap_or(0.01);
        let font_file = font_name.map(|n| {
            if decorate || !n.contains('\\') {
                self.skin(&n)
            } else {
                n
            }
        });
        let colour = last(props, "FontColor")
            .map(numbers)
            .filter(|n| n.len() >= 3);
        let (plain_text, coded) = plain(text);
        let justify = match last(props, "FontJustificationH")
            .and_then(|v| v.first())
            .map(Value::text)
        {
            Some("JUSTIFYRIGHT") => Justify::Right,
            Some("JUSTIFYCENTER") => Justify::Centre,
            _ => Justify::Left,
        };
        Text {
            rect,
            size,
            font: font_file.unwrap_or_else(|| self.skin("MasterFont")),
            text: plain_text,
            colour: coded.unwrap_or(colour.map_or([1.0, 0.82, 0.0, 1.0], |n| {
                [n[0], n[1], n[2], n.get(3).copied().unwrap_or(1.0)]
            })),
            justify,
        }
    }
}

fn has(props: &[(String, Vec<Value>)], key: &str) -> bool {
    props.iter().any(|(k, _)| k == key)
}

/// The values of the last property `key`: the definition's own over the
/// inherited ones.
fn last<'a>(props: &'a [(String, Vec<Value>)], key: &str) -> Option<&'a [Value]> {
    props
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_codes() {
        let (text, colour) = plain("|Cff00ff00No upkeep|r");
        assert_eq!(text, "No upkeep");
        assert_eq!(colour, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(plain("a|nb").0, "ab");
    }

    #[test]
    fn anchors() {
        // A 0.1 x 0.05 texture anchored TOPRIGHT, -0.032 in from the right.
        let r = placed([0.1, 0.05], point("TOPRIGHT").unwrap(), [0.8 - 0.032, 0.6]);
        assert!((r[2] - 0.768).abs() < 1e-6 && (r[3] - 0.6).abs() < 1e-6);
        assert!((r[0] - 0.668).abs() < 1e-6 && (r[1] - 0.55).abs() < 1e-6);
    }
}
