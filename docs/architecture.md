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
each a layout tree of `layout.rs` — panes split beside or under each other, a pane and the
auto thread panel it opened split as one — its active pane and whether it is zoomed on it), the
user's splits, zoom, even room and resizing (`app/work/splits.rs`; what a split opens, the room
it needs, the keyboard moving to the pane on a side, by place, keys and a dragged border
resizing a pane, in `app/splits.rs`; the core's `Node::fitted` gives every split the room its
sides need, so a smaller screen shrinks every pane alike), the tabs saved for the next run and
opened again (`app/work/restore.rs`, `app/restore.rs`; the file's form is the core's
`layout/saved.rs`, the file itself `layout_file.rs`, read and written by the binary only, and
only when the work area's change count moved; a saved pane is left out only on the backend's own
boot answer, never on the cache's or a failed one), the
tabs closed to open again
(`app/work.rs`, `app/work/tabs.rs`; the app's side of tabs, their titles and the tab bar's mouse
in `app/tabs.rs`, the bar's geometry in `tabbar.rs`), the list panel's workspace chip, its view switcher and the workspace switcher
(`app/nav.rs`; their geometry in `navbar.rs`, where a Nerd Font glyph always takes two cells) — with its panes (`app/pane.rs`: views only — selection, VISUAL range, Insert mode, anchor,
rows drawn, the thread panel it opened: a pane's role is this relation, never its place in the tree; its own
back/forward history), the messages of each target a pane shows, held once however many panes show it
(`app/timelines.rs`), the files of a message downloaded and offered to open (`app/files.rs`),
and what is being written to it (`app/drafts.rs`, composers of
`app/composer.rs`); every request to the backend gets its id from one allocator
(`app/requests.rs`), the read model of the workspaces (`app/model.rs`), To-dos' filter and
order and the changes to saved items it waits for (`app/todos.rs`), the
keyboard help (`app/help.rs`) and a question with two answers (`app/dialog.rs`). Where the keyboard is is one field of the app — the view switcher, the list panel or the work area,
whose active pane has it (`app/focus.rs`; the pane is never kept twice, so the focus cannot name
a closed pane; the shell and the work area hand back where it should go) — and the
popups come in one order (`app/overlay.rs`); the key map, the mouse and drawing all read
those two. The geometry of the screen (`screen.rs`) is pure:
`screen::frame` lays out a frame once — the regions, each open pane, its messages and its
composer — and drawing, the mouse and scrolling all read that one layout. Panes side by side
share the border between them (a pane starts on the last column or row of the one before it);
drawing joins the lines where they meet. Rules:

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
  Once Slack refuses a workspace's sign-in while it runs (any request, or its socket), its client
  sends nothing more and its live updates say `Connection::SignedOut`; the app keeps it on screen,
  marked, and `:login` signs in again over the app: the binary puts a new backend in place of
  that workspace's alone (`many::Replace`), and the app boots it again, keeping its panes. Each
  request the old backend had not answered is answered once, as failed for the network, and
  asked again of the new one; the new client shares the workspace's governor lane (its kept slot,
  its background hour). The old cache layer's writes are refused once the part is another
  user's; signed in as someone else, the app forgets that workspace's panes, drafts and lists.
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
  shows OS or parser text. The log redacts each detail (`fault::redact`: tokens, cookies,
  addresses past their host, quoted text), is mode 0600, opened with `O_NOFOLLOW`, its directory
  kept 0700, and rotates at 1 MiB to one `errors.log.1` (both kept 0600).
- **Files.** A file downloaded is marked as from the internet while it is still the part file,
  before it gets its name (`slakio_core::download`: on macOS the `com.apple.quarantine`
  attribute set on the open file, on Windows the `Zone.Identifier` stream, internet zone); a
  file that could not be marked is kept but never offered to open, nor is the demo's. Only a
  kind a viewer shows (`download::openable`, an allowlist: PDF, pictures, sound, video, plain
  text; no office documents, tables or `.md`/`.json`) is offered to open, by a
  question whose safe answer has the focus; yes hands its absolute path, a plain file and not
  a link, to the same opener as a link (`opener::open_file`). Anything else is only saved.
- **Links.** `opener::check` lets only `http`/`https` addresses through (at most 8 KiB, a
  plain ASCII host within the DNS limits, no user or password, no host that reads as a number
  but a dotted IPv4 address) and hands over a normalised form (scheme and host lower case,
  anything past the host that is not plain ASCII percent-encoded) as one argument to the
  system's opener (`/usr/bin/open`, `xdg-open` on `PATH`, `%SystemRoot%\System32\rundll32.exe
  url.dll,FileProtocolHandler`; no shell). A link whose text names another host is asked about
  first (`dialog::Question::Open`), the real host first and cut from its start, so its end
  shows. `o` with several links that open (`app::picker::Picker`) draws letters over the first
  cells of each, where drawing last placed them (`Pane::links`, also what a click hits; drawn
  over, so nothing moves), or lists them when one was not drawn; its keys are the modal
  `links` context of the key map. A pick opens only while the message reads as it did when
  picking began; a click opens only the address drawn there, and not just after the pane's
  links moved (`Pane::links_moved`). Tests
  stand in for the opener (`SLAKIO_TEST_OPENER`, debug
  builds only) and never start a browser.
- **Slack links.** Every way of opening a link goes through `App::open_address`; one that
  `permalink::parse` reads as Slack's own (`https` only, after `opener::check`; a host whose
  last two labels are `slack.com`, a workspace's or an Enterprise Grid one, or
  `app.slack.com/client/<team>/<conversation>`; ids and a 16-digit time of Slack's shape) opens
  inside (`app/jump.rs`). Its workspace is the one signed in on that host (`SignedIn::host`) or
  with that id, and only that one; an Enterprise Grid organization's link (its host, or its
  `E…` id) names none, so one signed in to that lists the conversation. None, or one that does
  not list it, or Slack saying the conversation cannot be read here (`channel_not_found`,
  `not_in_channel`, `access_denied`, `missing_scope`, `is_archived`), asks before opening it in
  the browser (`dialog::Question::Browser`, then the web link's own rules). A message loaded already is
  selected; one that is not is read around (`Command::Around`, `app/work/jump.rs`: what was
  loaded goes): the backend reads `conversations.history` with `latest` and `inclusive`, then
  with `oldest`, and keeps the newer ones only when Slack says none is left past them, else the
  page is `cut` (no gap hidden; nothing comes live after the last message read, there is no
  catch-up, and `G` reads the newest again); when that second request fails, the first half is
  shown as a cut page. It is not cached, and the same link opened again before its answer asks
  nothing more. The message is marked in the gutter for two seconds (`Pane::flash`); one that is
  not there any more is said (`jump.gone`) and nothing is marked. A reply older than a thread's
  newest page is read back to (older pages of a thread come from what the backend kept, without
  a request).

## The Slack transport

`slakio-slack` is the only crate that may talk to Slack. It is read-only by construction, and
every rail sits in the transport, below the backend, so no caller can go around it:

1. **Allowlist.** A request is built from a `Method`, an enum of read-only methods
   (`client.userBoot`, `client.counts`, `client.shouldReload`, `conversations.view` / `history` /
   `replies` / `info`, `users.info`, `users.prefs.get`, `users.channelSections.list`,
   `emoji.list`, `stars.list`, `usergroups.list`, `dnd.info`, `dnd.teamInfo`, `search.messages`
   (a page of the messages a query finds, only when the user asks), `activity.feed` (a page of
   the user's own Activity, only while it shows; nothing is marked read) and `messages.list`
   (the texts of the messages that page names, by id), and the edge cache's
   `channels/info` and `users/info`). A name that comes in as text is refused
   (`ReadOnly`) before anything is counted or sent. No write, and no directory listing
   (`users.list`, `conversations.list`), is on it.
2. **Kill switch.** `SLAKIO_NETWORK=off` or `network = "off"` in the config file: nothing is
   sent. A value of the variable that is not `on` or `off` stops the program (exit code 2).
   `Governor::stop` turns it off while running and ends what is in flight; `Governor::switch`
   (the app's `:net off` / `:net on`, an `Effect::Network` the binary carries out) turns it off
   and on again, never on when it was off from the start or the run's cap was reached.
3. **Request cap**, for the whole process: 120 requests a minute, of which background work
   (counts, lookups) may use 90, so what the user waits for always has room; 1,500 an hour;
   and a safety ceiling of 10,000 for a run, after which all network stops until the program
   starts again. Over a cap a request fails with `CapReached` and is not sent.
   A file's smaller copy (a preview, on the file host with the session's cookie) is a
   background request with a share of its own inside the background shares: at most 30 a
   minute and 350 an hour of a workspace's 60 and 700, and 45 a minute and 500 an hour of the
   process's 90 and 1,000 (`Limits::lane_picture_*`, `Limits::picture_*`). Over it a preview
   fails with `CapReached`, is asked again a minute later (`images::RETRY_AFTER`) and shows as
   "not loaded" meanwhile; it counts on the minute, the hour and the run like any request.
   Profile photos and a workspace's emoji (the avatar and emoji hosts, no session) keep to the
   picture hosts' budget instead (`governor::cdn`, `CdnLimits`), which never touches the API's
   shares nor its run total: 300 a minute and 3,000 an hour for the process, of which the
   pictures asked for the list and ahead take at most two thirds, at most 12 at once, and an
   alarm of its own at 20,000 a run, past which every later photo or emoji is refused for the
   run (initials and `:name:` stay; one diagnostics line says so) while the API goes on. A
   picture host that answers 429 is not asked again for its `Retry-After` (30 seconds when it
   does not say, at most an hour); the others go on. Which budget a picture takes follows its
   kind (`Source`), never its host. So however many pictures a screen wants, the socket's
   opening, its catching up and the people lookups always have their room.
   When Slack asks to slow down (HTTP 429 or `ratelimited`), that method is paused for its
   `Retry-After` (30 seconds when Slack does not say); a paused method fails with
   `RateLimited` without being sent, and the other methods go on. A method Slack allows only a
   few of a minute that the user can ask for again and again (`search.messages`,
   `activity.feed`) goes at most
   once a second: one asked sooner waits for its turn.
4. **In flight**: at most 8 requests at once, 64 more waiting; beyond that a request fails as
   `Busy`. Every workspace's client has a lane of its own: one of the 8 is kept for it (for as
   many lanes as there are slots), and it holds at most 6 of the 8 and 48 of the waiting places,
   so busy workspaces, however many, never leave another without a slot (the caps and budgets
   above stay shared by all of them). A request given up while it waits frees its place.
5. **Hosts.** TLS (rustls with the OS's certificate verifier, HTTP/2 when offered) to the
   signed-in workspace's own host (`<name>.slack.com`) and the edge cache's
   (`edgeapi.slack.com`), for the live updates the realtime host (`wss-primary.slack.com`,
   HTTP/1.1 only), for a file the user downloads and a picture's smaller copy Slack's file
   host (`model::FILE_HOST`, the `files` host of `slack.com`), and for profile photos the
   avatar hosts (`images::AVATAR_HOSTS`: Slack's avatar host, `secure.gravatar.com` and Slack's
   static host for default avatars), and for a workspace's own emoji the emoji host
   (`images::EMOJI_HOSTS`); the avatar and emoji hosts never get a cookie, a token, an `Origin`
   or a `Referer`: the wire refuses a request to them that carries one;
   nothing else: the host is checked where the socket opens, so no other host is ever reached.
   The tests reach a loopback address over plain HTTP.
6. **Format.** Requests look like the official web client's (the browser's header set in its
   order, no `Referer`, the token in the form body and never in a header, the client's `_x_*`
   parameters in its order; an edge lookup is sorted JSON as text). Golden tests hold the exact
   requests.
7. **Answers.** Slack's JSON is decoded tolerantly (unknown fields ignored, every field but an id
   optional, an unreadable list entry left out). `ok: false` and HTTP statuses map to the core's
   `FailKind` (`Auth`, `RateLimited { retry_after }`, `Network`, `Offline`, `Busy`, `CapReached`,
   `Api { code }`); the error log gets the method, the code or the status, never a body, a token
   or a message.

**Pictures** (`picture.rs`, `http/fetch.rs`; the UI's `images`): the UI asks for a picture
(`Command::Image`) only when it would draw it and the terminal draws pictures. A file's smaller
copy (`File::thumb`, only on the file host, only for PNG, JPEG and GIF files that can be
downloaded; never the original) goes with the download's cookie and image headers; a profile
photo (`User::avatar`, only on an avatar host) with no cookie, no token, no `Origin` or
`Referer`. Both are `https` only, checked when Slack's answer is read and again before the
request (`images::Source::checked`); a copy follows redirects only on the file host, a photo
at most 2 (`picture::AVATAR_REDIRECTS`), each only to an address `images::avatar_address` keeps
(a Gravatar address only when it names Gravatar's own parameters alone, plainly and with no `;`,
and each `d=`/`default=` fallback it names is a keyword or on Slack's avatar hosts), each hop
counted. The user's own photo comes from the start bundle's `self`
profile, else from the edge `users/info` lookup the boot already makes for the DMs' people.
A photo slakio makes from the uploaded original (`images::avatar_sized`, `User::avatar_guessed`)
never replaces one Slack gave, in the model or the cache; refused (401, 403, 404, 410) before
any redirect, it is asked at most `picture::AVATAR_FALLBACKS` (2) more times, at the same file's
other names on the avatar host (`images::avatar_fallbacks`), each checked and counted. A
preview's ask counts as a background request of the workspace's lane (`Governor::picture`), a
photo's or an emoji's in the picture hosts' budget, holding one of its 12 places over all its
hops; each within 25 s in all (`picture::DEADLINE`, however its bytes trickle). Only an answer whose type is `image/…` is read, at most `images::BYTES_MAX`
bytes (said or counted). The UI decodes it on a blocking task, the size read
from the header first and refused over `images::SIDE_MAX` a side or `images::PIXELS_MAX`
pixels, makes it smaller and sends it to the terminal once as a kitty graphics virtual
placement (the base64 is made on that task too; only the write is the loop's); it is drawn as
placeholder cells, so whatever is drawn over it replaces it, and no pass over the screen (the
dim under a popup, the selection bar) changes their colour, which names the picture. A panic in
the decoder is caught with the panic hook told so (`terminal::catching`): the terminal is left
as it is. Turning pictures off removes each by its id, then all; the way out removes every id
sent (a range), then all.

**A workspace's own emoji** (`emoji.rs`; core `images::emoji`; the UI's `images/custom.rs` and
`ui/timeline/custom.rs`): the first frame that shows a shortcode the standard emoji do not know
(one with a letter) notes its workspace; after the frame the UI asks for that workspace's list
once (`Command::Emoji`, answered `Event::Emoji`): `emoji.list`, one background request. It is
kept in memory for the run, forgotten when the workspace is signed in again (read again when
next wanted), and after a failure asked again only after the pictures' waits (never for a
refusal); a failure is never said. The backend reads it into what each name draws
(`images::emoji::resolve`): its picture only when `images::emoji_address` keeps the address
(the emoji host, `https`, no port, no user), aliases followed at most `ALIAS_DEPTH` (4) times
with a guard against cycles, an alias of a name the workspace lacks kept as that standard
emoji's name. A name written in conjoining Hangul jamo (as a Mac writes an uploaded file's
name) is read as its syllables (`images::emoji::compose_hangul`), in the list and in text and
reactions, the typed form winning where both are listed; only modern Hangul composes (other
scripts' decomposed letters, archaic and leftover jamo stay refused). A reaction or a rich-text
emoji that comes with its picture's address (another workspace's, in a shared channel) gives
its message that address (`Message::emoji_pictures`, at most 16, on the emoji host only); it is
drawn only once the workspace's own list is in and lacks the name, and is not kept in the
cache. A link's label draws a standard emoji's shortcode as that emoji. An emoji's picture is `images::Source::Emoji`, asked for only when a frame draws
it, fetched like a photo (no cookie, no token, no `Origin`, no `Referer`), following at most 2
redirects (`picture::EMOJI_REDIRECTS`) each only to the emoji host and counted; decoded whole
into two cells of a row (`Shape::Emoji`, first frame of a GIF) and drawn as two placeholder
cells; the wrap keeps a picture's later cell with the one before it, so a line never breaks
inside one. Pictures off, not here, failed or refused: `:name:` text.

**The pictures' room** (the UI's `images.rs`): the terminal holds at most `ENTRIES_MAX` (64)
pictures and `BYTES_CAP` bytes, keyed by address and cells (a person in both the timeline and
the list is two pictures, so at most two fetches a run while the screen does not change and
neither is removed or fails). The least recently drawn goes first, never one the latest shown
frame or the one before it looked at; when all are on screen a list photo makes room for a
timeline picture, else the new picture is not sent and is parked until the screen changes (the
set of pictures a shown frame looks at differs from the frame before's). After a frame the
timeline's pictures are asked before the list's photos, at most `IN_FLIGHT_MAX` (4) previews and
`CDN_IN_FLIGHT_MAX` (12) photos and emoji at once, and none while the pictures on screen and
those on their way fill the room. A photo or an emoji that failed for a while is asked again
after 2, 5, then 15 seconds (`images::backoff`; a 429's own wait when longer), a preview after
a minute; a passing failure of one shape leaves the other free. A frame draws at most
`EMOJI_PER_FRAME` (32) distinct emoji pictures, counted in the order it lays them out; a
message only measured to place the view gives its part back (`Images::not_shown`) and is laid
out again if it is shown after all, so only what is shown counts. The rest stay `:name:`.

**Downloads** (`download.rs`, `http/fetch.rs`): a file a message shares is downloaded only
when the user asks (`Command::Download`), at most two at once a workspace. Only an address
on the file host is fetched (`https://` + `FILE_HOST` + a plain path; `download::address`):
one `GET` with the browser's headers for a download (no `Origin`, the web client as
`Referer`) and the session's cookie, never the token. A redirect is followed (at most 3) only
to another address on the file host; one anywhere else fails without being followed, so the
cookie never leaves that host. Each step passes the kill switch and counts as a request the
user waits for (`Governor::fetch`); the bytes do not count. A file that says it is larger than
the cap (`download_max_mb`) is not asked for, an answer that says so is not read, and the
bytes are counted as they come and stop at the cap; a web page where a file was asked for (a
sign-in page) is not saved; an answer cut off before its length is not saved. Saving is the
core's (`slakio_core::download`): a hidden part file made new in the folder, renamed to the
first free name (`name (2).ext`) without replacing anything, the name the remote one
sanitised and made plain (no folders, no leading dot or dash, no reserved Windows name, at
most 120 bytes, its extension kept); a part file not finished is removed, and those of runs
that are gone are swept at the start. A folder slakio makes for it is 0700, the part file 0600.
The bytes are written, flushed and renamed on a blocking-pool thread (`Writer`), never on the
runtime that draws. Progress comes as `Event::Progress` (it ends nothing) and the answer is
`Event::Downloaded` or `Failed`. `Command::Cancel` stops a download still receiving (answered
`Event::Cancelled`; the download itself `Failed(Cancelled)`, nothing kept).

