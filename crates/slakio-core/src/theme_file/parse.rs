//! Checking the text of a theme file: every problem with where it is, the first one reported.

use super::{COLOR_TOKENS, ColorSpec, DimSpec, MAX_PALETTE, MAX_PERCENT, ModifierSpec, OTHER_COLOR_TOKENS, Problem};
use super::{STYLE_TOKENS, StyleSpec, ThemeSpec};
use serde::Deserialize;
use std::collections::BTreeMap;
use toml::Spanned;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    extends: Option<Spanned<String>>,
    #[serde(default)]
    colors: BTreeMap<Spanned<String>, Spanned<toml::Value>>,
    #[serde(default)]
    styles: BTreeMap<Spanned<String>, Spanned<StyleFile>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleFile {
    #[serde(default)]
    fg: Option<Spanned<String>>,
    #[serde(default)]
    bg: Option<Spanned<String>>,
    #[serde(default)]
    modifiers: Vec<Spanned<String>>,
}

/// A file being checked: what it sets, with where, and every problem with where it is.
#[derive(Default)]
struct Checked {
    spec: ThemeSpec,
    colors: Vec<(usize, &'static str, ColorSpec)>,
    styles: Vec<(usize, &'static str, StyleSpec)>,
    problems: Vec<(usize, Problem)>,
}

/// Check the text of a theme file. `Err`: the first problem in the file (by where it is) and
/// its line, when known.
pub fn parse(text: &str) -> Result<ThemeSpec, (Option<usize>, Problem)> {
    // Bytes, not a `str` slice: an offset inside a character never panics.
    let line = |at: usize| text.as_bytes()[..at.min(text.len())].iter().filter(|b| **b == b'\n').count() + 1;
    let f: File = toml::from_str(text)
        .map_err(|e| (e.span().map(|s| line(s.start)), Problem::Toml(e.message().trim().to_string())))?;
    let mut c = Checked::default();
    if let Some(e) = &f.extends {
        let base = e.get_ref().to_ascii_lowercase();
        if base != "auto" && crate::config::THEMES.contains(&base.as_str()) {
            c.spec.extends = Some((base, line(e.span().start)));
        } else {
            c.problems.push((e.span().start, Problem::UnknownBase(e.get_ref().clone())));
        }
    }
    for (k, v) in &f.colors {
        c.color(text, k, v);
    }
    for (k, v) in &f.styles {
        c.style(k, v.get_ref());
    }
    if let Some((at, p)) = c.problems.into_iter().min_by_key(|p| p.0) {
        return Err((Some(line(at)), p));
    }
    c.colors.sort_by_key(|x| x.0);
    c.styles.sort_by_key(|x| x.0);
    c.spec.colors = c.colors.into_iter().map(|(_, n, v)| (n, v)).collect();
    c.spec.styles = c.styles.into_iter().map(|(_, n, v)| (n, v)).collect();
    Ok(c.spec)
}

/// The `&'static` token of `list` that is `name`.
fn token(list: &[&'static str], name: &str) -> Option<&'static str> {
    list.iter().copied().find(|t| *t == name)
}

/// A value as the problem shows it: a string as it is, anything else as TOML.
fn value_text(v: &toml::Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_string)
}

impl Checked {
    fn problem(&mut self, at: usize, p: Problem) {
        self.problems.push((at, p));
    }

    /// One colour, or its problem at `at`.
    fn one_color(&mut self, at: usize, v: &toml::Value) -> Option<ColorSpec> {
        let c = v.as_str().and_then(ColorSpec::parse);
        if c.is_none() {
            self.problem(at, Problem::BadColor(value_text(v)));
        }
        c
    }

    /// A key of `[colors]`.
    fn color(&mut self, text: &str, k: &Spanned<String>, v: &Spanned<toml::Value>) {
        let (name, at, vat) = (k.get_ref().as_str(), k.span().start, v.span().start);
        if let Some(t) = token(COLOR_TOKENS, name) {
            if let Some(c) = self.one_color(vat, v.get_ref()) {
                self.colors.push((at, t, c));
            }
            return;
        }
        match name {
            "workspaces" | "avatars" => {
                let items = item_starts(text, v.span());
                let list = self.palette(name, vat, &items, v.get_ref());
                if name == "workspaces" {
                    self.spec.workspaces = list;
                } else {
                    self.spec.avatars = list;
                }
            }
            "avatar_tint" => match v.get_ref().as_integer().and_then(|n| u16::try_from(n).ok()) {
                Some(n) if n <= MAX_PERCENT => self.spec.avatar_tint = Some(n),
                _ => self.problem(vat, Problem::BadPercent(name.to_string())),
            },
            "dim" => match dim(v.get_ref()) {
                Some(d) => self.spec.dim = Some(d),
                None => self.problem(vat, Problem::BadDim),
            },
            _ if STYLE_TOKENS.contains(&name) => self.problem(at, Problem::NotAColor(name.to_string())),
            _ => self.problem(at, Problem::UnknownToken(name.to_string())),
        }
    }

    /// `workspaces` or `avatars`: 1 to [`MAX_PALETTE`] colours.
    /// `starts`: where each item is in the file (else the list's start, `at`, is said).
    fn palette(&mut self, name: &str, at: usize, starts: &[usize], v: &toml::Value) -> Option<Vec<ColorSpec>> {
        let Some(items) = v.as_array().filter(|a| (1..=MAX_PALETTE).contains(&a.len())) else {
            self.problem(at, Problem::BadPalette(name.to_string()));
            return None;
        };
        let colors: Vec<Option<ColorSpec>> =
            items.iter().enumerate().map(|(n, i)| self.one_color(starts.get(n).copied().unwrap_or(at), i)).collect();
        colors.into_iter().collect()
    }

    /// A key of `[styles]`.
    fn style(&mut self, k: &Spanned<String>, s: &StyleFile) {
        let (name, at) = (k.get_ref().as_str(), k.span().start);
        let Some(t) = token(STYLE_TOKENS, name) else {
            let known = COLOR_TOKENS.contains(&name) || OTHER_COLOR_TOKENS.contains(&name);
            self.problem(at, if known { Problem::NotAStyle(name.into()) } else { Problem::UnknownToken(name.into()) });
            return;
        };
        let mut color = |c: &Spanned<String>| {
            let parsed = ColorSpec::parse(c.get_ref());
            if parsed.is_none() {
                self.problems.push((c.span().start, Problem::BadColor(c.get_ref().clone())));
            }
            parsed
        };
        let fg = s.fg.as_ref().and_then(&mut color);
        let bg = s.bg.as_ref().and_then(&mut color);
        let mut modifiers = Vec::new();
        for m in &s.modifiers {
            match ModifierSpec::parse(m.get_ref()) {
                Some(parsed) => modifiers.push(parsed),
                None => self.problem(m.span().start, Problem::BadModifier(m.get_ref().clone())),
            }
        }
        self.styles.push((at, t, StyleSpec { fg, bg, modifiers }));
    }
}

/// Where each item of the list at `span` of `text` starts (none when it is not a list).
fn item_starts(text: &str, span: std::ops::Range<usize>) -> Vec<usize> {
    #[derive(Deserialize)]
    struct List {
        x: Vec<Spanned<toml::Value>>,
    }
    const KEY: &str = "x = ";
    let Some(list) = text.get(span.clone()) else { return Vec::new() };
    match toml::from_str::<List>(&format!("{KEY}{list}")) {
        Ok(l) => l.x.iter().map(|i| (span.start + i.span().start).saturating_sub(KEY.len())).collect(),
        Err(_) => Vec::new(),
    }
}

/// `dim`: `"modifier"` or `{ toward = "#rrggbb", keep = 0 to 100 }` (both keys, nothing else).
fn dim(v: &toml::Value) -> Option<DimSpec> {
    if let Some(s) = v.as_str() {
        return s.trim().eq_ignore_ascii_case("modifier").then_some(DimSpec::Modifier);
    }
    let t = v.as_table().filter(|t| t.len() == 2)?;
    let Some(ColorSpec::Rgb(r, g, b)) = t.get("toward").and_then(toml::Value::as_str).and_then(ColorSpec::parse) else {
        return None;
    };
    let keep = t.get("keep")?.as_integer().and_then(|n| u16::try_from(n).ok()).filter(|n| *n <= MAX_PERCENT)?;
    Some(DimSpec::Blend { toward: (r, g, b), keep })
}
