# slakio key bindings

<!-- Generated from crates/slakio-tui/src/keymap.rs and the action registry. Do not edit;
     run `SLAKIO_BLESS=1 cargo test -p slakio-tui --test keybindings_doc` at the repository root. -->

Keys resolve in the current context first, then in its parents. A text input context (marked **[text]**) has no parent: every key it does not bind is typed.

## Everywhere (`root`)

| Keys | Action | Id |
|---|---|---|
| `:` | Open the command line | `cmdline.open` |

## Command line (`cmdline`) **[text]**

| Keys | Action | Id |
|---|---|---|
| `Enter` | Run the typed command | `cmdline.run` |
| `Esc` | Close the command line | `cmdline.cancel` |

## Commands

Type `:` and the command, then `Enter`.

| Command | Also | Action |
|---|---|---|
| `:qa` | `:qall` `:quitall` `:q` `:quit` | Quit |
