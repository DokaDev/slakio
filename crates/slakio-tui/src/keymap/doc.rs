//! `docs/keybindings.md`, generated from the default bindings, the action registry and the
//! English catalog. `tests/keybindings_doc.rs` fails when the committed file is out of date.

use super::{Ctx, Keymap, keys};
use crate::action::REGISTRY;
use slakio_core::i18n::Lang;

/// The whole document (LF line endings).
pub fn render() -> String {
    let km = Keymap::default();
    let mut s = String::new();
    s.push_str("# slakio key bindings\n\n");
    s.push_str(
        "<!-- Generated from crates/slakio-tui/src/keymap.rs and the action registry. Do not edit;\n     \
         run `SLAKIO_BLESS=1 cargo test -p slakio-tui --test keybindings_doc` at the repository root. -->\n\n",
    );
    s.push_str(
        "Keys resolve in the current context first, then in its parents. A text input context \
         (marked **[text]**) has no parent: every key it does not bind is typed.\n",
    );
    for &ctx in Ctx::ALL {
        let text = if ctx.is_text_input() { " **[text]**" } else { "" };
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
    s
}
