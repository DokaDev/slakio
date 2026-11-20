use super::*;

fn env(pairs: &[(&str, &str)]) -> Env {
    let pairs: Vec<(String, String)> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Env::read(move |k| pairs.iter().find(|(name, _)| name == k).map(|(_, v)| v.clone()))
}

fn auto(pairs: &[(&str, &str)]) -> bool {
    images_on(&env(pairs), "auto", false)
}

#[test]
fn kitty_and_ghostty_draw_pictures() {
    assert!(auto(&[("TERM", "xterm-kitty")]));
    assert!(auto(&[("TERM", "xterm-256color"), ("KITTY_WINDOW_ID", "1")]));
    assert!(auto(&[("TERM", "xterm-ghostty")]));
    assert!(auto(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "ghostty")]));
}

#[test]
fn other_terminals_and_unsure_hints_draw_text() {
    for term in ["xterm-256color", "alacritty", "dumb", ""] {
        assert!(!auto(&[("TERM", term)]), "{term}");
    }
    assert!(!auto(&[]));
    // Ghostty's resources folder leaks into a multiplexer's panes: alone it says nothing.
    assert!(!auto(&[("TERM", "xterm-256color"), ("GHOSTTY_RESOURCES_DIR", "/x")]));
    assert!(!auto(&[("TERM", "xterm-256color"), ("KITTY_WINDOW_ID", "")]), "an empty variable is unset");
}

#[test]
fn herdr_inside_ghostty_draws_text() {
    let herdr = [("TERM", "xterm-256color"), ("TERM_PROGRAM", "herdr"), ("GHOSTTY_RESOURCES_DIR", "/Applications/x")];
    assert!(!auto(&herdr));
    assert!(!auto(&[("TERM", "xterm-ghostty"), ("HERDR_ENV", "1")]));
}

#[test]
fn inside_any_multiplexer_pictures_are_not_drawn() {
    for mux in [
        &[("TERM", "xterm-kitty"), ("TMUX", "/tmp/tmux-1/default,1,0")][..],
        &[("TERM", "tmux-256color"), ("KITTY_WINDOW_ID", "1")],
        &[("TERM", "screen-256color"), ("TERM_PROGRAM", "ghostty")],
        &[("TERM", "xterm-kitty"), ("STY", "1.pts")],
        &[("TERM", "xterm-ghostty"), ("ZELLIJ", "0")],
        &[("TERM", "xterm-ghostty"), ("ZELLIJ_SESSION_NAME", "s")],
        &[("TERM", "xterm-kitty"), ("TERM_PROGRAM", "tmux")],
    ] {
        assert!(!auto(mux), "{mux:?}");
    }
}

#[test]
fn the_override_beats_detection_both_ways_and_the_variable_beats_the_setting() {
    let kitty = [("TERM", "xterm-kitty")];
    let herdr = [("TERM", "xterm-256color"), ("TERM_PROGRAM", "herdr")];
    assert!(!images_on(&env(&kitty), "off", false), "off in kitty");
    assert!(images_on(&env(&herdr), "on", false), "on in a multiplexer: the user's choice");
    assert!(images_on(&env(&[("TERM", "xterm-256color"), (IMAGES_ENV, "on")]), "auto", false));
    assert!(!images_on(&env(&[("TERM", "xterm-kitty"), (IMAGES_ENV, "off")]), "on", false), "the variable wins");
    assert!(images_on(&env(&[("TERM", "xterm-kitty"), (IMAGES_ENV, "auto")]), "off", false), "the variable wins");
    for v in ["OFF", "no", "1", "yes", "true"] {
        assert!(!images_on(&env(&[("TERM", "xterm-kitty"), (IMAGES_ENV, v)]), "auto", false), "{v}: not understood");
    }
    assert!(images_on(&env(&[("TERM", "xterm-kitty"), (IMAGES_ENV, "")]), "auto", false), "empty: the setting");
}

#[test]
fn the_demo_draws_pictures_only_when_asked() {
    let kitty = [("TERM", "xterm-kitty")];
    assert!(!images_on(&env(&kitty), "auto", true));
    assert!(images_on(&env(&[("TERM", "xterm-kitty"), (IMAGES_ENV, "on")]), "auto", true));
    assert!(images_on(&env(&kitty), "on", true));
}
