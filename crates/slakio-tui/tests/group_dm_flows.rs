//! A group DM is named by two of its people and how many more (`Ann, Bob and 6 others`, ko
//! its `group.others` form), in its list row and its pane's title, and starts with how many people are
//! in it (`9+` from ten): a chip with avatars, the number with avatars off. A group DM not named
//! by its people keeps its name. At 80x24 and 120x40, en and ko.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_core::i18n::Lang;
use slakio_core::model::{ConversationKind, User};
use slakio_core::sanitize::Remote;
use slakio_tui::app::Settings;
use slakio_tui::app::shell::View;
use slakio_world::World;

const SIZES: [(u16, u16); 2] = [(80, 24), (120, 40)];
const LANGS: [Lang; 2] = [Lang::En, Lang::Ko];

const NAMES: [&str; 12] = ["Ann", "Bob", "Cy", "Dee", "Eve", "Fay", "Gus", "Hal", "Ivy", "Jo", "Kai", "Lu"];

/// The demo with its group DM of `n` people (the user left out, short names), named by them as
/// the boot names it, with `unread` messages; their names.
fn with_group(w: u16, h: u16, lang: Lang, avatars: bool, (n, unread): (usize, u32)) -> (Demo, Vec<String>) {
    let mut d = Demo::with(w, h, lang, Settings { avatars, ..Settings::default() });
    let mut snap = World::demo().snapshot().clone();
    let at = snap.conversations.iter().position(|c| matches!(c.kind, ConversationKind::GroupDm { .. })).unwrap();
    let ws = snap.conversations[at].workspace.clone();
    let me = snap.workspaces.iter().find(|w| w.id == ws).unwrap().me.clone();
    let people: Vec<&mut User> =
        snap.users.iter_mut().filter(|u| u.workspace == ws && u.id != me && !u.bot).take(n).collect();
    assert_eq!(people.len(), n, "enough people in the demo");
    let mut users = Vec::new();
    for (u, name) in people.into_iter().zip(NAMES) {
        u.display_name = Remote::new(name);
        users.push(u.id.clone());
    }
    let names: Vec<String> = NAMES.iter().take(n).map(ToString::to_string).collect();
    let c = &mut snap.conversations[at];
    c.kind = ConversationKind::GroupDm { users };
    c.name = Remote::new(names.join(", "));
    (c.unread, c.mentions) = (unread, 0);
    d.app.model.merge(&ws, &snap, false);
    d.app.show_unfolded(View::Dms);
    (d, names)
}

/// The list panel's row starting with `lead` after its border, as drawn (left of the panel's
/// right border), the blanks before `lead` and its count pill left out.
fn row(screen: &str, lead: &str) -> String {
    let line = screen
        .lines()
        .map(|l| l.split("││").next().unwrap_or_default().trim_start_matches('│').trim_start())
        .find(|l| l.starts_with(lead))
        .unwrap_or_else(|| panic!("no row {lead:?}: {screen}"));
    let text = line.trim_end_matches('│').trim_end();
    let (head, rest) = text.split_at(lead.len());
    let rest = rest.rsplit_once("  ").map_or(rest, |(name, _pill)| name.trim_end());
    format!("{head}{rest}")
}

fn others(lang: Lang, names: &str, n: usize) -> String {
    match (lang, n) {
        (Lang::En, 1) => format!("{names} and 1 other"),
        (Lang::En, n) => format!("{names} and {n} others"),
        (Lang::Ko, n) => format!("{names} \u{c678} {n}\u{ba85}"),
    }
}

/// Whether the cell of `d`'s screen at the start of the list row starting with `lead` is bold.
fn lead_bold(d: &Demo, lead: &str) -> bool {
    use ratatui::style::Modifier;
    let screen = d.screen();
    let y = screen.lines().position(|l| l.trim_start_matches('│').trim_start().starts_with(lead)).unwrap() as u16;
    let buf = d.buffer();
    let x = (0..buf.area.width).find(|&x| !buf[(x, y)].symbol().trim().is_empty() && buf[(x, y)].symbol() != "│");
    buf[(x.unwrap(), y)].modifier.contains(Modifier::BOLD)
}

#[test]
fn with_avatars_off_a_group_dm_starts_with_its_count() {
    for ((w, h), lang) in SIZES.into_iter().flat_map(|s| LANGS.map(|l| (s, l))) {
        let (d, _) = with_group(w, h, lang, false, (8, 0));
        let screen = d.screen();
        let at = format!("{w}x{h} {lang:?}: {screen}");
        assert_eq!(row(&screen, "8  "), format!("8  {}", others(lang, "Ann, Bob", 6)), "{at}");
        assert!(!screen.contains("@ Ann"), "no `@` before it: {at}");
        assert!(!lead_bold(&d, "8  "), "read: not bold: {at}");
        // Unread: the count bold; the name cut to one and the others, the pill beside it.
        let (d, _) = with_group(w, h, lang, false, (8, 2));
        let screen = d.screen();
        let text = row(&screen, "8  ");
        let fitted = [others(lang, "Ann, Bob", 6), others(lang, "Ann", 7)];
        assert!(fitted.iter().any(|f| text == format!("8  {f}")), "{text:?} {w}x{h} {lang:?}: {screen}");
        assert!(lead_bold(&d, "8  "), "unread: bold: {w}x{h} {lang:?}: {screen}");
        // Twelve people: `9+`, its name one cell closer.
        let (d, _) = with_group(w, h, lang, false, (12, 0));
        let screen = d.screen();
        assert_eq!(row(&screen, "9+ "), format!("9+ {}", others(lang, "Ann, Bob", 10)), "{w}x{h} {lang:?}: {screen}");
    }
}

