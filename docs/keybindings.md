# slakio key bindings

<!-- Generated from crates/slakio-tui/src/keymap.rs and the action registry. Do not edit;
     run `SLAKIO_BLESS=1 cargo test -p slakio-tui --test keybindings_doc` at the repository root. -->

Keys resolve in the current context first, then in its parents. A text input context (marked **[text]**) has no parent: every key it does not bind is typed.

## Everywhere (`root`)

| Keys | Action | Id |
|---|---|---|
| `:` | Open the command line | `cmdline.open` |

## Main screen (`shell`)

| Keys | Action | Id |
|---|---|---|
| `Ctrl+H` | Move the focus left | `focus.left` |
| `Ctrl+L` | Move the focus right | `focus.right` |
| `Space w h` | Move the focus left | `focus.left` |
| `Space w l` | Move the focus right | `focus.right` |
| `Space h` | Show Home | `view.home` |
| `Space d` | Show DMs | `view.dms` |
| `Space a` | Show Activity | `view.activity` |
| `Space f` | Show Files | `view.files` |
| `Space l` | Show Later | `view.later` |
| `Space e` | Show or hide the list panel | `list.toggle_panel` |

## Rail (`rail`)

| Keys | Action | Id |
|---|---|---|
| `j` | Next rail item | `rail.next` |
| `Down` | Next rail item | `rail.next` |
| `k` | Previous rail item | `rail.prev` |
| `Up` | Previous rail item | `rail.prev` |
| `Enter` | Show the rail item | `rail.select` |

## List panel (`list`)

| Keys | Action | Id |
|---|---|---|
| `j` | Next row | `list.next` |
| `Down` | Next row | `list.next` |
| `k` | Previous row | `list.prev` |
| `Up` | Previous row | `list.prev` |
| `g g` | First row | `list.first` |
| `G` | Last row | `list.last` |
| `Enter` | Open the conversation, or fold the section | `list.open` |

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
| `:list` |  | Show or hide the list panel |
| `:home` |  | Show Home |
| `:dms` |  | Show DMs |
| `:activity` |  | Show Activity |
| `:files` |  | Show Files |
| `:later` |  | Show Later |

## Keys that need the kitty keyboard protocol or Option as Alt

Without the kitty keyboard protocol a terminal sends `Ctrl+I` as `Tab`, `Ctrl+M` as `Enter`, `Ctrl+[` as `Esc`, and on some terminals `Backspace` as `Ctrl+H`; on macOS, `Alt` keys need "Option as Alt" (Ghostty: `macos-option-as-alt = true`). Each of these keys has another key that works everywhere.

| Keys | Action | Also |
|---|---|---|
| `Ctrl+H` | Move the focus left | `Space w h` |
