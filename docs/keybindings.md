# slakio key bindings

<!-- Generated from crates/slakio-tui/src/keymap.rs and the action registry. Do not edit;
     run `SLAKIO_BLESS=1 cargo test -p slakio-tui --test keybindings_doc` at the repository root. -->

The keys of *Always* work everywhere, also while typing. Other keys resolve in the current context first, then in its parents. A text input context (marked **[text]**) has no parent: every key it does not bind is typed. A modal context (marked **[modal]**) has no parent either: only its own keys work there. `?`, `F1` or `Space ?` show these keys in the app; after `Space` (or another first key of a sequence) a popup lists what may follow.

## Always (also while typing) (`global`)

| Keys | Action | Id |
|---|---|---|
| `Ctrl+Q` | Quit | `app.quit` |
| `F1` | Keyboard help | `help.open` |
| `Ctrl+P` | Command palette: every command and action by name, with its keys | `palette.open` |

## Everywhere (`root`)

| Keys | Action | Id |
|---|---|---|
| `:` | Open the command line | `cmdline.open` |
| `?` | Keyboard help | `help.open` |
| `Ctrl+C` | Cancel what is pending (Ctrl+Q quits) | `app.interrupt` |

## Main screen (`shell`)

| Keys | Action | Id |
|---|---|---|
| `Tab` | Next panel | `focus.next` |
| `F6` | Next panel | `focus.next` |
| `Shift+Tab` | Previous panel | `focus.prev` |
| `Shift+F6` | Previous panel | `focus.prev` |
| `Ctrl+R` | Rail: workspaces and views | `rail.focus` |
| `Space r` | Rail: workspaces and views | `rail.focus` |
| `Ctrl+H` | Move the focus left | `focus.left` |
| `Ctrl+J` | Move the focus down | `focus.down` |
| `Ctrl+K` | Move the focus up | `focus.up` |
| `Ctrl+L` | Move the focus right | `focus.right` |
| `Space w h` | Move the focus left | `focus.left` |
| `Space w j` | Move the focus down | `focus.down` |
| `Space w k` | Move the focus up | `focus.up` |
| `Space w l` | Move the focus right | `focus.right` |
| `Space w c` | Close the pane, then its tab | `pane.close` |
| `Space h` | Show Home | `view.home` |
| `Space d` | Show DMs | `view.dms` |
| `Space a` | Show Activity | `view.activity` |
| `Space f` | Show Files | `view.files` |
| `Space l` | Show Later | `view.later` |
| `Space e` | Show or hide the list panel | `list.toggle_panel` |
| `Space W` | Switch workspace | `workspace.choose` |
| `Ctrl+O` | Back to the conversation before | `history.back` |
| `Alt+Left` | Back to the conversation before | `history.back` |
| `Space [` | Back to the conversation before | `history.back` |
| `Ctrl+I` | Forward again | `history.forward` |
| `Alt+Right` | Forward again | `history.forward` |
| `Space ]` | Forward again | `history.forward` |
| `g t` | Next tab | `tab.next` |
| `g T` | Previous tab | `tab.prev` |
| `Ctrl+PageDown` | Next tab | `tab.next` |
| `Ctrl+PageUp` | Previous tab | `tab.prev` |
| `Space 1` | Tab 1 | `tab.go.1` |
| `Space 2` | Tab 2 | `tab.go.2` |
| `Space 3` | Tab 3 | `tab.go.3` |
| `Space 4` | Tab 4 | `tab.go.4` |
| `Space 5` | Tab 5 | `tab.go.5` |
| `Space 6` | Tab 6 | `tab.go.6` |
| `Space 7` | Tab 7 | `tab.go.7` |
| `Space 8` | Tab 8 | `tab.go.8` |
| `Space 9` | Tab 9 | `tab.go.9` |
| `Alt+1` | Tab 1 | `tab.go.1` |
| `Alt+2` | Tab 2 | `tab.go.2` |
| `Alt+3` | Tab 3 | `tab.go.3` |
| `Alt+4` | Tab 4 | `tab.go.4` |
| `Alt+5` | Tab 5 | `tab.go.5` |
| `Alt+6` | Tab 6 | `tab.go.6` |
| `Alt+7` | Tab 7 | `tab.go.7` |
| `Alt+8` | Tab 8 | `tab.go.8` |
| `Alt+9` | Tab 9 | `tab.go.9` |
| `Space t n` | Open in a new tab (a message: its thread) | `tab.open` |
| `Space t c` | Close the tab | `tab.close` |
| `Space t u` | Reopen the tab closed last | `tab.reopen` |
| `Space t r` | Rename the tab | `tab.rename` |
| `Space t h` | Move the tab left | `tab.move_left` |
| `Space t l` | Move the tab right | `tab.move_right` |
| `Space ?` | Keyboard help | `help.open` |
| `Space /` | Open the command line | `cmdline.open` |
| `Space q` | Quit | `app.quit` |

