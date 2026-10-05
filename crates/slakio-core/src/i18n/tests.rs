use super::*;

/// The Korean catalog's text for `key`, read from `locales/ko.toml` when the test runs: the
/// tests hold no Korean text of their own.
fn ko_text(key: &str) -> String {
    let table: toml::Table = include_str!("../../../../locales/ko.toml").parse().expect("ko.toml");
    table[key].as_str().expect("a string").to_string()
}

#[test]
fn labels_and_messages_render_in_each_language() {
    let mut i = I18n::new(Lang::Ko);
    assert_eq!(i.label(Label::ActionQuit), ko_text("action.quit").as_str());
    assert_eq!(
        i.msg(&Msg::CommandUnknown { name: "wq".into() }),
        ko_text("command.unknown").replace("{name}", "wq").as_str()
    );
    i.set_lang(Lang::En);
    assert_eq!(i.label(Label::ActionQuit), "Quit");
    assert_eq!(i.msg(&Label::ModeCommand.into()), "COMMAND");
    assert_eq!(Msg::CommandUnknown { name: "x".into() }.key(), "command.unknown");
}

#[test]
fn every_message_fills_every_placeholder() {
    // The build checks that en and ko have the same placeholders per key; this checks the
    // generated rendering: every argument shows up formatted and no `{name}` is left over.
    for lang in [Lang::En, Lang::Ko] {
        for &l in Label::ALL {
            assert!(!l.text(lang).is_empty(), "{lang:?} {}", l.key());
        }
        for m in Msg::samples() {
            let text = m.render(lang);
            let en = m.render(Lang::En);
            for arg in ["1,234,567", "1.2s"].iter().copied().chain(sample_texts(&en)) {
                assert_eq!(text.matches(arg).count(), en.matches(arg).count(), "{lang:?} {}: {text}", m.key());
            }
            let leftover = text.split('{').skip(1).any(|t| {
                t.split_once('}')
                    .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
            });
            assert!(!leftover, "{lang:?} {}: unfilled placeholder in {text}", m.key());
        }
    }
}

/// The `<name>` texts the samples pass for string placeholders.
fn sample_texts(s: &str) -> Vec<&str> {
    s.match_indices('<').filter_map(|(i, _)| s[i..].find('>').map(|j| &s[i..=i + j])).collect()
}

#[test]
fn count_and_elapsed() {
    assert_eq!(fmt_count(0), "0");
    assert_eq!(fmt_count(999), "999");
    assert_eq!(fmt_count(1000), "1,000");
    assert_eq!(fmt_count(4_000_000), "4,000,000");
    assert_eq!(fmt_elapsed(Duration::from_millis(123)), "123ms");
    assert_eq!(fmt_elapsed(Duration::from_millis(1234)), "1.2s");
}

#[test]
fn language_detection() {
    let env = |pairs: &'static [(&'static str, &'static str)]| {
        move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    };
    assert_eq!(detect_lang("auto", env(&[("LANG", "ko_KR.UTF-8")])), Lang::Ko);
    assert_eq!(detect_lang("auto", env(&[("LANG", "en_US.UTF-8")])), Lang::En);
    assert_eq!(detect_lang("en", env(&[("LANG", "ko_KR.UTF-8")])), Lang::En);
    assert_eq!(detect_lang("auto", env(&[("LC_ALL", "en_US.UTF-8"), ("LANG", "ko_KR.UTF-8")])), Lang::En);
    assert_eq!(detect_lang("auto", env(&[("LC_ALL", ""), ("LC_MESSAGES", "ko_KR"), ("LANG", "C")])), Lang::Ko);
    assert_eq!(detect_lang("auto", env(&[])), Lang::En);
}
