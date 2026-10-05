//! i18n: the typed message catalog.
//!
//! `build.rs` checks `locales/*.toml` and generates [`Label`] (entries without
//! placeholders) and [`Msg`] (any entry, with typed arguments) from them, so an unknown key or
//! a missing argument is a compile error and every locale is complete. UI chrome takes
//! [`Localized`] text, which only the catalog (or the explicit [`Localized::verbatim`] escape
//! hatch) can produce. See `docs/architecture.md`, "UI strings (i18n)".
//!
//! ```
//! use slakio_core::i18n::{I18n, Label, Lang, Localized, Msg};
//! let i = I18n::new(Lang::En);
//! assert_eq!(i.label(Label::ModeNormal), "NORMAL");
//! assert_eq!(i.msg(&Msg::CommandUnknown { name: "wq".into() }), "Not a command: wq");
//! let _data: Localized = Localized::verbatim("#general");
//! ```
//!
//! What the compiler rejects. Each example differs from a line above in the one mistake it
//! shows (stable rustdoc does not check the error codes, so keep them minimal):
//!
//! ```compile_fail,E0599
//! // A key that is not in en.toml.
//! let _ = slakio_core::i18n::Label::ModeNormel;
//! ```
//!
//! ```compile_fail,E0063
//! // A message without its argument (`command.unknown` has `{name}`).
//! let _ = slakio_core::i18n::Msg::CommandUnknown {};
//! ```
//!
//! ```compile_fail,E0277
//! // Chrome text from a raw string (only `Localized::verbatim` opts out).
//! let _: slakio_core::i18n::Localized = "Quit".into();
//! ```

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;
use std::time::Duration;

/// A UI language. Each has a catalog `locales/<code>.toml` (listed in `build.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ko,
}

include!(concat!(env!("OUT_DIR"), "/messages.rs"));

/// Text for UI chrome (titles, labels, hints, status messages). It comes from the typed
/// catalog ([`I18n::label`], [`I18n::msg`]); text that is not translated at all (user and
/// workspace data, key names, the product name) goes through the grep-able escape hatch
/// [`Localized::verbatim`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Localized(Cow<'static, str>);

impl Localized {
    /// Escape hatch for text that is not translated: user or workspace data, key names, or
    /// chrome composed from those and already localized parts. Never pass English UI wording
    /// here; add a catalog key instead.
    pub fn verbatim(text: impl Into<Cow<'static, str>>) -> Self {
        Self(text.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Deref for Localized {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Localized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for Localized {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for Localized {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

/// The current UI language and the lookups in it.
pub struct I18n {
    pub lang: Lang,
}

impl I18n {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    /// A catalog entry without placeholders, in the current language.
    pub fn label(&self, l: Label) -> Localized {
        Localized(Cow::Borrowed(l.text(self.lang)))
    }

    /// A catalog message with its arguments, in the current language.
    pub fn msg(&self, m: &Msg) -> Localized {
        match m {
            Msg::Label(l) => self.label(*l),
            m => Localized(Cow::Owned(m.render(self.lang))),
        }
    }
}

/// A number with thousands separators (`{count}` placeholders).
pub fn fmt_count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A duration as `123ms` / `1.2s` (`{elapsed}` and `{latency}` placeholders).
pub fn fmt_elapsed(d: Duration) -> String {
    if d < Duration::from_secs(1) { format!("{}ms", d.as_millis()) } else { format!("{:.1}s", d.as_secs_f64()) }
}

/// Language resolution: explicit `en`/`ko` wins; `auto` checks LC_ALL -> LC_MESSAGES -> LANG
/// (first non-empty value decides) and picks ko when it starts with `ko`.
pub fn detect_lang(setting: &str, env: impl Fn(&str) -> Option<String>) -> Lang {
    match setting.to_ascii_lowercase().as_str() {
        "en" => return Lang::En,
        "ko" => return Lang::Ko,
        _ => {}
    }
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(v) = env(var).filter(|v| !v.is_empty()) {
            return if v.to_ascii_lowercase().starts_with("ko") { Lang::Ko } else { Lang::En };
        }
    }
    Lang::En
}

#[cfg(test)]
mod tests;