## Rail (`rail`)

| Keys | Action | Id |
|---|---|---|
| `j` | Next rail item | `rail.next` |
| `Down` | Next rail item | `rail.next` |
| `k` | Previous rail item | `rail.prev` |
| `Up` | Previous rail item | `rail.prev` |
| `g g` | First rail item | `rail.first` |
| `Home` | First rail item | `rail.first` |
| `G` | Last rail item | `rail.last` |
| `End` | Last rail item | `rail.last` |
| `Enter` | Show the rail item | `rail.select` |
| `Esc` | Back to the list | `rail.leave` |
| `l` | Back to the list | `rail.leave` |
| `Right` | Back to the list | `rail.leave` |

## List panel (`list`)

| Keys | Action | Id |
|---|---|---|
| `j` | Next row | `list.next` |
| `Down` | Next row | `list.next` |
| `k` | Previous row | `list.prev` |
| `Up` | Previous row | `list.prev` |
| `g g` | First row | `list.first` |
| `Home` | First row | `list.first` |
| `G` | Last row | `list.last` |
| `End` | Last row | `list.last` |
| `Ctrl+D` | Half a page down | `list.half_down` |
| `Ctrl+U` | Half a page up | `list.half_up` |
| `PageDown` | A page down | `list.page_down` |
| `PageUp` | A page up | `list.page_up` |
| `Enter` | Open the conversation, or fold the section | `list.open` |
| `l` | Open, staying in the list (or unfold) | `list.peek` |
| `Right` | Open, staying in the list (or unfold) | `list.peek` |
| `h` | To the section header, fold it, then the rail | `list.fold` |
| `Left` | To the section header, fold it, then the rail | `list.fold` |
| `{` | Previous section | `list.section_prev` |
| `}` | Next section | `list.section_next` |
| `t` | Open in a new tab (a message: its thread) | `tab.open` |

## Work area (Normal mode) (`pane.normal`)

| Keys | Action | Id |
|---|---|---|
| `j` | Next message (extends a VISUAL range) | `pane.next` |
| `Down` | Next message (extends a VISUAL range) | `pane.next` |
| `k` | Previous message (extends a VISUAL range) | `pane.prev` |
| `Up` | Previous message (extends a VISUAL range) | `pane.prev` |
| `g g` | Oldest message | `pane.first` |
| `Home` | Oldest message | `pane.first` |
| `G` | Newest message | `pane.last` |
| `End` | Newest message | `pane.last` |
| `Ctrl+D` | Half a page of messages down | `pane.half_down` |
| `Ctrl+U` | Half a page of messages up | `pane.half_up` |
| `PageDown` | A page of messages down | `pane.page_down` |
| `PageUp` | A page of messages up | `pane.page_up` |
| `Enter` | Open the thread (with none selected: write) | `pane.open_thread` |
| `i` | Write in the composer | `pane.insert` |
| `a` | Write in the composer | `pane.insert` |
| `y` | Copy the selected messages | `pane.copy` |
| `V` | Select a range of messages (VISUAL) | `pane.visual` |
| `t` | Open in a new tab (a message: its thread) | `tab.open` |
| `Esc` | One step out (VISUAL, selection, thread, list) | `pane.escape` |
| `h` | The panel to the left | `pane.left` |
| `Left` | The panel to the left | `pane.left` |
| `l` | The thread panel | `pane.right` |
| `Right` | The thread panel | `pane.right` |
| `Ctrl+W` | Close the pane, then its tab | `pane.close` |

## Command line (`cmdline`) **[text]**

| Keys | Action | Id |
|---|---|---|
| `Enter` | Run the typed command | `cmdline.run` |
| `Esc` | Close the command line | `cmdline.cancel` |
| `Ctrl+C` | Close the command line | `cmdline.cancel` |
| `Down` | Next entry of the command list | `cmdline.next` |
| `Tab` | Next entry of the command list | `cmdline.next` |
| `Ctrl+N` | Next entry of the command list | `cmdline.next` |
| `Up` | Previous entry of the command list | `cmdline.prev` |
| `Shift+Tab` | Previous entry of the command list | `cmdline.prev` |

## Composer (Insert mode) (`composer.insert`) **[text]**

| Keys | Action | Id |
|---|---|---|
| `Enter` | Send | `composer.send` |
| `Alt+Enter` | New line | `composer.newline` |
| `Ctrl+J` | New line | `composer.newline` |
| `Shift+Enter` | New line | `composer.newline` |
| `Esc` | Stop writing (Normal mode) | `composer.leave` |
| `Ctrl+C` | Stop writing (Normal mode) | `composer.leave` |
| `Ctrl+W` | Delete the word before the cursor | `composer.delete_word` |
| `Ctrl+U` | Delete to the start of the line | `composer.delete_line` |
| `Ctrl+K` | Delete to the end of the line | `composer.delete_to_end` |

