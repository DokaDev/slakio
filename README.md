# slakio

An unofficial terminal client for Slack, written in Rust with [Ratatui](https://ratatui.rs).

> **Unofficial.** slakio is an independent open-source project. It is not affiliated with,
> endorsed by, or sponsored by Slack Technologies, LLC or Salesforce. "Slack" is a trademark of
> its owner and is used here only to say which service the client is meant to work with.

## Status

**Pre-alpha, early development. Read-only.**

slakio signs in to your Slack workspaces with your own session ([Sign in](#sign-in))
and shows its channels and DMs with their unread marks (muted ones faint: a muted channel
shows only its mentions of you, a muted DM nothing); opening a conversation shows its newest
messages, older ones as you scroll up, and a message's thread when you open it (a thread of
more than 3,000 replies shows its first 3,000, and says so). Messages show Slack's formatting (bold, italic, strike, code and code
blocks, quotes, links, people and channels by name, emoji); a bot's sections and attachments
show as text, and a bot's or app's message is marked `app`. Channels shared with another
organization and people and bots from it are marked `ext`,
and people in do not disturb `◐` (the status line shows it when it is you). It only
reads: it sends no message, reacts to nothing and marks nothing as read. Messages posted,
edited and deleted show as they happen in the conversations and threads open, and the unread
counts follow (live updates, over one connection per workspace that only receives; the status
line says when it drops and comes back, and the conversations and threads open and the unread
counts are read again then, also after the computer slept);
reactions show when a conversation is opened again, and who is typing or online is not shown.
The files a message shares show as cards and can be downloaded ([What works](#what-works));
in kitty and Ghostty, pictures show as previews under their cards and people can be pictured
by their profile photos ([Pictures](#pictures)).
What it fetched is kept in a local
cache ([Local cache](#local-cache)), so the next start shows it at once and then refreshes. The rest of the interface runs on invented data
(`slakio --demo`). Releases have binaries for macOS, Linux and Windows and a Homebrew formula
([Install](#install)); 0.0.26 makes the sidebar follow Slack's own sections and order (Starred, your custom sections, External connections, Apps, Channels, Direct messages), with group direct messages shown and closed ones hidden. Section names are localised, each section can be sorted alphabetically or by recent activity, folds are saved, and sections update live. Photos now show for people you never chatted with, initials stay for people without a custom photo, bots get their icon with an app mark, profile-photo changes appear live, and photos no longer wait behind the sidebar (one download per photo). 0.0.25 showed the photos in the direct-message list at launch, without opening each conversation; cached photos appeared instantly on later launches, and photos for conversations below the fold were fetched quietly in the background at a low rate. 0.0.24 drew custom emoji whose names are in any script (Korean and others) and played animated GIF and APNG emoji (the `animate` setting), kept photos and emoji in a disk cache so a second start was instant, showed your own photo, emoji and default avatars faster, scrolled only the view with the mouse wheel, and added opt-in picture diagnostics (`SLAKIO_DEBUG_PICTURES=1`); the binary budget became 10 MiB. 0.0.23 showed your own profile photo, Slack's default avatars (a redirect is followed only to an allowed avatar host) and the photos of people in the direct-message rows of the list. A workspace's own custom emoji were drawn as small pictures in text and reactions (fetched from Slack's emoji host only, at most 32 per screen), and pictures got a request share of their own. It also fixed some contrast and settings screen details. It is still read-only. 0.0.22 added a settings screen (`Space ,`) with readable labels and a live theme preview, and custom theme files (`themes/<name>.toml` with `extends`, `[colors]` and `[styles]`, strict validation that reports path:line, and a fallback to a built-in theme). `:theme copy`, `:theme new` and `:theme update` wrote and refreshed those files. `?` help listed more commands, Korean (Hangul) could be typed in the settings inputs, the binary budget became 9.5 MiB, and many review fixes went in. 0.0.21 showed picture previews under messages and people's profile photos, using the kitty graphics protocol (kitty and Ghostty; other terminals and terminals inside tmux, screen, Zellij or herdr keep text cards and initials). `:images auto|on|off` and `:avatars image|initials` switch them at once and are saved. Profile photos come from Slack's avatar host and Gravatar (no cookie or token is sent there), and the binary budget is now 9.0 MiB. 0.0.21 also fixes pictures lingering under popups, a picture decoder crash that could leave the terminal broken, a picture that trickles in forever, and saved settings that were not private. It is still read-only. 0.0.20 showed file cards under messages; `f` downloads a file (with several, you pick one) into the downloads folder (`~/Downloads/slakio` by default), with a standing progress line and `F` to stop. It never overwrites a file, and downloaded files are marked as coming from the internet (macOS quarantine, Windows Mark of the Web). Only viewer kinds (PDF, images, audio, video, text) can be opened, after a yes; office files, CSV, scripts and apps are only saved. 0.0.19 showed real Slack data in Activity: mentions, thread replies, reactions and direct messages of all signed-in workspaces in one list; Enter opened the message, or the reply's thread, and live mentions appeared at once. Nothing was ever marked as read. 0.0.18 searched messages (`/` or `Space s`, with Slack's own search words; Enter opened the message, or the reply thread), jumped to a day with `:jump <day>` (today, yesterday, 2026-11-01, -3d, weekdays; Korean words too), and fixed the time zone: all times and days were shown in UTC and are now shown in your local time zone, and `SLAKIO_TZ` overrides it. 0.0.17 opened Slack message, reply and channel links inside slakio (it jumped to the message, read around it if needed, and showed the thread beside it for replies; links for other workspaces offered the browser), and fixed a restored split so that it loaded every pane (0.0.16 loaded only the last one). 0.0.16 resized panes (`Space w >` / `<` / `+` / `-`, or a border
dragged with the mouse); neighbouring panes share one border, and the layout (tabs, splits, what each pane shows, zoom,
focus) comes back after a restart (`restore = "off"` stops that). 0.0.15 split the screen into panes: `v` splits beside, `s` splits under, Ctrl+H/J/K/L move between
panes, `z` zooms one pane, `Space w =` gives them even room, and Ctrl+W closes one. A thread panel splits off into its own
pane. The lowercase `v` now splits; the VISUAL selection is `V`. It is still
read-only: it sends nothing and marks nothing as read. While live is on, your coworkers may see you as active in Slack.
0.0.14 let you pick a message's link: `o` opened the message's only link, or drew letters on
each web link of the message to pick one (like tmux or vimium; a list when the links were off screen), and a mouse
click opened a link. `:open N` was removed (`:open <address>` stayed), and links still asked first when the text named
another host.
0.0.13 opened a message's web link in the browser: `o` or `:open [N|address]`
(http and https only; it asked first when the link's text named another host, and then showed the real host first).
The error log became redacted, private and size-capped, and the sign-out notice kept `:login` visible.
0.0.12 handled Slack signing a workspace out while slakio ran: that workspace stopped sending
and showed `signed out, :login`, and `:login` signed it in again without quitting, keeping your tabs, panes and
drafts (when another user signed in, the old view was cleared), and the refusals of `:net on` became clearer.
0.0.11 reconnected right after the computer woke from sleep, read the unread counts again and caught up the threads
that were open after a reconnect, and had `:net off` and `:net on` to stop or resume all network while it ran
(`:net on` could not turn the network on in a run that started offline). 0.0.10 showed
messages as they happen: new, edited and deleted messages, unread
counts and thread reply counts followed live, over a connection per workspace that only receives. When
the connection dropped the status line showed a `live off` marker, and after it came back the
conversations that were open were read again to catch up. `live = "off"` in the config turned it off.
0.0.10 sent nothing and marked nothing as read. 0.0.9 read real history (one page of older messages per `gg` or scroll, paced to
protect the request budget) and thread replies, showed Slack's formatting (bold, code, quotes and links,
with the link's real host shown), honoured mute (muted channels show only mentions, muted direct
messages nothing) and do not disturb, marked Slack Connect people `ext` and bots `app`, and cached
threads, without realtime updates. 0.0.8 signed in to several workspaces at once (from the Slack desktop app, or a
pasted request; the desktop picker lets you tick several with Space, signing in keeps the others,
and `logout <workspace>` or `logout --all` signs out) and reads them; one failing workspace does
not affect the others. It was read-only without realtime updates; it kept a local cache
per workspace, so slakio starts from the last run's data and then refreshes it (the cache is
excluded from Time Machine, and logout deletes it); 0.0.7 signed in to one workspace, 0.0.6 had no
cache, 0.0.5 signed in by paste only, and earlier releases
ran in demo mode only. Sign-in works on macOS only (the credentials go to the
macOS Keychain); it reads the session from the Slack desktop app, or you paste a browser request.
Nothing here is ready for daily use.

The plan, in order: a user interface on fake data first, then read-only Slack connectivity,
then sending messages, drafts, the Activity view and search, reactions and emoji, files and
images, and settings. This README only lists what works; it will grow with each step.

## What works

- `slakio --version` and `slakio --help`.
- `slakio` opens full screen and restores the terminal on exit, on `SIGTERM`/`SIGHUP`, and on
  a crash. Not signed in, it says how to sign in; signed in, it shows the workspace's
  channels and DMs (unread and mention marks from Slack) and, for a conversation you open, its
  newest 28 messages, read-only. `slakio login` signs in (from the Slack desktop app on macOS,
  or `slakio login --paste` to paste a browser request; several workspaces can be signed in at
  once), `slakio logout [<workspace> | --all]` signs out,
  `slakio --offline` sends nothing. A start shows the last session's channels and the
  conversations opened from the local cache right away (`cached` in the status line until
  Slack has answered), without waiting for Slack.
- `slakio --demo` shows an invented world (two workspaces, sidebar sections, about 300
  channels, DMs, a channel shared with another organization), nothing of which connects
  anywhere:
  - the left column navigates, the right one is the work: the list panel's title is the
    workspace (its colour band, its name, `▾`, then each other workspace that wants you by its
    letter and mark, `· B @9`), its first rows the view switcher as in GUI Slack's sidebar — a
    row each for Home, DMs, Activity, Files and To-dos (Files says it comes in a
    later version; To-dos counts its to-dos in progress, in the warning colour when one is
    overdue), an icon (icons on) and the name, the count at the right, the view shown marked
    with an accent `▎` (bars are for cursors only) — `@3` mentions, `●16` the DMs' unread messages, `●` unread channels; red only where a mention is behind it, in the list, the tabs and the
    switcher alike (a DM's pill counts its unread messages, red when one mentions you); the
    status line repeats no count; `[` / `]` in the list show the view before or after;
    `Ctrl+R` or `Space r` (`:nav`) goes to the switcher from anywhere (and `Ctrl+R` back),
    `j` / `k` move down it and on into the list (and back up), `Enter` picks a view, `Esc`
    leaves; `Space v` (`:navrows`, or a click on `▾` on the rule under it) folds it to one row, the view
    shown (`▸`) with the strongest mark of the others after that view's icon or letter (`@24 · A @37`), and unfolds it; the workspace (or `Space W`, or a click on
    it) opens a switcher of the workspaces with their counts; when the list panel makes room
    for a thread on a narrow screen, the status line names the workspace and the view;
  - each DM shows whether its person is active (`●`), away (`○`) or in do not disturb (`◐`),
    in the list and in the conversation's title;
  - the list panel shows sections (fold with `Enter`), channels and DMs, unread ones in bold
    with mention counts; `Enter` or a click opens the conversation in the work area and moves
    there, `l` opens it and stays in the list; Home ends with Saved, the messages kept without
    a reminder;
  - To-dos (`Space o`, also `Space l` and `:later`, Later's names before Slack renamed it)
    lists a filter row, then In progress and Completed (each folds with `Enter`): each to-do
    shows its author's chip, its text and its reminder (`Overdue` in the warning colour, or `in
    20h`), then where and when it was written; `x` completes or reopens it, `m` moves it to
    Saved (and a saved message back to To-dos), `d` clears it after asking; `f` (`:filter`)
    filters by state or reminder (overdue, upcoming, no reminder), `s` (`:sort`) orders by due
    date or by when it was saved, and a click on the filter row opens either; `Enter` opens the
    message, selected (a thread's own message: its thread). The demo's to-dos are invented,
    like the rest of it;
  - the conversation shows its messages as in GUI Slack (`density = "comfortable"`): the
    sender's initials in a 4 × 2 block, the name and time over the text, a sender's messages
    within five minutes grouped under one header (their time shows when selected), one blank
    row between blocks and after reactions, reactions as emoji chips (Slack's shortcodes such as
    `:+1:` drawn as emoji, skin tones too, and the common names Slack spells its own way such as
    `:thinking_face:`; a workspace's own emoji show as small pictures where pictures are drawn
    ([Pictures](#pictures)), as the emoji an alias names, and otherwise stay `:name:`); `:density`
    switches to compact columns and saves it; date separators, the edited marker, "N replies"
    rows; older messages load a page at a time as you go up, paced so reading back never
    runs into Slack's limits (a page on its way, one that failed with `r` to retry, and the
    start of a conversation each show a row); `j` / `k` select messages, `g g` / `G` jump to
    the oldest loaded (asking for one more page) / newest (arrows, `PageUp` / `PageDown`, `Home` / `End` too; the
    mouse wheel scrolls the view three rows a notch and leaves the selection where it is,
    until a key moves it back into view; up at the top it loads older messages too), `Enter` opens the selected message's thread in a panel on the right (another
    thread replaces it; with no message selected it writes), `Ctrl+W` closes the panel, then
    the conversation; `Ctrl+O` / `Space [` and `Space ]` go back and forward between the
    conversations a pane showed;
  - tabs: `t` opens the conversation under the list's cursor, or the selected message's thread,
    in a new tab (what is open already, in any tab, is focused instead, also by `Enter`); the
    tab bar shows from two tabs on, with the unread count of the tabs not shown; `g t` / `g T`,
    `Ctrl+PageDown` / `Ctrl+PageUp`, `Space 1`…`9` (`Alt+1`…`9`) or a click switch tabs;
    `Ctrl+W` on a tab's last pane, `Space t c`, its `×` or a middle click close a tab (the last
    one leaves an empty work area, nothing quits) and `Space t u` opens the tabs closed this
    session again where they were; `Space t r`, `:rename <name>` or a double click rename one;
    `Space t h` / `Space t l` or a drag move one. The app never reorders tabs. Tabs and their
    names are kept across restarts (below);
  - splits, tmux style: `v` opens beside the pane and `s` under it (the selected message's
    thread, else the same conversation again; the thread panel splits off as a pane of its
    own, also when `v` opens another thread beside it, so all three show), `v` on the list
    opens the conversation under the cursor beside (`Space w v` /
    `Space w s`, `:vsplit` / `:split` too); as many panes as fit, at least 24×8 cells each (a
    split with no room left says so); `Ctrl+H/J/K/L` (`Space w h/j/k/l`) go to the pane on
    that side (the one last there first, so going and back returns), `Tab` / `F6` go round,
    a click focuses; `z` (`:zoom`) shows the pane alone and
    back (it stays zoomed while that pane has the keyboard; moving to another pane, a split
    or closing it shows every pane), `Space w >` / `<` / `+` / `-` make the pane wider,
    narrower, taller or shorter (`:resize >N`, `<N`, `+N`, `-N` by N cells), a border
    dragged with the mouse moves (pressed where two borders meet, the one that runs across
    moves), `Space w =` (`:equalize`) evens the room, `Ctrl+W` closes the
    pane (the keyboard goes to the pane that takes its room). Panes side by side share one
    border. Each pane keeps its own selection and history;
  - the tabs (their order, names and the one shown), each tab's splits and sizes, what each
    pane shows, zoom and the pane with the keyboard are saved in the state directory, one file
    per set of signed-in workspaces and users (private to you), and opened again at the next
    start (`restore = "off"` in the config turns this off), once the workspaces have answered:
    a start that fails (offline) keeps them waiting and the file as it was, and tabs you open
    meanwhile stay, the saved ones after them. A pane whose conversation Slack no longer lists
    is left out, and a file that cannot be read is not used (it is kept as `<name>.bak` when
    the layout is next saved); either is said once. Drafts are not saved: what you were
    writing is not there after a restart;
  - `V` selects a range of messages and `y` copies it (or the selected message) to the
    clipboard through the terminal (OSC 52);
  - `i` writes in the pane's composer (multiline: `Ctrl+J` or `Alt+Enter` for a new line,
    `Shift+Enter` with the kitty keyboard protocol; `Ctrl+W` deletes a word; Korean input
    works); `Enter` shows the message in the demo only, it is not sent anywhere; `Esc` goes
    back to Normal mode;
  - `Esc` steps out one level at a time (VISUAL, the selection, the thread panel, the main
    pane) and never closes anything;
  - with a Korean input source, Normal-mode keys still work (`j` typed as its jamo moves);
  - every name and message from the (invented) remote side is shown through a sanitiser that
    removes terminal escape sequences, control and bidi characters; the demo has a channel of
    hostile strings and a few hostile names to show it;
  - `Tab` / `Shift+Tab` go round the view switcher, the list, the main pane and the thread panel; `Ctrl+h` /
    `Ctrl+l` (or `Space w h` / `Space w l`) move left and right, the view switcher before the list; `Space h` /
    `Space d` show Home / DMs; `Space e` hides the list panel;
  - the status line shows the keys worth knowing where you are, and every key is listed in
    [docs/keybindings.md](docs/keybindings.md).
- `Ctrl+Q` quits from anywhere (it asks first when a message you wrote was not sent); so do
  `Space q` and `:qa`. `:q` closes like `Ctrl+W` (the pane, then its tab) and never quits.
- `Ctrl+P` (or `:`) opens the command palette: every command and action that works where you
  are, with its keys; type to filter (letters in order are enough: `thm` finds `:theme`),
  `Tab` / arrows to select, `Enter` to run, or click (a click on Quit asks first).
- `Space ,` (or `:settings`) opens the settings screen: every setting below with its value;
  `j`/`k` move, `h`/`l` change it at once and save it in the config file (comments kept);
  on the theme row `h`/`l` show each theme and `Enter` keeps it (`Esc` goes back), `c` copies
  it to a new theme file; on the downloads row `Enter` types a folder. `language`, `network`, `live`, `restore` and `cache`
  apply on the next launch, and the screen says so. `:set <key>=<value>` changes the same
  settings from the command palette; each row has a name of its own, and the selected one says its
  `:set` key.
- `:icons on|off` (or `:icons` to switch) draws Nerd Font icons or text instead, while running,
  and saves it; the first run asks once, showing the icons.
- People are pictured by their initials on a colour of their own: before a sender's name, before
  a DM in the list (with whether they are around at its corner) and in a DM's title;
  `:avatars` turns them off and on and saves it; `:avatars image` pictures people by their
  profile photos where pictures are drawn ([Pictures](#pictures)), and by initials elsewhere.
- Themes: the terminal's own colours, `dark`, `light`, `high-contrast`, `nord`, `dracula`, and
  `catppuccin`, `tokyo-night` and `gruvbox` (each its light or dark variant by the terminal's
  background); `:theme <name>` switches while running and saves it; `NO_COLOR=1` draws without
  colours. Your own theme: a file in `themes/` next to the config file
  ([Theme files](#theme-files)).
- Links: `o` on a selected message opens its link in your browser. With several links, each
  one that opens gets a letter or two drawn over its start (`a`, `s`, `d` …, never `j`, `k` or
  `o`): type them to open that one (any case; `Backspace` takes a letter back), `Esc` to stop.
  `o`, `j` or `k` show them as a list (host first; `j`/`k` move, `Enter` opens, the letters
  work there too), which also shows at once when one of them is off screen. A click on a link
  opens it too (a click just after the messages moved only selects), and `:open <address>`
  opens one you type. Only
  `http` and `https` addresses open (other links get no letter and are not drawn as links);
  when a link's text names another host than the one it goes to, slakio shows the real host
  and asks first.
  Nothing is sent to Slack for it, so it works in read-only mode too.
- Files: the files a message shares show as cards under its text (name, kind, size; a file
  hosted elsewhere, such as a linked document, says so). `f` (or `:download`) on a selected
  message downloads its file, or lists its files to pick one (letters, `j`/`k` and `Enter`, a
  click, `Esc`). It is saved in `Downloads/slakio` in your home folder (`downloads` in the
  config names another; `download_max_mb` caps the size, 1024 by default) under a safe name
  (no folders, no hidden or reserved names, never over a file there: `name (2).pdf`), and the
  status line shows how much has come; `F` (or `:download cancel`) stops it, and nothing of it
  is kept. A folder slakio makes for downloads, and the files it saves, are readable by you
  alone. On macOS and Windows it is marked as downloaded from the internet before it gets its
  name, as a browser marks its downloads (if that fails it is kept, but not offered to open). A
  PDF, picture, sound, video or plain text file is offered to open in your system's app for it
  once saved (it asks; the safe answer is "Not now"); office documents, tables (`.csv`),
  programs, scripts, installers, archives and anything else are only saved. `--demo` saves its
  invented files in a temporary folder (`slakio-demo`), never offered to open. Downloading reads
  only: the file comes from Slack's file host (the `files` host of `slack.com`) alone, which is sent your
  session cookie and never your token, and redirects to any other host are refused. A picture's
  card has a small preview of it under it where pictures are drawn ([Pictures](#pictures)).
- Slack's own links (a message's, a reply's, a channel's, `…slack.com/archives/…`) open inside
  slakio instead (marked `↳` in the list of links): the conversation opens, or the pane showing
  it is focused, and the message is selected and marked for a moment; a message not loaded is
  read with the ones before it (and the newer ones when they are few; else the pane says they
  are not loaded and `G` reads the newest). A reply's link opens its thread beside the
  conversation. A link opens only in the workspace it names (an Enterprise Grid link in the
  one of yours that has the conversation). A link to a workspace you have not signed in to, or
  a conversation slakio cannot see there, asks whether to open it in the browser instead.
- Search (`/`, `Space s`, or `:search <words>`): a popup searches the messages of the
  workspace you are in (`Tab` the next one) with Slack's own search words (`in:#channel`,
  `from:@name`, `"exact words"`, `before:` / `after:` a date), newest first, the words found
  underlined. It searches when you press `Enter` or stop typing for a moment, never per key;
  `PageDown` / `PageUp` show the next or previous page, and `Enter` on a result opens it as a
  Slack link opens (a reply's thread beside its conversation; a reply is marked `⤷`). `Esc`
  closes it; the words and results stay for next time (typing replaces the words). Searching
  reads only; it is Slack's `search.messages`, one request per page you ask for, at most one a
  second.
- Activity (`Space a`): your mentions (by name, `@here` / `@channel` / `@everyone`, a group of
  yours, a word you follow), new replies in threads you follow, reactions to your messages and
  DMs, of every workspace you are signed in to, newest first (each workspace's colour stripe at
  the left when there are several). Each notice shows its mark (`@` you, `!` everyone, `&` a
  group, `*` a word, `⤷` a thread, `+` a reaction, `»` a DM), where and when, then who and the
  message's text; unread ones are bold, as Slack counts them. `Enter` opens the message as a
  Slack link opens (a reply's thread beside its conversation), `l` opens it staying in the
  list; older notices load as you near the end. It is read when it shows (Slack's
  `activity.feed`, and `messages.list` for the texts: a few requests a page), not again within
  30 seconds; a mention that comes live is listed at once and Activity read again shortly after
  (also after the live updates come back). Nothing is marked read; `r` asks again for a page
  that could not be loaded.
- `:jump <day>` (or `Space j`) reads the conversation you are in from a day: `today`,
  `yesterday`, `2026-11-01`, `11-01` (month-day), `-3d`, `-2w` or a weekday (`monday`); with
  `language = "ko"` also the Korean words for today, yesterday, the day before and the
  weekdays. A day still to come is refused; when nothing was written on the day or after it,
  the newest message is shown and the status line says so. Its first message is selected and
  marked; reading back from there (`g g`, scrolling up) reaches older ones, and `G` the newest.
- Times and days (beside messages, the date rows between them, search results, `:jump`) are in
  your local time zone: `SLAKIO_TZ` (an IANA name such as `Asia/Seoul`, or a POSIX rule such as
  `KST-9`) when set, else `TZ`, else the system's, else UTC. It is read once, at the start.
- Failures are written to `errors.log` in the state directory (`$SLAKIO_STATE_DIR`, else
  `$XDG_STATE_HOME/slakio`, or the platform's equivalent), readable by you only. It never holds
  a token, a cookie, a request's address past its host or message text, and it is kept small:
  past 1 MiB it is renamed `errors.log.1` (replacing the one before) and a new one starts.

### Pictures

Where the terminal surely draws pictures, a picture a message shares (PNG, JPEG or GIF; a GIF
shows its first frame) has a preview under its card, at most 36 × 10 cells, and with
`avatars = "image"` (or `:avatars image`) the comfortable layout shows people's profile photos
in place of their initials, and each DM in the list shows its peer's photo, small, in the two
cells of their initials (group DMs keep how many people are in them; only the rows on screen
fetch their photos, and a photo shown in both places is fetched for each). Elsewhere, and
until a picture has come, the text card and the initials show as before; a picture that cannot
be shown says why on its card (too large, no preview, not loaded). A workspace's own emoji show as small pictures, two cells wide, in a
message's text and in its reaction chips (a line never breaks inside one); a GIF or animated PNG
emoji or profile photo moves (at most 8 on screen at once, at most 10 frames a second, 64 frames, from
the bytes already fetched; `animate = "off"` or `:set animate=off` keeps the first frame), and a
file's preview shows a GIF's first frame; where pictures are not drawn, until one has come, or when it cannot be fetched,
`:name:` shows as before. Nothing of it ever blocks the screen: pictures are fetched in the
background and decoded off the screen's thread.

- Terminals: kitty and Ghostty (the kitty graphics protocol, drawn as ordinary cells, so a
  popup, a split, scrolling or a closed pane never leaves a picture behind). slakio tells them
  from `TERM` (`xterm-kitty`, `xterm-ghostty`, or `TERM_PROGRAM=ghostty` with `TERM` `xterm` or
  `xterm-256color`), and never asks the terminal. Inside a multiplexer (tmux, screen, Zellij,
  herdr, dvtm), Neovim's or Emacs' terminal, or any other terminal it draws text, unless told
  otherwise.
- `images = "auto"` (the default) in the config, `"on"` to send pictures whatever slakio can
  tell (inside tmux, screen or Zellij they will not show: slakio does not wrap them for a
  multiplexer), or `"off"` never to fetch or draw them. `:images auto|on|off` changes it while slakio runs (off
  removes every picture from the screen at once; `:images` alone switches between off and
  auto) and saves it in the config file. `SLAKIO_IMAGES=auto|on|off` in the environment wins
  over both (slakio says so when you choose otherwise), and `:images on` says when slakio cannot
  tell that the terminal shows pictures. `slakio --demo` draws its invented pictures only with
  `SLAKIO_IMAGES=on` (or `images = "on"`).
- What is fetched: the smaller copy Slack makes of a picture (never the original), from Slack's
  file host with your session cookie and never your token, as a file's download; and profile
  photos from Slack's avatar host (`avatars.slack` + `-edge.com`), `secure.gravatar.com`, or
  Slack's static host (`a.slack` + `-edge.com`, where Slack keeps the default avatars of people
  without a photo of their own), over `https` only, on those exact hosts (no other port, no
  user name in the address), with no cookie, no token, no `Origin` and no `Referer`. A photo
  follows at most 2 redirects, each only to one of those hosts and checked again, and each
  counted as a request; a redirect anywhere else is refused, and nothing is asked there. A
  Gravatar address is used only when it carries Gravatar's own settings alone and the fallback
  picture it names (`d=`) is Slack's own or one of Gravatar's built-in pictures. Your own photo shows too: it comes from your profile in
  Slack's start answer, or from the same people lookup slakio already makes. Only an answer that says
  it is an image is read, at most 2 MB of it; a picture larger than 4096 pixels a side or 4.2
  million pixels is refused from its header, before it is decoded. Each picture fetched counts
  as a background request against the request cap, and the network switches stop it. Pictures
  (photos, emoji and file previews together) take at most 30 requests a minute of a workspace,
  so live updates can always reconnect and catch up; a picture over that share shows as text or
  initials and is asked again a minute later. With
  `avatars = "image"`, people whose photo is on Gravatar are fetched from `secure.gravatar.com`,
  a service outside Slack: it sees your IP address and which photos you look at.
- A workspace's own emoji: the first time a message on screen shows a shortcode that is not a
  standard emoji, slakio reads the workspace's emoji list once (`emoji.list`, one background
  request; read again only after you sign in to the workspace again, and after a wait when it
  failed). An alias is followed at most 4 times (a loop of aliases stays `:name:`), and an alias
  of a standard emoji shows that emoji, with pictures on or off. Each emoji's picture is fetched
  only when it is on screen, from Slack's emoji host (`emoji.slack` + `-edge.com`) only, over
  `https`, on that exact host, with no cookie, no token, no `Origin` and no `Referer`, following
  at most 2 redirects, each only to that host and each counted as a request; an emoji whose
  picture is anywhere else stays `:name:`. They share the limits above.
- slakio keeps at most 64 pictures (24 MiB) in the terminal at once, the least recently shown
  going first, but never one on screen (a list photo only makes room for a timeline picture):
  past that, the rest keep their initials or `:name:` until the screen changes. The timeline's
  pictures are fetched before the list's photos. A screen draws at most 32 emoji pictures; more
  stay `:name:`. A person shown in both the timeline and the list is fetched at most twice, once
  per size.
- Pictures kept between runs: each profile photo and emoji picture slakio made for the terminal
  (a small PNG, at the size it is drawn; a moving one with all its frames) is kept in `pictures` in the cache directory
  ([Local cache](#local-cache)), so the next start draws it without fetching it (and without
  counting it against the shares above, also with the network off). A file's preview is never
  kept: it is a copy of a file of your workspace, fetched with your session each run. Photos
  are kept for up to 7 days, emoji for up to 30, at most 1 MiB a picture and 64 MiB in all (the
  least recently shown go first); the folder is made when slakio starts with pictures on (never
  while it runs, so a folder removed by `slakio logout` stays removed; started with pictures
  off, a folder already there is used once they are turned on), is readable by you only
  (`0700`, the files `0600`) and on macOS is excluded from Time Machine backups when it is made
  (the error log says when that fails); a file is named by a hash, never by its address or
  name, and a link or a damaged file in it is removed, never read. `cache = "off"` keeps none
  of them, `slakio --demo` writes none, every `slakio logout` removes the folder, and signing
  in to a workspace as someone else empties it. Deleting the folder is always safe; it only
  means fetching them again.
- When a picture does not show and you want to find out why, start slakio with
  `SLAKIO_DEBUG_PICTURES=1` (off by default). slakio then writes one line for each step a
  photo, an emoji, a file preview, the emoji list and your own photo went through to
  `pictures.log` in the state directory (`$SLAKIO_STATE_DIR`, else `$XDG_STATE_HOME/slakio`,
  else `~/Library/Application Support/slakio/state` on macOS, `~/.local/state/slakio` on Linux,
  `%LOCALAPPDATA%\slakio` on Windows), readable by you only and kept under 1 MiB (then
  `pictures.log.1`). It holds stages, counts, HTTP statuses and host names only: no address
  past its host, no token, cookie, name, id, emoji name or message text; a picture is told apart
  by a short hash that changes every run. With it on, slakio also reads the shape of your
  sidebar's sections once per workspace (field names and counts only) and nothing else changes.
  Have a look at the file before you share it, and delete it when you are done.

## Keys

Six keys get you everywhere:

| Key | What it does |
|---|---|
| `?` (or `F1`) | The keyboard help for where you are: every key, searchable, and `Enter` runs one |
| `Space` | Wait a moment: a popup lists what may follow (`Space h` Home, `Space d` DMs, …) |
| `Tab` | The next panel, the view switcher included (`Shift+Tab` the previous one) |
| `Ctrl+P` | The command palette: every command and action by name, with its keys |
| `Esc` | One step out; never closes anything |
| `Ctrl+Q` | Quit (also `Space q`, `:qa`; `:q` and `Ctrl+W` only close the pane, then its tab) |
- English and Korean interface text (`language = "auto" | "en" | "ko"` in the config file).

## Sign in

slakio signs in the way your browser is signed in, with your own session. On macOS it can read
that session straight from the Slack desktop app, so there is usually nothing to copy; otherwise
you paste one request the Slack web app sends. Nothing leaves your computer but slakio's own
read-only requests to your workspace.

### From the Slack desktop app (macOS)

If you use the Slack desktop app, run `slakio login`: it lists the workspaces the app is signed
in to, you pick one (or mark several with `Space`, then `Enter` signs in to each of them in
turn), and macOS asks whether `security` (the system tool slakio reads the key
with) may use the **"Slack Safe Storage"** key — choose **Allow**, not **Always Allow** (that
would let any program on the Mac read the key without asking). slakio reads the app's files (its cookie database and its stored session
tokens) and changes nothing; it never contacts Slack except the one request that checks the
sign-in. The desktop app may keep running. This reads the app's own data only when you run
`slakio login`; a development run (`SLAKIO_SECRET_STORE=memory`) never touches it.

### By pasting a request

Run `slakio login --paste` (or just `slakio login` when the desktop app is not found, or on
Linux and Windows):

1. Open your workspace in a browser (`https://<workspace>.slack.com`) and sign in as usual.
2. Open the developer tools (`Cmd+Option+I`), choose the **Network** tab and reload the page.
3. Right-click any request whose address has `/api/` and choose **Copy > Copy as cURL**.
4. Run `slakio login --paste` and paste it (or `pbpaste | slakio login`).

Signing in again from the desktop app to the workspace you are signed in to, while the app is
still in the same session, uses what slakio kept and asks for the "Slack Safe Storage" key only
if Slack no longer accepts it.

Either way, slakio checks the session token and the `d` cookie (and `d-s`) with one request to
Slack, shows the workspace's name and stores them in the **macOS Keychain** (the first time, and
after an update, macOS asks whether slakio may use the entry). `workspaces.toml` next to the
config file names the workspaces and holds no secret. Signing in to another workspace adds it:
the ones signed in to before stay, and the workspace switcher (`Space W`, or the workspace's
name at the top of the list) moves between them, each with its own unread marks. Each starts on
its own: one that Slack refuses or that does not answer shows "Could not load" in its own list
and leaves the others working. `slakio logout <workspace>` (its name, host or team id) signs out
of one and removes its credentials and its part of the local cache, keeping the others;
`slakio logout --all` signs out of every one (with one workspace, `slakio logout` alone does).
If Slack refuses a workspace at start, slakio says "run slakio login". If Slack signs a
workspace out while slakio runs, it stays on screen as it was, marked `signed out, :login`, and
nothing more is sent for it; `:login` signs in to it again without quitting (the desktop app's
picker or a paste; `Esc` goes back), and only that workspace starts over: tabs, panes and drafts
stay, what was posted meanwhile is read, and the other workspaces are not touched. Signed in
again as someone else, nothing of the user before stays on screen: that workspace's panes,
messages and drafts go.

The desktop-app import is adapted from the open-source client
[gammons/slk](https://github.com/gammons/slk) (MIT); see `NOTICE`.

## Local cache

slakio keeps what it fetched — the workspace's channels, people and sections, the newest
messages of the conversations you opened, and the replies of the threads you opened — in one
SQLite file in a directory of its own,
`slakio-store`, in the cache directory (`$SLAKIO_CACHE_DIR`, else `$XDG_CACHE_HOME/slakio`, else
`~/Library/Caches/slakio` on macOS, `~/.cache/slakio` on Linux, `%LOCALAPPDATA%\slakio\cache` on
Windows). A start draws from it without waiting for Slack (a cache slower than half a second
for the workspace, or a tenth of a second for a conversation's messages, is not waited for, and
the error log says so), then replaces what it showed with Slack's answer; with the network
off it shows the cache alone. No token, cookie or other secret is ever in it: the credentials
stay in the keychain. Each workspace has its own part of it, the signed-in user's only:
signing in to a workspace as someone else starts that workspace's part again from nothing, and
the other workspaces' parts are left as they are.

- It keeps at most 200 messages a conversation, 500 MB in all, and drops a conversation not
  opened for 30 days (a conversation open on screen is never dropped); `cache_messages`,
  `cache_max_mb` and `cache_days` change the limits, and `cache = "off"` keeps nothing between
  runs ([Configuration](#configuration)). The threads you opened are kept the same way, each
  with the same limits.
- Its directory is readable by you only (`0700`, the files `0600`); only that directory is
  changed or excluded, never the cache directory you named, and a symbolic link in its place or
  in place of one of its files is refused. On macOS it is excluded from Time Machine backups
  (`tmutil addexclusion`, which marks the directory itself); if that fails, slakio says so once
  and tries again on the next start.
- The pictures kept between runs (profile photos and emoji, never a file's preview) are beside
  it, in `pictures` in the same cache directory ([Pictures](#pictures)): readable by you only,
  excluded from Time Machine on macOS when the folder is made, at most 64 MiB, and removed by
  `slakio logout` too.
- A second slakio while one runs reads the cache but does not write it, and says so (the
  pictures kept are the exception: each slakio keeps the pictures it makes). A cache that
  cannot be read (damaged, or written by a newer slakio) is started again from nothing, with one
  line in the status line (shown after any other notice, so each is seen). `slakio logout`
  (of the last workspace, or `--all`) removes it, also when the keychain could not be changed,
  and says so; a slakio still running keeps showing the workspace until it is quit (logout says
  so when one runs). Signing out of one of several removes that workspace's part only, and not
  while a slakio runs (quit it first). Deleting the directory is always safe.

## Install

- **Homebrew** (macOS and Linux, arm64 and x86_64), in one line:

  ```sh
  brew install dokadev/tap/slakio
  ```

  or add the tap once and use the short name from then on:

  ```sh
  brew tap dokadev/tap
  brew install slakio
  brew upgrade slakio   # later, for a new release
  ```

  The tap installs stable releases only.
- **GitHub Releases**: download the archive for your platform from
  [Releases](https://github.com/DokaDev/slakio/releases) and put the `slakio` binary on your
  `PATH`. There are archives for macOS and Linux (arm64 and x86_64) and Windows (x86_64,
  untested); `SHA256SUMS` lists their checksums. Each archive also has the licenses and the
  third-party notices.

## Versioning

Releases follow [Semantic Versioning](https://semver.org), as `0.0.PATCH` while slakio is
early and read-only. The configuration format, key bindings and behavior may change in any
release before 1.0; the release notes say so.

## Build from source

Requires Rust 1.93 (pinned in `rust-toolchain.toml`; rustup installs it automatically).

```sh
git clone https://github.com/DokaDev/slakio
cd slakio
cargo run -p slakio-tui -- --demo
```

macOS is the primary platform. Linux and Windows are built and tested in CI but nobody has used
slakio on them yet.

## Configuration

`$XDG_CONFIG_HOME/slakio/config.toml`, else `~/.config/slakio/config.toml` (or `--config
<path>`):

```toml
language = "auto"        # "auto" (from LC_ALL / LC_MESSAGES / LANG), "en" or "ko"
theme = "auto"           # "auto", "terminal", "dark", "light", "high-contrast", "nord",
                         # "dracula", or a family: "catppuccin" (-latte / -mocha),
                         # "tokyo-night" (-day / -night), "gruvbox" (-light / -dark)
icons = "ask"            # Nerd Font icons: "on", "off", or "ask" once (true / false work too)
avatars = "initials"     # a person's initials on a coloured chip, or "off"
density = "comfortable"  # messages as in GUI Slack, or "compact" columns (`:density` switches)
nav_rows = "expanded"    # the view switcher a row per view, or "collapsed" (`Space v` switches)
network = "on"           # "off": nothing is ever sent to Slack (also SLAKIO_NETWORK=off)
live = "on"              # "off": no live updates connection (the rest of the network stays on)
cache = "on"             # "off": nothing is kept on disk between runs (every start fetches)
cache_messages = 200     # the most messages kept a conversation (10 to 10000)
cache_max_mb = 500       # the most megabytes the cache may use (10 to 100000)
cache_days = 30          # a conversation not opened for this many days is dropped (1 to 3650)
restore = "on"           # "off": the tabs and panes of the last run are not opened again
downloads = ""           # the folder files are saved in ("": Downloads/slakio in your home)
download_max_mb = 1024   # the largest file downloaded, in megabytes (1 to 100000)
images = "auto"          # pictures where the terminal surely draws them, "on" always, "off" never
```

`downloads` takes an absolute path or `~/…`; `SLAKIO_DOWNLOAD_DIR` in the environment (an
absolute path) overrides it. A megabyte here is 1,000,000 bytes, the unit file sizes are shown
in (a file shown as `1.0 GB` is over `download_max_mb = 999`).

`theme = "auto"` takes `tokyo-night` when the terminal says it shows 24-bit colour (`COLORTERM`
is `truecolor` or `24bit`), else the terminal's own colours. `:theme <name>` (also
`:colorscheme`, or `:set theme=<name>`) switches the theme while slakio runs and saves it here,
keeping your comments. `:avatars` (or `:set avatars=off`) does the same for `avatars`;
`avatars = "image"` shows profile photos where pictures are drawn, and initials elsewhere
([Pictures](#pictures)). With `icons = "ask"`, `slakio
--demo` asks once whether your font shows the icons (the view switcher previews the answer) and saves
the answer in the config file; `:icons on|off` changes it later. `rail_expand`, a setting of the
left rail (later the top bar, now the view switcher), is ignored (the status line says it can be removed).

`network = "off"` (or `SLAKIO_NETWORK=off` in the environment, or `slakio --offline`) is a kill
switch for the Slack connection: with any of them set, slakio sends nothing at all and opens no
live updates connection. While it runs, `:net off` does the same until `:net on` (the status
line shows `Offline` meanwhile); a run started with the network off stays off. Every request also goes through a request cap and an allowlist of
read-only methods; each opening of the live updates connection counts against the cap.

While the live updates connection is open, Slack may show you as active to your coworkers, as
it does for its own apps, even when slakio sits idle. `live = "off"` opens no such connection
(and so shows no live updates) while everything else keeps working.

A config file that cannot be used is never overwritten: slakio starts with the defaults and
says why in the status line.

### Theme files

`theme = "<name>"` (or `:theme <name>`) also takes a theme of your own,
`<config dir>/themes/<name>.toml`: a built-in theme it `extends`, with the colours and styles
you change. A file that cannot be used (an unknown token, a bad colour, a symbolic link) is not
fatal: slakio draws with `terminal`, says `Theme not used: <path>:<line>: <problem>` in the
status line and keeps the name in the config file. The file format, every token and its rules
are in [docs/themes.md](docs/themes.md). `:theme copy <from> [<name>]` writes a theme file with
every token of a theme (also `c` on the settings screen's theme row), `:theme new <name>` one
with every token commented out, and `:theme update [<name>]` adds the tokens a file does not list.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the gates every change passes, and
[docs/architecture.md](docs/architecture.md) for how the code is laid out.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
