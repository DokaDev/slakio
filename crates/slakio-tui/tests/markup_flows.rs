//! Messages drawn as GUI Slack draws Slack's markup: bold, italic, struck text and code in their
//! styles, people and channels by name (the user named in the warm color), links by their text,
//! a code block cut and never wrapped, quoted lines under a bar; at 120x40 and 80x24. Hostile
//! text inside markup is still drawn harmless.

#[path = "support/demo.rs"]
mod demo;
#[path = "support/scripted.rs"]
#[expect(dead_code, reason = "the shared harness has more than these tests use")]
mod scripted;

use demo::assert_harmless;
use ratatui::style::Modifier;
use scripted::Scripted;
use slakio_core::backend::Event;
use slakio_core::model::{ConversationKind, Message, Ts};
use slakio_core::sanitize::Remote;

/// Booted, `deploys` opened, its page answered with messages of `texts` (oldest first).
fn shown(texts: &[String], w: u16, h: u16) -> Scripted {
    let mut s = Scripted::booted(true, None);
    s.d.app.resize(w, h);
    s.d.list_cursor_on("deploys");
    s.keys("enter");
    let mut e = s.b.reply(0);
    let Event::History(page) = &mut e.event else { panic!("a page") };
    let author = page.messages[0].user.clone();
    page.messages = texts
        .iter()
        .enumerate()
        .map(|(i, t)| Message {
            ts: Ts(1_767_603_600_000_000 + i as u64 * 600_000_000),
            user: author.clone(),
            text: Remote::new(t.clone()),
            thread: None,
            reactions: Vec::new(),
            edited: false,
        })
        .collect();
    page.complete = true;
    s.b.push(e);
    s.pump();
    s
}

/// The ids of the user and of another person of the first workspace, and a channel's.
fn ids(s: &Scripted) -> (String, String, String) {
    let m = &s.d.app.model;
    let ws = &m.workspaces()[0].id;
    let me = m.me(ws).unwrap().to_string();
    let other = m.conversations().iter().find_map(|c| match &c.kind {
        ConversationKind::Dm { user } if &c.workspace == ws => Some(user.to_string()),
        _ => None,
    });
    let channel = m.conversations().iter().find(|c| c.name == "alerts" && &c.workspace == ws).unwrap().id.to_string();
    (me, other.unwrap(), channel)
}

fn texts(me: &str, other: &str, channel: &str) -> Vec<String> {
    vec![
        format!(
            "*Deploy* of _api_ is ~cancelled~ done: run `make ship` &amp; ask <@{other}> or <@{me}> in <#{channel}>"
        ),
        "see <https://docs.example.com/runbooks/deploy|the runbook> or <https://status.example.com> <!here> :tada:"
            .to_string(),
        format!(
            "```\nfn main() {{ println!(\"a very long line of code that never wraps in a narrow pane at all\"); }}\n    *not bold*\n```"
        ),
        "&gt; quoted *line* that is long enough to wrap under its bar when the pane is narrow\nafter the quote"
            .to_string(),
    ]
}

/// The style of the first cell of the first `needle` on screen.
fn style_of(s: &Scripted, needle: &str) -> ratatui::style::Style {
    let screen = s.d.screen();
    let (y, line) =
        screen.lines().enumerate().find(|(_, l)| l.contains(needle)).unwrap_or_else(|| panic!("{needle}: {screen}"));
    let x = ratatui::text::Span::raw(&line[..line.find(needle).unwrap()]).width() as u16;
    s.d.buffer()[(x, y as u16)].style()
}

#[test]
fn markup_is_drawn_in_its_styles_at_120x40_and_80x24() {
    for (w, h) in [(120, 40), (80, 24)] {
        let probe = Scripted::booted(true, None);
        let (me, other, channel) = ids(&probe);
        let s = shown(&texts(&me, &other, &channel), w, h);
        let screen = s.d.screen();
        let at = format!("{w}x{h}: {screen}");
        for gone in ["*Deploy*", "&amp;", "<@", "<#", "<https", "&gt;", "```", ":tada:"] {
            assert!(!screen.contains(gone), "{gone} is markup: {at}");
        }
        assert!(screen.contains("#alerts"), "{at}");
        assert!(screen.contains("the runbook"), "{at}");
        assert!(screen.contains("@here"), "{at}");
        assert!(screen.contains("    *not bold*"), "code is as written: {at}");
        assert!(screen.contains("▎ quoted"), "{at}");
        assert!(screen.contains("…"), "a code line wider than the pane is cut: {at}");
        assert!(style_of(&s, "Deploy").add_modifier.contains(Modifier::BOLD), "{at}");
        assert!(style_of(&s, "api").add_modifier.contains(Modifier::ITALIC), "{at}");
        assert!(style_of(&s, "cancelled").add_modifier.contains(Modifier::CROSSED_OUT), "{at}");
        let t = &s.d.app.theme;
        assert_eq!(style_of(&s, "make").fg, t.code().fg, "{at}");
        assert_eq!(style_of(&s, "the runbook").fg, t.link().fg, "{at}");
        assert!(style_of(&s, "the runbook").add_modifier.contains(Modifier::UNDERLINED), "{at}");
        assert!(screen.contains("`make"), "the 16-color theme keeps the backticks: {at}");
        let me_name =
            s.d.app.model.user(&s.d.app.model.workspaces()[0].id, &slakio_core::model::UserId::new(&me)).unwrap();
        let me_name = format!("@{}", me_name.display_name.line().as_str());
        assert_eq!(style_of(&s, &me_name).fg, t.mention(true).fg, "the user named stands out: {at}");
        insta::assert_snapshot!(format!("markup_{w}x{h}"), s.d.snap());
    }
}