#[test]
fn with_avatars_on_a_group_dm_starts_with_its_count_chip() {
    for (w, h) in SIZES {
        let (d, _) = with_group(w, h, Lang::En, true, (12, 0));
        let screen = d.screen();
        let text = row(&screen, "9+  ");
        assert_eq!(text, "9+  Ann, Bob and 10 others", "{w}x{h}: {screen}");
        let y = screen.lines().position(|l| l.contains(&text)).unwrap() as u16;
        let buf = d.buffer();
        let x = (0..buf.area.width).find(|&x| buf[(x, y)].symbol() == "9").unwrap();
        assert_eq!(buf[(x + 1, y)].symbol(), "+");
        assert_eq!(buf[(x, y)].style(), buf[(x + 1, y)].style(), "the chip is two cells of one style");
        assert_ne!(buf[(x, y)].style(), buf[(x + 4, y)].style(), "the name is not in the chip's style");
    }
}

#[test]
fn a_group_dm_is_named_two_people_and_n_others() {
    for ((w, h), lang) in SIZES.into_iter().flat_map(|s| LANGS.map(|l| (s, l))) {
        let at = |screen: &str| format!("{w}x{h} {lang:?}: {screen}");
        // Two people: both named, as before.
        let (d, _) = with_group(w, h, lang, false, (2, 0));
        let screen = d.screen();
        assert_eq!(row(&screen, "2  "), "2  Ann, Bob", "{}", at(&screen));
        // Three: two and one other.
        let (d, _) = with_group(w, h, lang, false, (3, 0));
        let screen = d.screen();
        assert_eq!(row(&screen, "3  "), format!("3  {}", others(lang, "Ann, Bob", 1)), "{}", at(&screen));
        assert!(!screen.contains("Ann, Bob, Cy"), "{}", at(&screen));
    }
}

#[test]
fn a_group_dm_header_and_tab_name_two_people_and_the_rest_after_the_count() {
    for ((w, h), lang) in SIZES.into_iter().flat_map(|s| LANGS.map(|l| (s, l))) {
        for avatars in [false, true] {
            let (mut d, names) = with_group(w, h, lang, avatars, (8, 0));
            d.open(&names.join(", "));
            let screen = d.screen();
            let at = format!("{w}x{h} {lang:?} avatars {avatars}: {screen}");
            let name = format!("@{}", others(lang, "Ann, Bob", 6));
            let top = screen.lines().next().unwrap();
            assert!(top.contains(&format!("\u{258c}8  {name} ")), "{at}");
            let buf = d.buffer();
            let x = (0..buf.area.width).find(|&x| buf[(x, 0)].symbol() == "8").unwrap();
            // A chip with avatars (two cells of the list's group chip), the muted number without.
            let t = &d.app.theme;
            let want = if avatars { t.avatar_group() } else { t.muted() };
            for cell in [&buf[(x, 0)], &buf[(x + 1, 0)]] {
                assert_eq!((Some(cell.fg), want.bg.map(|_| cell.bg)), (want.fg, want.bg), "{at}");
            }
            assert_ne!(buf[(x, 0)].style(), buf[(x + 3, 0)].style(), "the name is not in the badge's style: {at}");
            // The divider and the status line name it so too.
            assert!(screen.contains(&format!(" {name} ─")), "{at}");
            assert!(d.status_line().contains(&name), "{at}");
            assert!(!screen.contains(&names.join(", ")), "nowhere by all its names: {at}");
        }
    }
}

#[test]
fn a_group_dm_not_named_by_its_people_keeps_its_name() {
    for lang in LANGS {
        let (mut d, _) = with_group(120, 40, lang, false, (8, 0));
        let snap = {
            let mut s = World::demo().snapshot().clone();
            let c = s.conversations.iter_mut().find(|c| matches!(c.kind, ConversationKind::GroupDm { .. })).unwrap();
            c.name = Remote::new("ann, bob, cy, dee");
            s
        };
        let ws = snap.conversations.iter().find(|c| c.name == "ann, bob, cy, dee").unwrap().workspace.clone();
        d.app.model.merge(&ws, &snap, false);
        let screen = d.screen();
        assert_eq!(row(&screen, "3  "), "3  ann, bob, cy, dee", "{lang:?}: {screen}");
    }
}
