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
| `Ctrl+O` | Back to the conversation before | `history.back` |
| `Alt+Left` | Back to the conversation before | `history.back` |
| `Ctrl+I` | Forward again | `history.forward` |
| `Tab` | Forward again | `history.forward` |
| `Alt+Right` | Forward again | `history.forward` |

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

## Work area (Normal mode) (`pane.normal`)

| Keys | Action | Id |
|---|---|---|
| `j` | Next message (extends a VISUAL range) | `pane.next` |
| `Down` | Next message (extends a VISUAL range) | `pane.next` |
| `k` | Previous message (extends a VISUAL range) | `pane.prev` |
| `Up` | Previous message (extends a VISUAL range) | `pane.prev` |
| `g g` | Oldest message | `pane.first` |
| `G` | Newest message | `pane.last` |
| `Enter` | Open the thread in the thread panel | `pane.open_thread` |
| `V` | Select a range of messages (VISUAL) | `pane.visual` |
| `y` | Copy the selected messages | `pane.copy` |
| `i` | Write in the composer | `pane.insert` |
| `Ctrl+W` | Close the pane | `pane.close` |

## A range of messages (VISUAL) (`pane.visual`)

| Keys | Action | Id |
|---|---|---|
| `Esc` | Leave VISUAL | `pane.escape` |

## Command line (`cmdline`) **[text]**

| Keys | Action | Id |
|---|---|---|
| `Enter` | Run the typed command | `cmdline.run` |
| `Esc` | Close the command line | `cmdline.cancel` |

## Composer (Insert mode) (`composer.insert`) **[text]**

| Keys | Action | Id |
|---|---|---|
| `Enter` | Send | `composer.send` |
| `Shift+Enter` | New line | `composer.newline` |
| `Alt+Enter` | New line | `composer.newline` |
| `Ctrl+J` | New line | `composer.newline` |
| `Esc` | Stop writing (Normal mode) | `composer.leave` |
| `Ctrl+W` | Delete the word before the cursor | `composer.delete_word` |
| `Ctrl+U` | Delete to the start of the line | `composer.delete_line` |

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
| `:close` |  | Close the pane |
| `:back` |  | Back to the conversation before |
| `:forward` |  | Forward again |

## Keys that need the kitty keyboard protocol or Option as Alt

Without the kitty keyboard protocol a terminal sends `Ctrl+I` as `Tab`, `Ctrl+M` as `Enter`, `Ctrl+[` as `Esc`, and on some terminals `Backspace` as `Ctrl+H`; on macOS, `Alt` keys need "Option as Alt" (Ghostty: `macos-option-as-alt = true`). Each of these keys has another key that works everywhere.

| Keys | Action | Also |
|---|---|---|
| `Ctrl+H` | Move the focus left | `Space w h` |
| `Alt+Left` | Back to the conversation before | `Ctrl+O` |
| `Ctrl+I` | Forward again | `Tab` |
| `Alt+Right` | Forward again | `Tab` |
| `Alt+Enter` | New line | `Shift+Enter` `Ctrl+J` |