**Live updates** (`live.rs`, `slakio_slack::serve_live`, what the binary runs): once a workspace
has booted, one WebSocket to the realtime host, opened as the web client opens it (the token in
its address, the browser's smaller header set for a socket, the `d` cookie). It only receives:
the client writes nothing on it but a pong for each ping and a close (frames read and written by
hand, `live/socket.rs`: masked, no extension, a message at most 4 MiB, anything a server must not
send fails it). Messages posted, changed and deleted and conversations read elsewhere
(`*_marked`) are pushed to the UI as `Event::Live` (generation `Generation::PUSHED`, no request
asked for it). They go out in order through a task of their own, so reading the socket (and
answering its pings) never waits for a lookup: new authors of a conversation the UI read are
looked up together (in the background share, each once a run, an id Slack does not name kept as
such); those elsewhere are not looked up. Everything else on the socket is left unread. A socket
that drops, closes or is silent for 60 seconds opens again after a pause doubling from 1 to 30
seconds, each a random half to all of it, never shorter than a `429`'s `Retry-After`, reset once
one said hello and stayed open 30 seconds; each opening goes past the kill switch and counts
against the request cap and the workspace's part of the background share (`Governor::dial`).
Switching the network off ends the socket; switched on again, it opens again and catches up. A
socket open while the machine slept (the wall clock ran more than 10 s ahead of the monotonic
one between two checks 5 s apart, `Realtime::slept`) is taken as lost at once and opens again
without the doubled pause. After a loss, Slack's unread counts are read again once
(`client.counts`, background share, `Live::Counts`; a conversation not listed has none, as at a
boot); `live = "off"` opens none
(`slakio_slack::serve_with`). The socket replays nothing it missed, so when one opens after it
was said to be lost or not connected the UI is told to catch up (`Connection::Connected { catch_up: true }`) and asks
for the newest page of each conversation open again (`Command::Catchup`, at most 8, one page each,
`conversations.history` in the background share of the budget, never an open), then of each
thread open (at most 4, not one read only in part: what is new in it through `conversations.replies`), each page the
truth for the span it covers, the whole timeline replaced when it does not reach back to what was
loaded when it was asked (no gap is hidden), what came live meanwhile kept after it; one that
fails is asked again by itself after a pause that doubles up to 15 minutes. Each workspace's
socket is its own; one that fails holds none back, and the background share is split so it
cannot spend another's either (`Limits::lane_background_*`); its previews keep to their part of
that share (`Limits::lane_picture_*`) and its photos and emoji to the picture hosts' budget, so
a reopening never waits for them.

The backend (`slakio_slack::serve`) owns a `BackendEnd`: every request gets one answer, a change
is refused before any request, and a newer page of the same conversation or thread supersedes
the one in flight: that one is cancelled (before it is sent, or by dropping its answer) and
answered `Busy`. Pages of different targets never cancel each other, so the panes of a restored
layout each load; the governor paces them. Opening a conversation waits 200 ms first. People are named by id lookups, never a directory listing: a view brings some of its
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
written out as Slack markup from its blocks; attachments follow the text (`blocks.rs`); the
files it shares are its own (`Message::files`: id, name, kind, size, and the download address
when it is on the file host and the file is hosted there, not deleted or hidden); a bot is
named as it posts. A read-only backend offers no composer: nothing is
ever shown as sent. The binary runs one for
each signed-in workspace, behind one handle (`slakio_tui::many`, below); the tests run it
against `slakio-fake` (one fake per workspace, `FakeSlack::start_team`). The HTTP stack is chosen in [ADR 0001](architecture/0001-http-stack.md).

## The local cache

`slakio-store` keeps, in `<cache>/slakio-store/cache.sqlite3`, the last boot snapshot (replaced whole by each
boot; a workspace it no longer lists leaves the cache with it) and the newest messages of each
conversation and thread opened (each page the backend answers is the truth for the span of time
it covers: what the cache held there and the page lacks is gone; a thread is a place of its own,
`<conversation>:<thread>`, capped and aged as a conversation is). Remote text is kept as it came and read
back as `Remote`, so it still reaches the screen only through the sanitiser. A message's files
are kept as the message lists them (name, kind, size, address), never their bytes. No secret is
kept.

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

`crates/slakio-tui/src/theme.rs` holds the colour tokens and the styles made of them; widgets
name roles, never colours. The built-ins are the terminal's own 16 colours, `dark`, `light`,
`high-contrast`, `nord`, `dracula` and three families (`catppuccin`, `tokyo-night`, `gruvbox`)
whose light or dark variant follows the terminal's background, asked once with OSC 11; `auto`
takes `tokyo-night` on a terminal that says it shows 24-bit colour. `:theme <name>` resolves a
name again with what was found at startup (`theme::Look`) and saves it to the config file. Every
24-bit colour theme passes WCAG contrast checks (`theme/tests.rs`): body text 4.5:1 on every surface,
muted text and marks 3:1, pills, mode badges and the initials on avatar chips 4.5:1 (4.3:1 on
a dark theme's tinted chip). An avatar chip (`avatar.rs`: initials and a colour slot hashed
from the person's id) takes one of the theme's avatar hues, none in the red family (red is the
mention pills'); a 24-bit colour theme draws it as a tint of the hue on the background with the
hue as the initials (darkened on a light theme), never brighter than the selection bar or the
accent, and the 16-colour theme as coloured initials without a background: a solid block is a
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
