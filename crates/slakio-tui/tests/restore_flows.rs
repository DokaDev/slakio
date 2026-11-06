//! The tabs and panes of one run opened again by the next, over the demo world: the layout an
//! app would save, written as the file's text and read back, opens the same tabs (names,
//! order, the tab shown), the same splits and shares, each pane on what it showed, the thread
//! panel beside its pane, the zoom and the pane with the keyboard. A pane whose conversation is
//! gone is left out, said once; the layout waits for the boot, and is not opened over what the
//! user opened first.

#[path = "support/demo.rs"]
mod demo;

use demo::Demo;
use slakio_core::i18n::Label;
use slakio_core::layout::saved::Saved;
use slakio_core::model::{ConversationId, Target};
use slakio_tui::app::PaneKind;

/// What each pane on screen shows, in reading order, and its area.
fn shown(d: &Demo) -> Vec<(String, ratatui::layout::Rect)> {
    d.app
        .panes_on_screen()
        .into_iter()
        .map(|(p, r)| {
            let c = d.app.model.target(p.target()).expect("a conversation");
            let kind = if p.kind() == PaneKind::Thread { "thread of " } else { "" };
            (format!("{kind}{}", c.name.line().as_str()), r)
        })
        .collect()
}

/// Three panes in a renamed tab, a resized border, a thread panel, and a second tab shown.
fn built(w: u16, h: u16) -> Demo {
    let mut d = Demo::new(w, h);
    d.open("backend");
    d.keys("v");
    d.keys("s");
    d.keys("space w >");
    d.keys("space w h");
    d.keys("k");
    d.keys("enter");
    d.command("rename Incident");
    d.open_in_tab("deploys");
    d.keys("space 1");
    d
}

/// `saved` written as the file holds it and read back.
fn through_the_file(saved: &Saved) -> Saved {
    Saved::parse(&saved.to_text()).expect("reads back")
}

#[test]
fn the_next_run_opens_the_same_tabs_and_panes() {
    for (w, h) in [(120u16, 40u16), (80, 24)] {
        let before = built(w, h);
        let saved = before.app.layout_to_save().expect("a layout");
        assert_eq!(saved.tabs.len(), 2);
        assert_eq!(saved.tabs[0].name.as_deref(), Some("Incident"));
        let mut after = Demo::new(w, h);
        after.app.restore_layout(through_the_file(&saved), after.now);
        after.pump();
        assert_eq!(after.app.layout_to_save(), Some(saved.clone()), "{w}x{h}: saved again the same");
        assert_eq!(shown(&after), shown(&before), "{w}x{h}");
        assert_eq!(after.app.focused_pane().map(|p| p.kind()), before.app.focused_pane().map(|p| p.kind()));
        // The same work area (the list's cursor is where the user left it, not saved).
        let work = |d: &Demo| {
            let x = usize::from(d.app.areas().work.x);
            d.snap().lines().map(|l| l.chars().skip(x).collect::<String>()).collect::<Vec<_>>()
        };
        assert_eq!(work(&after), work(&before), "{w}x{h}: the same work area");
        // The other tab is there too, in its place.
        after.keys("space 2");
        before_keys(&mut built(w, h), &after);
    }
}

/// The second tab of a fresh `built` shows what `after`'s does.
fn before_keys(before: &mut Demo, after: &Demo) {
    before.keys("space 2");
    assert_eq!(shown(after), shown(before));
}

#[test]
fn a_zoomed_tab_comes_back_zoomed_on_its_pane() {
    let mut d = built(120, 40);
    d.keys("z");
    let saved = d.app.layout_to_save().unwrap();
    assert!(saved.tabs[0].zoomed);
    let mut after = Demo::new(120, 40);
    after.app.restore_layout(through_the_file(&saved), after.now);
    assert!(after.screen().contains("[zoomed]"), "{}", after.screen());
    assert_eq!(shown(&after), shown(&d));
}

#[test]
fn a_pane_whose_conversation_is_gone_is_left_out_said_once() {
    let d = built(120, 40);
    let mut saved = d.app.layout_to_save().unwrap();
    // The second tab's conversation, and one pane of the first, no longer exist.
    let gone = |t: &mut Target| match t {
        Target::Conversation { conversation, .. } | Target::Thread { conversation, .. } => {
            *conversation = ConversationId::new("CGONE");
        }
    };
    gone(&mut saved.tabs[1].panes[0].target);
    let last = saved.tabs[0].panes.len() - 1;
    gone(&mut saved.tabs[0].panes[last].target);
    let mut after = Demo::new(120, 40);
    after.app.restore_layout(saved.clone(), after.now);
    let now = after.app.layout_to_save().unwrap();
    assert_eq!(now.tabs.len(), 1, "a tab left with nothing is left out");
    assert_eq!(now.tabs[0].panes.len(), saved.tabs[0].panes.len() - 1);
    let notice = after.app.i18n.label(Label::StatusLayoutDropped).to_string();
    assert!(after.status_line().contains(&notice), "{}", after.status_line());
    assert_eq!(after.screen().matches(&notice).count(), 1);
}

#[test]
fn the_layout_waits_for_the_boot_and_never_opens_over_what_the_user_opened() {
    let saved = built(120, 40).app.layout_to_save().unwrap();
    // Given before the backend answered: saved as it waits, opened when the boot comes.
    let mut d = Demo::new(120, 40);
    let mut early = slakio_tui::app::App::new(slakio_core::i18n::Lang::En, slakio_tui::theme::Theme::terminal());
    early.resize(120, 40);
    std::mem::swap(&mut d.app, &mut early);
    d.app.connect(slakio_core::backend::Backend::capabilities(&d.backend));
    d.app.restore_layout(saved.clone(), d.now);
    assert_eq!(d.app.layout_to_save(), Some(saved.clone()), "waiting, it is what is saved");
    assert!(d.app.open_panes().is_empty());
    d.pump();
    assert_eq!(d.app.layout_to_save(), Some(saved.clone()));
    assert!(!d.app.open_panes().is_empty());
    // The user opened something first: that stays, the layout is not opened over it.
    let mut d = Demo::new(120, 40);
    d.open("incidents");
    let mine = d.app.layout_to_save();
    d.app.restore_layout(saved, d.now);
    assert_eq!(d.app.layout_to_save(), mine);
}
