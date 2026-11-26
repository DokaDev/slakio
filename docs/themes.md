# Theme files

Besides its built-in themes, slakio draws with a theme of your own: a small TOML file that
starts from a built-in theme and changes the colours and styles you name.

## Where it goes

A theme file is `<config dir>/themes/<name>.toml`, where `<config dir>` is the folder of the
config file (`$XDG_CONFIG_HOME/slakio/`, else `~/.config/slakio/`, or the folder of the file
given with `--config`). Use it with `theme = "<name>"` in the config file, or while slakio runs
with `:theme <name>` (also `:set theme=<name>`), which also saves it.

- The name is 1 to 64 ASCII letters, digits, `-` and `_`. It is used as written.
- A name of a built-in theme (`auto`, `terminal`, `dark`, `light`, `high-contrast`, `nord`,
  `dracula`, `catppuccin`, `catppuccin-latte`, `catppuccin-mocha`, `tokyo-night`,
  `tokyo-night-day`, `tokyo-night-night`, `gruvbox`, `gruvbox-light`, `gruvbox-dark`, in any
  case) always means the built-in theme; a file with such a name is never read.
- A theme file that is a symbolic link is refused ("is a symbolic link"). The `themes` folder
  itself, or the config folder, may be a symbolic link.
- A file larger than 64 KiB, or anything but a plain file, is refused.
- slakio creates the `themes` folder (only you may open it) and a file in it (only you may read
  it) only when you ask it to write a theme file ([Writing a theme file](#writing-a-theme-file)).
- slakio writes no theme file in a `themes` folder that is not yours or that others may write
  in (it says "is shared"); it still reads from one. Through a symbolic link it never changes
  the folder's permissions.

## The file

```toml
# themes/mine.toml, used with theme = "mine"
extends = "tokyo-night"

[colors]
accent = "#89b4fa"
warning = "yellow"
workspaces = ["#5b8def", "#4fb3a9", "#c792ea"]
dim = { toward = "#000000", keep = 40 }

[styles]
selection = { bg = "#2a3b4d", modifiers = ["bold"] }
link = { fg = "#7dcfff", modifiers = ["underlined"] }
```

`extends` names the built-in theme the file starts from, `terminal` without it. A family
(`catppuccin`, `tokyo-night`, `gruvbox`) takes its light or dark variant by the terminal's
background, as the built-in theme does; `auto` is not taken. Every token the file leaves out
keeps the value of the theme it extends.

The theme draws with 24-bit colour when the theme it extends does, or when the file names any
`#rrggbb` colour; otherwise with the terminal's 16 colours. `NO_COLOR` still draws without
colour, whatever the theme.

### Colours

A colour is `#rrggbb`, one of the 16 ANSI names (`black`, `red`, `green`, `yellow`, `blue`,
`magenta`, `cyan`, `white`, `bright-black`, `bright-red`, `bright-green`, `bright-yellow`,
`bright-blue`, `bright-magenta`, `bright-cyan`, `bright-white`), or `default` (the terminal's
own). Names are taken in any case.

### `[colors]`

| Token | What it colours |
|---|---|
| `bg` | The app's background. |
| `surface` | The status line and popups. |
| `surface_alt` | The background of a neutral avatar chip where the theme has no tint. |
| `border` | Borders of panels without the focus, dividers. |
| `accent` | The focused panel's border, links, prompts, the current workspace, keys in popups. |
| `accent_warm` | Key groups, a mention of you, words a search found. |
| `fg` | Body text. |
| `fg_muted` | Section headers, titles without the focus, hint labels. |
| `fg_dim` | Times, date lines, muted conversations, placeholders. |
| `success` | Success, and the mark of someone active. |
| `warning` | Warnings, the mark of do not disturb, the letters that pick a link. |
| `error` | Errors, and the background of the unread and mention counts. |
| `selection` | The background of the selected row of the focused panel. |
| `cursor_line` | The background of the selected row of a panel without the focus (24-bit themes). |
| `range` | The background of a VISUAL range. |
| `reaction` | The background of a reaction chip (24-bit themes). |
| `reaction_mine` | The background of your own reaction (24-bit themes): a light wash of the accent. |
| `mode_normal` | The mode badge in NORMAL. |
| `mode_insert` | The mode badge in INSERT. |
| `mode_visual` | The mode badge in VISUAL. |
| `mode_command` | The mode badge while the command palette is open. |
| `mode_fg` | The text of the mode badge and of the counts. |

A file that sets `bg` or `fg` but not `reaction` gets a chip of body text mixed 10% into the
background; one that sets `bg` or `accent` but not `reaction_mine` gets the least share of the
accent (from 10%) that sets the chip apart from the background (1.15:1) and from the selection
and the other chip. Where a colour is not `#rrggbb`, the chip of the theme it extends stays.

These four are slakio's own:

| Token | Value |
|---|---|
| `workspaces` | A list of 1 to 16 colours, taken by workspaces in sign-in order (wrapping round). A colour too close to `accent` is skipped, so a workspace's stripe never reads as a focus frame. Where a colour would read under 4.5:1 on `bg` as the workspace's letter on the list's title, the letter steps toward black or white until it does. |
| `avatars` | A list of 1 to 16 colours for the initials chips of people. |
| `avatar_tint` | A whole number from 0 to 100: the percent of a person's colour mixed into the background for their chip. `0` and `100` draw the colour solid under `mode_fg`. Where the initials would read under 4.5:1 on their chip (a tint close to 100, say), they step toward black or white until they do. It only changes the initials chips, never pictures. |
| `dim` | How the screen behind a dialog or the palette is dimmed: `"modifier"` (the terminal's dim attribute, colours kept), or `{ toward = "#rrggbb", keep = 0 to 100 }`: every colour blended toward `toward`, keeping `keep` percent of it. `toward` must be `#rrggbb`. |

### `[styles]`

A style is `{ fg = <colour>, bg = <colour>, modifiers = [...] }`, each part optional; what it
leaves out is not set. The modifiers are `bold`, `dim`, `italic`, `underlined`, `reversed` and
`crossed-out`.

Each style token replaces, whole, a style slakio otherwise works out from the colours. A token
left out keeps exactly the look of the theme it extends.

| Token | What it styles |
|---|---|
| `selection` | The selected row of the focused panel. Its `bg` replaces the bar's colour; its `fg` and modifiers go on the row's text. |
| `cursor_line` | The selected row of a panel without the focus, as `selection`. |
| `link` | Links in a message, and the "N replies" row of a message with a thread. |
| `mention` | A person, channel or group named in a message (also a mention of you). |
| `badge` | The count of mentions and unread DMs. |
| `current` | The current workspace or view. |
| `code` | Code in a message, inline or a block's lines. |
| `reaction` | A reaction chip. |
| `reaction_mine` | Your own reaction. |
| `tab_active` | The tab shown: its title, number and close button. |
| `tab_badge` | A tab's unread count. |
| `status_line` | The status line. |
| `hint` | The letters that pick a link (`o` on a message with several). |
| `search_match` | A word a search found, in a result's text. |

Pictures are drawn through cells whose colour says which picture they show, so a `selection`
or `cursor_line` style never changes those cells: its `fg` and modifiers (also `reversed`)
apply to text only, and the dimming behind a dialog leaves pictures as they are.

## Writing a theme file

- `:theme copy <from> [<name>]` writes `themes/<name>.toml` with every colour token of the theme
  `<from>` (a built-in theme, `auto` or a theme file; a family such as `catppuccin` as the
  variant drawn now) set to its value, each under a comment saying what it colours, and
  `extends` naming the built-in theme it starts from. Each style token is there too: set where
  `<from>` sets it, else commented out with the look it takes now (it follows the colours). The
  name is `<from>-copy` (then `-copy-2`, …) when you leave it out.
- `:theme new <name>` writes `themes/<name>.toml` extending the current theme when it is built in
  (`auto` as the theme it picks), else `terminal`, with every token commented out and showing the
  value it takes from there: remove the `# ` in front of a key to set it.
- On the settings screen (`Space ,`), `c` on the theme row copies the theme shown there (the one
  being previewed, else the current one): type the name (it starts as `<from>-copy`), `Enter`
  writes it, `Esc` cancels.

Each writes a new file and never replaces one: a name that is taken, that is not a valid theme
file name, that is a built-in theme's or a Windows device name such as `nul` (in any case) is
refused, and nothing is written. Then slakio draws with the new theme, saves
`theme = "<name>"` in the config file (unless that file has errors) and says where the file is.

`:theme update [<name>]` adds to the theme file `<name>` (the current theme without one) the
tokens it does not list, such as tokens added after it was written: each under what it is for,
at the end of its section, commented out with the value it takes now. No line of the file
changes and its line breaks (also Windows ones) are kept. The file is replaced, at once and only
yours to read, only when the result reads exactly as before and the file has not changed since
it was read. The theme drawn and the config file stay as they are.

## When a theme cannot be used

An unknown token, a style under `[colors]` (or a colour under `[styles]`), a bad colour,
modifier, list or number, an unknown `extends`, or a file that is not valid TOML is an error
with its line: `<path>:<line>: <problem>`.

- At launch, slakio draws with `terminal` and the status line says `Theme not used: <why>`. The
  config file keeps the name, so the theme comes back once the file is fixed.
- `:theme <name>` changes nothing and the palette says why. A name with no file says which
  built-in themes there are.