#[test]
fn hostile_text_inside_markup_is_drawn_harmless() {
    let texts: Vec<String> = slakio_world::hostile_strings()
        .into_iter()
        .take(12)
        .map(|h| format!("*{0}* `{0}` <https://x.example|{0}> &gt; {0}", h.text))
        .collect();
    let s = shown(&texts, 120, 40);
    assert_harmless(&s.d.screen());
}

/// Code, links and mentions read apart in tokyo-night and under NO_COLOR: a link underlined,
/// code tinted (without color, inline code keeps its backticks and a code block's lines a bar),
/// never in the user's mention style.
#[test]
fn code_links_and_mentions_read_apart_with_and_without_color() {
    use slakio_tui::theme::{Background, Theme};
    let probe = Scripted::booted(true, None);
    let (me, other, channel) = ids(&probe);
    let me_name = probe.d.app.model.user(&probe.d.app.model.workspaces()[0].id, &slakio_core::model::UserId::new(&me));
    let me_name = format!("@{}", me_name.unwrap().display_name.line().as_str());
    for theme in [Theme::no_color(), slakio_tui::theme::resolve("tokyo-night", true, Background::Dark)] {
        let mut s = shown(&texts(&me, &other, &channel), 120, 40);
        s.d.app.theme = theme.clone();
        let screen = s.d.screen();
        let at = format!("{}: {screen}", theme.name);
        let (code, link, mine) = (style_of(&s, "make"), style_of(&s, "the runbook"), style_of(&s, &me_name));
        assert!(link.add_modifier.contains(Modifier::UNDERLINED), "{at}");
        assert_ne!((code.fg, code.bg, code.add_modifier), (mine.fg, mine.bg, mine.add_modifier), "{at}");
        if theme.code_marks() {
            assert!(screen.contains("`make ship`"), "{at}");
        } else {
            assert_eq!(code.bg, Some(theme.code_bg()), "{at}");
            assert!(!screen.contains("`make"), "{at}");
        }
        // A code block's lines: tinted, or, where nothing tints them, after a bar.
        let block = screen.lines().find(|l| l.contains("fn main")).unwrap_or_else(|| panic!("{at}"));
        if theme.kind == slakio_tui::theme::Kind::NoColor {
            assert!(block.contains("│ fn main"), "{at}");
        } else {
            assert!(!block.contains("│ fn main"), "{at}");
            assert_eq!(style_of(&s, "fn main").bg, Some(theme.code_bg()), "{at}");
        }
    }
}

/// A link whose label names another address shows the real host after it, faint and not
/// underlined (no markup gives that style); a forged annotation in the label stays in the link;
/// a `javascript:` link is plain text; a bot's per-name id is no mention, and an app named is
/// marked.
#[test]
fn a_link_s_real_host_is_a_span_of_its_own_and_ids_cannot_pass_for_people() {
    let texts = vec![
        "<https://evil.example/login|https://bank.example (bank.example)> and <javascript:alert(1)|click here>"
            .to_string(),
        "<@BHOOK~3f00000000000000> wrote, then <@UAPP1> did".to_string(),
    ];
    let mut s = shown(&texts, 120, 40);
    let ws = s.d.app.model.workspaces()[0].id.clone();
    let app = slakio_core::model::User {
        id: slakio_core::model::UserId::new("UAPP1"),
        workspace: ws.clone(),
        name: Remote::new("Deploy Bot".to_string()),
        display_name: Remote::new("Deploy Bot".to_string()),
        org: slakio_core::model::Org::Own,
        bot: true,
        presence: slakio_core::model::Presence::Unknown,
    };
    s.d.app.model.add_users(&ws, &[app]);
    let screen = s.d.screen();
    let t = &s.d.app.theme;
    let real = style_of(&s, "→ evil.example");
    assert_eq!(real.fg, t.faint().fg, "{screen}");
    assert!(!real.add_modifier.contains(Modifier::UNDERLINED), "{screen}");
    assert!(style_of(&s, "(bank.example)").add_modifier.contains(Modifier::UNDERLINED), "the label's own text");
    let click = style_of(&s, "click here");
    assert!(!click.add_modifier.contains(Modifier::UNDERLINED) && click.fg == t.text().fg, "{screen}");
    assert!(screen.contains("@BHOOK~3f00000000000000"), "an id, not a name: {screen}");
    let mark = style_of(&s, "@Deploy Bot app");
    assert!(mark.add_modifier.contains(Modifier::BOLD), "{screen}");
    assert_eq!(style_of(&s, " app did").fg, t.faint().fg, "the app's mark is faint, apart: {screen}");
}