## Keyboard help (`help`) **[modal]**

| Keys | Action | Id |
|---|---|---|
| `j` | Next row | `help.next` |
| `Down` | Next row | `help.next` |
| `k` | Previous row | `help.prev` |
| `Up` | Previous row | `help.prev` |
| `PageDown` | A page down | `help.page_down` |
| `Ctrl+D` | A page down | `help.page_down` |
| `PageUp` | A page up | `help.page_up` |
| `Ctrl+U` | A page up | `help.page_up` |
| `g g` | First row | `help.first` |
| `Home` | First row | `help.first` |
| `G` | Last row | `help.last` |
| `End` | Last row | `help.last` |
| `Enter` | Run the key's action, or open the section | `help.run` |
| `l` | Open the section | `help.expand` |
| `Right` | Open the section | `help.expand` |
| `h` | Close the section | `help.collapse` |
| `Left` | Close the section | `help.collapse` |
| `/` | Search the keys | `help.search` |
| `Esc` | Close the help | `help.close` |
| `q` | Close the help | `help.close` |
| `?` | Close the help | `help.close` |
| `Ctrl+C` | Close the help | `help.close` |

## Keyboard help search (`help.filter`) **[text]**

| Keys | Action | Id |
|---|---|---|
| `Enter` | Stop typing the search | `help.search_done` |
| `Down` | Stop typing the search | `help.search_done` |
| `Esc` | Clear the search | `help.search_cancel` |
| `Ctrl+C` | Clear the search | `help.search_cancel` |

## Question (`dialog`) **[modal]**

| Keys | Action | Id |
|---|---|---|
| `y` | Yes | `dialog.yes` |
| `n` | No | `dialog.no` |
| `Esc` | No | `dialog.no` |
| `Ctrl+C` | No | `dialog.no` |
| `Enter` | The answer with the focus | `dialog.choose` |
| `Tab` | The other answer | `dialog.toggle` |
| `Left` | The other answer | `dialog.toggle` |
| `Right` | The other answer | `dialog.toggle` |
| `h` | The other answer | `dialog.toggle` |
| `l` | The other answer | `dialog.toggle` |

## Commands

Type `:` and the command, then `Enter`.

| Command | Also | Action |
|---|---|---|
| `:qa` | `:qall` `:quitall` | Quit |
| `:workspace` |  | Switch workspace |
| `:help` |  | Keyboard help |
| `:avatars` |  | Show or hide avatars (initials) |
| `:rail` |  | Rail: workspaces and views |
| `:list` |  | Show or hide the list panel |
| `:home` |  | Show Home |
| `:dms` |  | Show DMs |
| `:activity` |  | Show Activity |
| `:files` |  | Show Files |
| `:later` |  | Show Later |
| `:q` | `:quit` `:close` | Close the pane, then its tab |
| `:back` |  | Back to the conversation before |
| `:forward` |  | Forward again |
| `:tabnext` | `:tabn` | Next tab |
| `:tabprevious` | `:tabp` | Previous tab |
| `:tabclose` | `:tabc` | Close the tab |
| `:reopen` |  | Reopen the tab closed last |
| `:rename` |  | Rename the tab |

## Keys that need the kitty keyboard protocol or Option as Alt

Without the kitty keyboard protocol a terminal sends `Ctrl+I` as `Tab`, `Ctrl+M` as `Enter`, `Ctrl+[` as `Esc`, and on some terminals `Backspace` as `Ctrl+H`; on macOS, `Alt` keys need "Option as Alt" (Ghostty: `macos-option-as-alt = true`). Each of these keys has another key that works everywhere.

| Keys | Action | Also |
|---|---|---|
| `Ctrl+H` | Move the focus left | `Space w h` |
| `Alt+Left` | Back to the conversation before | `Ctrl+O` `Space [` |
| `Ctrl+I` (bound only with the kitty keyboard protocol) | Forward again | `Space ]` |
| `Alt+Right` | Forward again | `Space ]` |
| `Alt+1` | Tab 1 | `Space 1` |
| `Alt+2` | Tab 2 | `Space 2` |
| `Alt+3` | Tab 3 | `Space 3` |
| `Alt+4` | Tab 4 | `Space 4` |
| `Alt+5` | Tab 5 | `Space 5` |
| `Alt+6` | Tab 6 | `Space 6` |
| `Alt+7` | Tab 7 | `Space 7` |
| `Alt+8` | Tab 8 | `Space 8` |
| `Alt+9` | Tab 9 | `Space 9` |
| `Alt+Enter` | New line | `Ctrl+J` `Shift+Enter` |
