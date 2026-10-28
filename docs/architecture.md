# Architecture

slakio is a Cargo workspace. This document says how the code is laid out and which rules hold
across it. It grows with the code; nothing here describes something that does not exist yet
unless it says so.

## Crates

| Crate | What | Depends on |
|---|---|---|
| `slakio-core` | UI-free and network-free core: the domain model, the backend protocol (commands and events tagged with a generation), the terminal-escape sanitiser, paths, atomic file writes, the config file, the secret store interface, faults, i18n | nothing of slakio |
| `slakio-world` | a seeded, deterministic fake world (two workspaces, ~300 channels, DMs, a Slack Connect channel, a 10k-message channel, a 1,200-reply thread, hostile strings) for the demo mode and the tests | `slakio-core` |
| `slakio-slack` | the Slack transport and backend, read-only by construction, with each workspace's live updates (a socket that only receives): the method allowlist, requests in the official web client's format, tolerant decoding, errors as data, the process-wide request cap, in-flight limit and kill switch (see [the Slack transport](#the-slack-transport)) | `slakio-core` |
| `slakio-auth` | signing in: a browser request pasted as cURL parsed for the workspace's address, the session token and the cookies; the OS keychain behind the secret store (macOS); `workspaces.toml`, the signed-in workspace without any secret (see [Signing in](#signing-in)) | `slakio-core` |
| `slakio-store` | the local cache: the last boot snapshot and the newest messages of the conversations opened, in one SQLite file (bundled SQLite), private to the user, one writer at a time, pruned to caps, set up again when unreadable; the cache in front of a backend (see [The local cache](#the-local-cache)) | `slakio-core` |
| `slakio-fake` | an in-process fake Slack server for the tests, seeded by `slakio-world`; it logs every request as received | `slakio-core`, `slakio-world` (dev-dependency only) |
| `slakio-tui` | the terminal UI (Ratatui) and the `slakio` binary | `slakio-core`; `slakio-slack`, `slakio-auth` and `slakio-store` in the binary's wiring only |
| `slakio-bench` | performance budgets (not product code) | runs the binary |

Planned crates join with the step that needs them: `slakio-scrub` (a tool that scrubs captured API traffic into test fixtures).

### Dependency rules

```
tui -> core <- slack, auth, store, world
```

- `slakio-core` depends on no other slakio crate and on no UI, network or database crate
  (ratatui, crossterm, tokio's network parts, HTTP or websocket clients, rusqlite).
- Adapters (`slack`, `auth`, `store`, `world`) depend only on `slakio-core`, never on each
  other or on `slakio-tui`. The binary puts the store in front of the Slack backend; neither
  knows of the other.
- `slakio-auth` never depends on `slakio-slack`, nor the reverse: sign-in yields a core
  `Credentials` value and the binary hands it to the transport.
- The concrete adapter types appear in one wiring file of `slakio-tui` (`src/main.rs`, checked:
  no other file of the crate names `slakio_slack`); the UI reaches a backend only through the
  core's protocol and `Capabilities`.
- `slakio-fake` is only ever a dev-dependency, and depends on `slakio-core` and `slakio-world`
  only.

CI checks these with `cargo tree` (`.github/scripts/dependency-direction.sh`).

## Module rules

- `foo.rs` with its submodules in `foo/`, never `mod.rs`.
- Unit tests in `foo/tests.rs` (`#[cfg(test)] mod tests;` in `foo.rs`); integration tests in
  the crate's `tests/`.
- Every file opens with a comment that says what it is for and why it is shaped that way.
- A source file has at most 600 lines and a function at most 80 (clippy's `too_many_lines`).
  The few longer ones are listed — `.github/scripts/file-size-allowlist.txt` with their size,
  `#[expect(clippy::too_many_lines)]` on the function — and may only shrink; the repository
  rules and clippy fail on a new one.
- A lint is silenced only with `#[expect(<lint>, reason = "…")]`, which fails once the lint no
  longer fires; clippy refuses `#[allow]` (`allow_attributes`, `allow_attributes_without_reason`),
  and the repository rules refuse a clippy lint silenced for a whole file or crate (`#![…]`):
  it goes on the one item that needs it.

## The UI: state, actions, effects

`App` (`crates/slakio-tui/src/app.rs`) is a thin router. Each part of the state is a sub-state
that owns its data and its update: the command line (`app/cmdline.rs`), the status line's
notices (`app/status.rs`), the shell — focus between the view switcher, the list panel and the work
area, the view switcher's and the list's cursors, folded sections (`app/shell.rs`) —, the work area — its panes by id, placed by
tabs of `slakio-core` (`layout/tabs.rs`: tabs in the user's order, never reordered by the app,
each a layout tree of `layout.rs` — the conversation, or split with the auto thread panel — and
its active pane), the tabs closed to open again
(`app/work.rs`, `app/work/tabs.rs`; the app's side of tabs, their titles and the tab bar's mouse
in `app/tabs.rs`, the bar's geometry in `tabbar.rs`), the list panel's workspace chip, its view switcher and the workspace switcher
(`app/nav.rs`; their geometry in `navbar.rs`, where a Nerd Font glyph always takes two cells) — with its panes (`app/pane.rs`: views only — selection, VISUAL range, Insert mode, anchor,
rows drawn, the thread panel it opened: a pane's role is this relation, never its place in the tree; its own
back/forward history), the messages of each target a pane shows, held once however many panes show it
(`app/timelines.rs`), and what is being written to it (`app/drafts.rs`, composers of
`app/composer.rs`); every request to the backend gets its id from one allocator
(`app/requests.rs`), the read model of the workspaces (`app/model.rs`), To-dos' filter and
order and the changes to saved items it waits for (`app/todos.rs`), the
keyboard help (`app/help.rs`) and a question with two answers (`app/dialog.rs`); splits of a
tab join as they are built. Where the keyboard is is one field of the app — the view switcher, the list panel or the work area,
whose active pane has it (`app/focus.rs`; the pane is never kept twice, so the focus cannot name
a closed pane; the shell and the work area hand back where it should go) — and the
popups come in one order (`app/overlay.rs`); the key map, the mouse and drawing all read
those two. The geometry of the screen (`screen.rs`) is pure:
`screen::frame` lays out a frame once — the regions, each open pane, its messages and its
composer — and drawing, the mouse and scrolling all read that one layout. Rules:

- An `Action` is namespaced by its owner (`Action::CommandLine(CommandLineAction::Run)`), so
  dispatching one is routing, not one match over every action of the app.
- One action registry (`action.rs`) gives every action its id, label and `:` commands; the key
  map, the command line and `docs/keybindings.md` all read it.
- State does no I/O. The binary owns the terminal and a single `tokio::select!` loop over
  input, signals, the backend's events and the app's next deadline; it redraws only after something changed and
  sleeps when nothing happens (the idle wakeups are a performance budget).
- The UI talks to a backend only through the protocol of `slakio_core::backend`: the app
  queues requests (each with its id and the workspace it is about), the binary's loop delivers
  them through a `BackendHandle` (bounded channels; the backend runs on its own task) and hands
  the events back whenever they arrive (`exchange.rs`; an answer may ask for the next page),
  and an event for an older request (an older generation) or for a target no pane shows is
  dropped. A request can fail (`Event::Failed`, its kind as data): whatever waited for it stops
  waiting and says so, never "Loading" for ever; its detail goes to the error log, and only the
  user asks again (`r`, when the reason may pass). A read-only backend's handle
  (`Capabilities::read_only`) refuses a change before it reaches the backend, and the UI offers
  none. Saved items a backend cannot
  tell are `None` (To-dos is unavailable, not empty).
  Messages come in pages (`Command::History`), newest first, older ones as the selection nears
  the top.
  A saved item (a to-do, or a message in Home's Saved) changes by `Command::Save`; nothing
  changes on screen until the answer, every saved item after the change, replaces them. The
  snapshot, and every answer about saved items, says when it was taken (the backend's clock):
  reminders due before then are overdue.
- **Several workspaces.** The read model is keyed by id: people by the workspace that lists them
  and their id (a Slack Connect peer two workspaces list is two entries), the workspace shown by
  its id, and the list cursor found again by its row's id after the rows change. Each signed-in
  workspace has a backend of its own; `many::serve` puts one handle in front of them and hands a
  request to the workspace it is about (`Request.workspace`, a history request's target), so one
  that fails, hangs or is gone fails on its own and holds none of the others back; a request for
  no workspace, or one not signed in to, fails (`Auth`) without going anywhere. The app boots
  each workspace on its own (`App::connect_workspaces`): it lists them before they answer, a
  workspace's answer replaces its part of the model only, and each has its own failure ("Could
  not load", and the status line names whose start failed) and its own `read_only`. A backend
  that boots every workspace at once (the demo) answers `Command::Boot` without a workspace.
- **Live updates.** What a backend pushes (`Event::Live`) goes to every timeline that shows where
  it belongs (`app/work/live.rs`): a message posted into its conversation or thread (only among
  what is loaded, never into a span not read), a new copy over the one loaded, a deletion out, a
  reply onto its thread's count; a pane where the user chose a message keeps its view, one
  following the newest keeps following. Counts follow (`Model::bump`: not for the user's own
  messages, nor a reply not shown in the conversation; `Model::mark` for one read elsewhere).
  Whether a workspace's updates arrive is said briefly in the status line each time it changes
  (`app/answers/live.rs`; what was said of it before is dropped, `Status::restate_about`).
- What only the binary can do is an `Effect` the loop carries out: copying is OSC 52, so no
  clipboard library or helper process is involved; saving an answer the app asked for (the
  icons question) goes into the config file, keeping its comments. `slakio --demo` uses `DemoBackend`
  (`crates/slakio-tui/src/demo.rs`) over `slakio-world`; `main.rs` is the one place that names
  a concrete backend.
- Channels from background tasks are bounded wherever a stream can be large (websocket events,
  history pages, downloads); events carry their target and a generation, and stale ones are
  dropped.

## Remote text

Everything a remote party chose — names of workspaces, people, sections, conversations and
reactions, message texts, later topics, statuses, file names and link texts — is
`slakio_core::sanitize::Remote`. It has no `Display`, `Deref` or `AsRef<str>`, so it reaches a
widget only through `line()` / `block()`, the sanitiser, which removes escape sequences
(7-bit and 8-bit), control, bidi and invisible characters, private-use code points, mark
floods and overlong text. `Remote::unsanitized` (comparisons and lookups) and `Safe::trusted`
are kept out of the drawing code by a test. Message texts are sanitised once, when their page
arrives, and then read as Slack's markup (`mrkdwn.rs`: marks, code, quotes, people, channels,
links, emoji; only `&amp;`, `&lt;` and `&gt;` are decoded, so nothing the sanitiser removed
comes back); people and channels are named from the read model when drawn
(`ui/timeline/body.rs`). The composer sanitises what is typed or pasted into it.

## Robustness

- **Unknown is not absent.** A file, directory or store that cannot be read is an error, never
  "empty": a config file that cannot be used is reported and never written over; a secret store
  that fails says so instead of answering "no entry".
- **Fail closed.** `SLAKIO_SECRET_STORE` with a value that is not `keychain` or `memory` stops
  the program (exit code 2); a typo never reaches the real keychain. Stores never fall back to
  one another.
- **Terminal restored exactly once.** Every terminal mode the binary turns on is recorded in one
  `TermState` and undone once, by a guard (normal exit, error, failed setup) or by the panic hook
  (before the panic message), whichever comes first. SIGTERM, SIGHUP, SIGINT and SIGQUIT end the
  loop the same way as `:qa`. A pseudo-terminal test checks each path.
- **Atomic writes.** Files are written to a temporary file, flushed and renamed
  (`fsutil::atomic_write`); a new file never replaces one that appeared meanwhile
  (`fsutil::create_new`, `fsutil::rename_new`).
- **Failures as data.** Core never words a failure: a `Fault` has a kind the UI turns into a
  catalog message and a raw detail that only goes to `<state>/errors.log`. The status line never
  shows OS or parser text.

## The Slack transport

`slakio-slack` is the only crate that may talk to Slack. It is read-only by construction, and
every rail sits in the transport, below the backend, so no caller can go around it:

1. **Allowlist.** A request is built from a `Method`, an enum of read-only methods
   (`client.userBoot`, `client.counts`, `client.shouldReload`, `conversations.view` / `history` /
   `replies` / `info`, `users.info`, `users.prefs.get`, `users.channelSections.list`,
   `emoji.list`, `stars.list`, `usergroups.list`, `dnd.info`, `dnd.teamInfo`, and the edge
   cache's `channels/info` and `users/info`). A name that comes in as text is refused
   (`ReadOnly`) before anything is counted or sent. No write, and no directory listing
   (`users.list`, `conversations.list`), is on it.
2. **Kill switch.** `SLAKIO_NETWORK=off` or `network = "off"` in the config file: nothing is
   sent. A value of the variable that is not `on` or `off` stops the program (exit code 2).
   `Governor::stop` turns it off while running and ends what is in flight.
3. **Request cap**, for the whole process: 120 requests a minute, of which background work
   (counts, lookups) may use 90, so what the user waits for always has room; 1,500 an hour;
   and a safety ceiling of 10,000 for a run, after which all network stops until the program
   starts again. Over a cap a request fails with `CapReached` and is not sent.
   When Slack asks to slow down (HTTP 429 or `ratelimited`), that method is paused for its
   `Retry-After` (30 seconds when Slack does not say); a paused method fails with
   `RateLimited` without being sent, and the other methods go on.
4. **In flight**: at most 8 requests at once, 64 more waiting; beyond that a request fails as
   `Busy`. Every workspace's client has a lane of its own: one of the 8 is kept for it (for as
   many lanes as there are slots), and it holds at most 6 of the 8 and 48 of the waiting places,
   so busy workspaces, however many, never leave another without a slot (the caps and budgets
   above stay shared by all of them). A request given up while it waits frees its place.
5. **Hosts.** TLS (rustls with the OS's certificate verifier, HTTP/2 when offered) to the
   signed-in workspace's own host (`<name>.slack.com`) and the edge cache's
   (`edgeapi.slack.com`), and for the live updates the realtime host (`wss-primary.slack.com`,
   HTTP/1.1 only), nothing else: the host is checked where the socket opens, so no file,
   image or other host is ever reached. The tests reach a loopback address over plain HTTP.
6. **Format.** Requests look like the official web client's (the browser's header set in its
   order, no `Referer`, the token in the form body and never in a header, the client's `_x_*`
   parameters in its order; an edge lookup is sorted JSON as text). Golden tests hold the exact
   requests.
7. **Answers.** Slack's JSON is decoded tolerantly (unknown fields ignored, every field but an id
   optional, an unreadable list entry left out). `ok: false` and HTTP statuses map to the core's
   `FailKind` (`Auth`, `RateLimited { retry_after }`, `Network`, `Offline`, `Busy`, `CapReached`,
   `Api { code }`); the error log gets the method, the code or the status, never a body, a token
   or a message.

**Live updates** (`live.rs`, `slakio_slack::serve_live`, what the binary runs): once a workspace
has booted, one WebSocket to the realtime host, opened as the web client opens it (the token in
its address, the browser's smaller header set for a socket, the `d` cookie). It only receives:
the client writes nothing on it but a pong for each ping and a close (frames read and written by
hand, `live/socket.rs`: masked, no extension, a message at most 4 MiB, anything a server must not
send fails it). Messages posted, changed and deleted and conversations read elsewhere
(`*_marked`) are pushed to the UI as `Event::Live` (generation `Generation::PUSHED`, no request
asked for it), a new author named by one lookup a socket; everything else on the socket is left
unread. A socket that drops, closes or is silent for 60 seconds opens again after a pause doubling
from 1 to 30 seconds, each a random half to all of it, reset once one stayed open 30 seconds; each
opening goes past the kill switch and counts against the request cap (`Governor::dial`), and
switching the network off ends the socket for good. The socket replays nothing it missed, so when
one opens again the UI is told to catch up (`Connection::Connected { catch_up: true }`) and asks
for the newest page of each conversation open again (`Command::Catchup`, at most 8, one page each,
`conversations.history` in the background share of the budget, never an open), each page the
truth for the span it covers, the whole timeline replaced when it does not reach back to what was
loaded (no gap is hidden). Each workspace's socket is its own; one that fails holds none back.

The backend (`slakio_slack::serve`) owns a `BackendEnd`: every request gets one answer, a change
is refused before any request, and a newer page of the same conversation supersedes one in
flight. Opening a conversation waits 200 ms first and at most one open is under way: a newer
one cancels it (before it is sent, or by dropping its answer), and the cancelled one is answered
`Busy`. People are named by id lookups, never a directory listing: a view brings some of its
authors along, the rest (and the DMs' people at the start) are asked of the edge cache's
`users/info` in batches of 80, each id once a session (kept in memory); until then an id shows.
A start reads, besides `client.userBoot`, the unread counts (`client.counts`; when they
fail the snapshot says so, `Snapshot::counts_failed`, and the UI and the cache keep the counts
they had instead of zeros), the DMs' people by name and whether they are in do not disturb
(`dnd.teamInfo`, in batches of 80; the user's own from the start bundle); the conversations
muted come from the user's notification preferences in the start bundle. A muted conversation
keeps its counts in the model but never badges (`Conversation::badges`). Older pages of a
conversation come from `conversations.history` (`latest`, exclusive): a message listed twice
is shown once, a deleted one (`message_deleted`, `hidden`) not at all, and an older page with
nothing older in it ends the paging. Slack pages a thread forward only (`conversations.replies`
by cursor), so its newest page reads it from its start (200 a request, 15 requests at most) and
keeps it for the session (64 threads); its older pages come from what was kept. A message whose
blocks are more than rich text (a bot's sections, headers, context) or whose text is empty is
written out as Slack markup from its blocks; attachments and file names follow the text
(`blocks.rs`); a bot is named as it posts. A read-only backend offers no composer: nothing is
ever shown as sent. The binary runs one for
each signed-in workspace, behind one handle (`slakio_tui::many`, below); the tests run it
against `slakio-fake` (one fake per workspace, `FakeSlack::start_team`). The HTTP stack is chosen in [ADR 0001](architecture/0001-http-stack.md).

## The local cache

`slakio-store` keeps, in `<cache>/slakio-store/cache.sqlite3`, the last boot snapshot (replaced whole by each
boot; a workspace it no longer lists leaves the cache with it) and the newest messages of each
conversation and thread opened (each page the backend answers is the truth for the span of time
it covers: what the cache held there and the page lacks is gone; a thread is a place of its own,
`<conversation>:<thread>`, capped and aged as a conversation is). Remote text is kept as it came and read
back as `Remote`, so it still reaches the screen only through the sanitiser. No secret is kept.

- **Schema.** `schema_version` holds the number of migrations applied (`schema.rs`, appended to
  only); each one runs in a transaction. A cache of a newer version, or one that is not a
  database, is set up again from nothing (it only holds what can be fetched again).
- **One writer.** The writer holds an OS lock on `cache.lock` (`File::try_lock`); a second
  process opens the database read-only and never writes it; a read is one transaction. The
  store's own directory (`slakio-store`, which it makes) is `0700`, the files `0600` (Unix),
  opened without following a symbolic link and made private through the open file; the cache
  directory named (`SLAKIO_CACHE_DIR`) is never changed. The journal must be WAL, or the cache
  is not used.
- **Per workspace.** One cache holds a part per signed-in workspace: its last boot snapshot (the
  lists in order within it, its own time and saved-items marker) and its conversations' pages. A
  workspace's boot answer replaces its own part only; signing out of one removes that part
  (`Store::forget`).
- **Whose.** Each part keeps whose it is (`Store::claim`, meta `owner:<team>` = the user):
  another sign-in to that workspace finds that part emptied (in one transaction, the other parts
  untouched), and a second process of another sign-in does not use it.
- **Caps.** At most 200 messages a conversation, 500 MB, 30 days since a conversation was last
  opened (config keys `cache_messages`, `cache_max_mb`, `cache_days`); every conversation asked
  for during the run, by any workspace, is exempt, so nothing shown in a pane is pruned. Pruned after the first boot
  and every 32 pages, in one transaction; the freed pages go back to the file system 256 at a
  time, the jobs asked for meanwhile first.
- **In front of a backend** (`layer.rs`): a task between the UI's handle and one workspace's
  backend (every workspace's front shares the one cache). A boot, and the newest page of a
  conversation of that workspace, are answered from the
  cache first — `Event::CachedBoot` / `Event::CachedHistory`, queued for the UI before the
  request is handed on, so a start draws without waiting for Slack; the cache is waited for
  500 ms at most for a boot (`BOOT_WAIT`) and 100 ms for a page (`READ_WAIT`), so a busy or slow
  cache never holds a request back (the error log says when it was not waited for) — then by the
  backend, whose answer replaces it in
  the UI and is kept. A cached answer ends nothing: the request still gets exactly one answer of
  the backend's. The UI marks what came from the cache (`cached` in the status line) until then,
  keeps it when the refresh fails (the status line says why), and asks for the newest page again
  on `r`.
- **Its own thread.** The store runs on a thread of its own (`cache.rs`); jobs run in order,
  writes are not waited for, a failed job goes to the error log and a database found damaged is
  set up again.
- **Backups.** On macOS the store's own directory (an absolute path) is excluded from Time Machine once (`tmutil
  addexclusion`, remembered in the cache); a failure is said once. Debug builds run only a
  stand-in named by `SLAKIO_TEST_TMUTIL` (the tests'), never `tmutil` itself.

The binary opens it once for the process, when the first workspace's backend starts
(`src/store.rs`; never for the demo), and puts it in front of every signed-in workspace's
backend; `slakio logout` removes it (or one workspace's part).

## Signing in

`slakio login` (or the screen `slakio` shows when no workspace is signed in) takes a request
copied from the Slack web app with the browser's "Copy as cURL", pasted into the app (bracketed
paste) or piped in. `slakio-auth` reads the workspace's host, the session token and the `d` and
`d-s` cookies out of it wherever the browser put them (Chrome, Firefox, Safari, `cmd` and
PowerShell copies), without running anything; its errors are fixed words that never quote the
paste. The binary then asks Slack with one `client.userBoot`, through the transport and its
rails, and only an accepted sign-in is kept: the credentials as one entry of the OS keychain
(service `slakio`, the team id as the account), the workspace's id, name and host in
`workspaces.toml`. Signing in to another workspace adds it (the desktop picker marks several
with `Space`); the ones signed in to before keep their entries. `slakio logout <workspace>`
removes that workspace's keychain entry, its line of the file and its part of the cache;
`slakio logout --all` (or with one workspace, `slakio logout`) removes every keychain entry it
finds — those the file names, the team ids of a file that cannot be read, and every other entry
of the `slakio` service — and then the file, so nothing is left behind. When the keychain cannot
be listed (locked, no prompt possible) but every entry the file names is removed, it still ends
well and warns that entries no workspace names were not looked for. A `workspaces.toml` that
cannot be read is never written over: signing in moves it aside to `workspaces.toml.unreadable`
first. Both are written readable by the user alone (`0600`), and so is `workspaces.lock`.
Changes of the file hold `workspaces.lock` beside it, and `slakio logout <workspace>`
holds the cache as its writer from before the credentials go until the part is removed. The paste and the credentials are wiped from memory when dropped (`zeroize`);
the parser reads the paste in one pass and gives up on more than a few candidate values. With `SLAKIO_SECRET_STORE=memory` (every test)
the keychain is never touched. A debug build may sign in to a fake Slack on the loopback address
(`SLAKIO_TEST_SLACK_ADDR`); a release build never reads that variable.

## Keys

`crates/slakio-tui/src/keymap.rs` holds the context tree and the default bindings; every
binding names an action of the registry. A few global keys (`Ctrl+Q`, `F1`, `Ctrl+P`) are looked
up first, also while typing; then keys resolve from the focused context outwards; a text input
or a modal context (the help, a question) has no parent, so `Ctrl+W` can close a pane in Normal
mode and delete a word while typing. Sequences (`g g`, `Space w h`) wait for their next key;
after a short wait the which-key popup lists what may follow (`keymap/guide.rs`), its delay one
more deadline of the loop. The checker (`keymap/check.rs`) runs on the defaults in the tests and
rejects duplicates, shadowed parent keys, prefixes, character keys in text inputs, keys a
terminal cannot tell apart without the kitty keyboard protocol (`Ctrl+I`/`Tab`,
`Ctrl+M`/`Enter`, `Ctrl+[`/`Esc`, `Ctrl+H`/`Backspace`) bound to different actions, actions
reachable only by such keys or `Alt` keys, and a context taking a global key. `Ctrl+I` is bound
only when the terminal has the protocol on. The hint line (`keymap/hints.rs`), the keyboard help,
the empty states, the command palette and `docs/keybindings.md` all show keys looked up in the
key map, so a key is shown as bound.

The command palette (`app/palette.rs`, drawn by `ui/palette.rs`) is the `:` command line with a
list: `Ctrl+P` and `:` open it, and its entries are generated from the action registry, each
with its keys from the context under the palette. A typed word ranks them (`action::rank`): a
command it names, then one it starts, then an entry with a word it starts, then one with its
letters in order from a word start; `Quit` sorts after everything that matches as well, so it is
never the first entry of the full list. `:theme ` lists the themes, `:avatars ` its values.
Its geometry (`screen::palette`) is shared by drawing and the mouse.

## The look

`crates/slakio-tui/src/theme.rs` holds the color tokens and the styles made of them; widgets
name roles, never colors. The built-ins are the terminal's own 16 colors, `dark`, `light`,
`high-contrast`, `nord`, `dracula` and three families (`catppuccin`, `tokyo-night`, `gruvbox`)
whose light or dark variant follows the terminal's background, asked once with OSC 11; `auto`
takes `tokyo-night` on a terminal that says it shows 24-bit color. `:theme <name>` resolves a
name again with what was found at startup (`theme::Look`) and saves it to the config file. Every
truecolor theme passes WCAG contrast checks (`theme/tests.rs`): body text 4.5:1 on every surface,
muted text and marks 3:1, pills, mode badges and the initials on avatar chips 4.5:1 (4.3:1 on
a dark theme's tinted chip). An avatar chip (`avatar.rs`: initials and a color slot hashed
from the person's id) takes one of the theme's avatar hues, none in the red family (red is the
mention pills'); a truecolor theme draws it as a tint of the hue on the background with the
hue as the initials (darkened on a light theme), never brighter than the selection bar or the
accent, and the 16-color theme as colored initials without a background: a solid block is a
pill's. Two rules hold in every theme and are
tested cell by cell (`tests/style_flows.rs`): the focus shows on a panel's border and title
only, and a selection is a background (or a bar in the left gutter), never an underline.

## UI strings (i18n)

`locales/en.toml` is the source catalog; `locales/ko.toml` must have exactly the same keys and
placeholders. `crates/slakio-core/build.rs` checks both and generates a typed API: `Label` for
entries without placeholders, `Msg` for entries with typed arguments (`{count}` is a number,
`{elapsed}` a duration, anything else text). A missing key or argument is a compile error. UI
chrome takes `Localized` text, which only the catalog or the grep-able escape hatch
`Localized::verbatim` (data, key names, the product name) can produce. Korean text appears only
as the values of `locales/ko.toml`; a Korean value equal to the English one must be listed with
its reason in `SAME_IN_KO` (`crates/slakio-core/tests/catalog.rs`).

## Paths

See `crates/slakio-core/src/paths.rs`: config, data, state and cache directories, each with a
`SLAKIO_*_DIR` override and the XDG variables honored on every OS, so development runs and tests
pointed at scratch directories never touch the real ones.
