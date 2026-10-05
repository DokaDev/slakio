//! `docs/keybindings.md`, generated from the default bindings, the action registry and the
//! English catalog. `tests/keybindings_doc.rs` fails when the committed file is out of date.

use super::{Ctx, Keymap, check, keys, parse_keys};
use crate::action::REGISTRY;
use slakio_core::i18n::Lang;

/// The whole document (LF line endings).
pub fn render() -> String {
    // With the kitty keyboard protocol: every default binding (`Ctrl+I` is bound only then, and
    // the last section says so).
    let km = Keymap::new(true);
    let mut s = String::new();
    s.push_str("# slakio key bindings\n\n");
    s.push_str(
        "<!-- Generated from crates/slakio-tui/src/keymap.rs and the action registry. Do not edit;\n     \
         run `SLAKIO_BLESS=1 cargo test -p slakio-tui --test keybindings_doc` at the repository root. -->\n\n",
    );
    s.push_str(
        "The keys of *Always* work everywhere, also while typing. Other keys resolve in the current \
         context first, then in its parents. A text input context (marked **[text]**) has no parent: \
         every key it does not bind is typed. A modal context (marked **[modal]**) has no parent \
         either: only its own keys work there. `?`, `F1` or `Space ?` show these keys in the app; \
         after `Space` (or another first key of a sequence) a popup lists what may follow.\n",
    );
    for &ctx in Ctx::ALL {
        if km.bindings(ctx).next().is_none() {
            continue;
        }
        let text = match (ctx.is_text_input(), ctx.is_modal()) {
            (true, _) => " **[text]**",
            (_, true) => " **[modal]**",
            _ => "",
        };
        s.push_str(&format!("\n## {} (`{}`){text}\n\n", ctx.label().text(Lang::En), ctx.id()));
        s.push_str("| Keys | Action | Id |\n|---|---|---|\n");
        for (k, action) in km.bindings(ctx) {
            let spec = crate::action::spec(action);
            s.push_str(&format!("| `{}` | {} | `{}` |\n", keys::label(k), spec.label.text(Lang::En), spec.id));
        }
    }
    s.push_str(
        "\n## Commands\n\nType `:` and the command, then `Enter`.\n\n| Command | Also | Action |\n|---|---|---|\n",
    );
    for spec in REGISTRY.iter().filter(|s| !s.commands.is_empty()) {
        let also = spec.commands[1..].iter().map(|c| format!("`:{c}`")).collect::<Vec<_>>().join(" ");
        s.push_str(&format!("| `:{}` | {also} | {} |\n", spec.commands[0], spec.label.text(Lang::En)));
    }
    let fragile: Vec<_> = km.all().iter().filter(|b| b.keys.iter().any(|k| check::fragile(*k))).collect();
    if !fragile.is_empty() {
        s.push_str(
            "\n## Keys that need the kitty keyboard protocol or Option as Alt\n\n\
             Without the kitty keyboard protocol a terminal sends `Ctrl+I` as `Tab`, `Ctrl+M` as `Enter`, \
             `Ctrl+[` as `Esc`, and on some terminals `Backspace` as `Ctrl+H`; on macOS, `Alt` keys need \
             \"Option as Alt\" (Ghostty: `macos-option-as-alt = true`). Each of these keys has another key that \
             works everywhere.\n\n| Keys | Action | Also |\n|---|---|---|\n",
        );
        for b in fragile {
            let spec = crate::action::spec(b.action);
            let also = km
                .keys_for(b.action, b.ctx)
                .into_iter()
                .filter(|k| !k.iter().any(|c| check::fragile(*c)))
                .map(|k| format!("`{}`", keys::label(&k)))
                .collect::<Vec<_>>()
                .join(" ");
            let only =
                if crate::keymap::DEFAULTS.iter().any(|d| d.kitty && parse_keys(d.keys).ok().as_ref() == Some(&b.keys))
                {
                    " (bound only with the kitty keyboard protocol)"
                } else {
                    ""
                };
            s.push_str(&format!("| `{}`{only} | {} | {also} |\n", keys::label(&b.keys), spec.label.text(Lang::En)));
        }
    }
    s
}
