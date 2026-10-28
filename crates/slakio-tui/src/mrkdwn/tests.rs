use super::*;
use proptest::prelude::*;
use slakio_core::sanitize::Remote;

const B: Marks = Marks { bold: true, italic: false, strike: false };
const I: Marks = Marks { bold: false, italic: true, strike: false };
const N: Marks = Marks { bold: false, italic: false, strike: false };

fn one(text: &str) -> Vec<Piece> {
    match parse(text).as_slice() {
        [Block::Line { quote: false, pieces }] => pieces.clone(),
        other => panic!("{text:?}: {other:?}"),
    }
}

fn t(s: &str, m: Marks) -> Piece {
    Piece::Text(s.to_string(), m)
}

/// Every piece's text, as the drawing code shows it with no names known.
fn flat(blocks: &[Block]) -> String {
    let lines: Vec<String> = blocks
        .iter()
        .map(|b| match b {
            Block::Line { quote, pieces } => {
                let body: String = pieces.iter().map(|p| plain(p, &|_| None)).collect();
                if *quote { format!("> {body}") } else { body }
            }
            Block::Code(c) => format!("|{c}"),
        })
        .collect();
    lines.join("\n")
}

#[test]
fn marks_open_and_close_as_slack_reads_them() {
    assert_eq!(one("a *bold* b"), [t("a ", N), t("bold", B), t(" b", N)]);
    assert_eq!(one("_it_"), [t("it", I)]);
    assert_eq!(one("~gone~."), [t("gone", Marks { strike: true, ..N }), t(".", N)]);
    assert_eq!(one("*_both_*"), [t("both", Marks { bold: true, italic: true, strike: false })]);
    // Inside a word, after a blank, with no end, or empty: text.
    assert_eq!(one("2*3*4"), [t("2*3*4", N)]);
    assert_eq!(one("a * b *"), [t("a * b *", N)]);
    assert_eq!(one("*open"), [t("*open", N)]);
    assert_eq!(one("**"), [t("**", N)]);
    assert_eq!(one("snake_case_name"), [t("snake_case_name", N)]);
    // A mark does not nest in itself.
    assert_eq!(one("*a *b* c*"), [t("a *b", B), t(" c*", N)]);
}

#[test]
fn code_is_shown_as_written() {
    assert_eq!(
        one("run `*not bold* &lt;x&gt;` now"),
        [t("run ", N), Piece::Code("*not bold* <x>".into()), t(" now", N)]
    );
    let blocks = parse("before\n```\nfn main() {\n    *x*\n}\n```\nafter");
    assert_eq!(flat(&blocks), "before\n|fn main() {\n|    *x*\n|}\nafter");
    // On one line, and a fence with no end is text.
    assert_eq!(flat(&parse("```one line```")), "|one line");
    assert_eq!(flat(&parse("``` no end")), "``` no end");
}

#[test]
fn quotes_are_lines_that_start_with_a_quote_mark() {
    assert_eq!(flat(&parse("&gt; said\nnot &gt; quoted\n>also")), "> said\nnot > quoted\n> also");
    // A face or a comparison at the start of a line is not a quote.
    assert_eq!(flat(&parse("&gt;_&lt; oops\n&gt;=5 left\n&gt;.&lt;")), ">_< oops\n>=5 left\n>.<");
    assert_eq!(flat(&parse("&gt;\n&gt; *bold*")), "> \n> bold");
}

#[test]
fn people_channels_broadcasts_groups_and_links() {
    assert_eq!(
        one("hi <@U1> in <#C2|general>"),
        [
            t("hi ", N),
            Piece::User { id: "U1".into(), label: None, marks: N },
            t(" in ", N),
            Piece::Channel { id: "C2".into(), label: Some("general".into()), marks: N },
        ]
    );
    assert_eq!(one("<!here>"), [Piece::Broadcast("here".into())]);
    assert_eq!(one("<!subteam^S1|@ops>"), [Piece::Group(Some("@ops".into()))]);
    assert_eq!(one("<!date^1392734382^{date}|Feb 18>"), [t("Feb 18", N)]);
    assert_eq!(
        one("<https://x.example/a?b=1&amp;c=2|the docs>"),
        [Piece::Link { url: "https://x.example/a?b=1&c=2".into(), label: Some("the docs".into()), marks: N }]
    );
    assert_eq!(flat(&parse("<mailto:ops-desk>")), "ops-desk", "a mail address without its scheme");
    // A bold link keeps its mark; `<>` and an unclosed `<` are text.
    assert_eq!(one("*<https://x.example>*"), [Piece::Link { url: "https://x.example".into(), label: None, marks: B }]);
    assert_eq!(one("a <> b < c"), [t("a <> b < c", N)]);
    assert_eq!(flat(&parse("1 &lt; 2 &amp;&amp; 3 &gt; 2")), "1 < 2 && 3 > 2");
}

/// A label that reads as another address shows where the link goes; a person whose id is not
/// known shows the id, not a name the sender wrote.
#[test]
fn labels_cannot_pass_for_another_address_or_person() {
    assert_eq!(flat(&parse("<https://evil.example/x|https://bank.example>")), "https://bank.example → evil.example");
    assert_eq!(flat(&parse("<https://evil.example|bank.example/login>")), "bank.example/login → evil.example");
    assert_eq!(flat(&parse("<http://evil.example|WWW.Bank.Example>")), "WWW.Bank.Example → evil.example");
    // A label of the same host, or one that is no address, is shown as written.
    assert_eq!(flat(&parse("<https://bank.example/a|https://bank.example>")), "https://bank.example");
    assert_eq!(flat(&parse("<https://Bank.example:8443/a|bank.example>")), "bank.example");
    assert_eq!(flat(&parse("<https://grafana.example|web-1>")), "web-1");
    assert_eq!(flat(&parse("<https://grafana.example|see v1.2 notes>")), "see v1.2 notes");
    assert_eq!(flat(&parse("<mailto:ops-desk|ops-desk>")), "ops-desk", "a mail link is no web address");
    // Fail closed: once the label reads as an address, the target shows unless it is provably
    // the same web host.
    for (link, shown) in [
        ("<https://bank.example@evil.test/|https://bank.example>", "https://bank.example → evil.test"),
        ("<http://203.0.113.5/login|https://bank.example>", "https://bank.example → 203.0.113.5"),
        ("<https://b\u{430}nk.example/|bank.example>", "bank.example → b\\u{430}nk.example"),
        ("<https://evil.example./|evil.example>", "evil.example → evil.example."),
        ("<https://evil.xn--p1ai/|bank.example>", "bank.example → evil.xn--p1ai"),
        ("<https://evil.example:8443/|bank.example>", "bank.example → evil.example:8443"),
        ("<slack://channel?id=C1|https://bank.example>", "https://bank.example → slack://channel"),
        ("<mailto:x@evil.test|support@bank.test>", "support@bank.test → x@evil.test"),
        ("<https://evil.example/|bank.example.>", "bank.example. → evil.example"),
        ("<https://evil.example/|https://b\u{e4}nk.example>", "https://b\u{e4}nk.example → evil.example"),
        ("<https://evil.example/|10.0.0.1>", "10.0.0.1 → evil.example"),
        // A forged annotation inside the label is no way out of it.
        (
            "<https://evil.example/|https://bank.example (bank.example)>",
            "https://bank.example (bank.example) → evil.example",
        ),
    ] {
        assert_eq!(flat(&parse(link)), shown, "{link}");
    }
    // A link that runs code goes nowhere: its label alone, and it says it goes nowhere.
    assert_eq!(flat(&parse("<javascript:alert(1)|https://bank.example>")), "https://bank.example");
    assert!(runs_code(" JavaScript:x") && runs_code("data:text/html,x") && !runs_code("https://a.example"));
    // People: by the read model's name, else by id; the sender's label never names them.
    assert_eq!(flat(&parse("<@UUNKNOWN|CEO>")), "@UUNKNOWN");
    let name = |p: &Piece| matches!(p, Piece::User { .. }).then(|| "Minsu".to_string());
    assert_eq!(first_line(&parse("<@U1|CEO>"), &name).as_deref(), Some("@Minsu"));
}

#[test]
fn emoji_shortcodes_are_drawn_outside_code() {
    assert_eq!(one(":+1: `:+1:`"), [t("👍 ", N), Piece::Code(":+1:".into())]);
}

#[test]
fn a_first_line_is_the_first_with_text() {
    let blocks = parse("\n*Deploy* <@U9> done");
    let name = |p: &Piece| matches!(p, Piece::User { .. }).then(|| "Minsu".to_string());
    assert_eq!(first_line(&blocks, &name).as_deref(), Some("Deploy @Minsu done"));
}

/// Every grapheme of `out` is one the sanitiser keeps as it is.
fn safe(out: &str) -> Result<(), String> {
    use unicode_segmentation::UnicodeSegmentation;
    match out.graphemes(true).find(|g| *g != "\n" && Remote::new(*g).block().as_str() != *g) {
        Some(g) => Err(format!("{g:?} is not safe")),
        None => Ok(()),
    }
}

/// Hostile texts, sanitised first as every message is: reading them keeps every character the
/// sanitiser let through and adds none it would not.
#[test]
fn hostile_texts_stay_sanitised() {
    let markup = ["*", "_", "<@", "<#C1|", "&gt; ", "```", "`", "<https://x.example|", ">"];
    for h in slakio_world::hostile_strings() {
        for m in markup {
            let safe = Remote::new(format!("{m}{0}{m} {0}", h.text)).block();
            let out = flat(&parse(safe.as_str()));
            assert_eq!(self::safe(&out), Ok(()), "{}", h.what);
        }
    }
}

/// A closing mark looked for past [`REACH`] bytes: the look-ahead ends inside a character of
/// several bytes (Hangul, CJK, emoji) at every offset, behind every mark and every pass-over.
#[test]
fn a_look_ahead_ending_inside_a_character_does_not_crash() {
    for mark in ["*", "_", "~"] {
        for over in ["<", "`"] {
            for wide in ["\u{AC00}", "\u{6F22}", "\u{1F600}", "\u{0301}"] {
                for pad in 0..4 {
                    let text = format!("{mark}{over}{}{}", "a".repeat(REACH - 3 + pad), wide.repeat(400));
                    let blocks = parse(&text);
                    assert!(flat(&blocks).contains(wide), "{mark}{over} +{pad} {wide:?}");
                }
            }
        }
    }
}

#[test]
fn a_long_hostile_line_is_read_quickly() {
    let line = "*a _b ~c `d <e ".repeat(3_000);
    let started = std::time::Instant::now();
    let _ = parse(&line);
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Any text: reading never panics, and what is shown has no control character, nor any the
    /// sanitiser removes.
    #[test]
    fn any_text_reads_to_safe_text(raw in "\\PC{0,200}|[*_~`<>&|@#!:a-z \\n]{0,200}") {
        let safe = Remote::new(raw).block();
        let out = flat(&parse(safe.as_str()));
        prop_assert_eq!(self::safe(&out), Ok(()));
    }

    /// Longer than the look-ahead, in characters of several bytes: reading never panics.
    #[test]
    fn long_wide_text_reads_to_safe_text(raw in "[*_~`<> a\u{AC00}\u{6F22}\u{1F600}\u{0301}]{900,1400}") {
        let safe = Remote::new(raw).block();
        let out = flat(&parse(safe.as_str()));
        prop_assert_eq!(self::safe(&out), Ok(()));
    }

    /// Text with no markup in it reads as itself.
    #[test]
    fn plain_text_reads_as_itself(s in "[a-zA-Z0-9 ,.]{0,120}") {
        prop_assert_eq!(flat(&parse(&s)), s);
    }

    /// Marks only style: the letters shown are those written, less the delimiters.
    #[test]
    fn marks_keep_every_letter(words in proptest::collection::vec(("[a-z]{1,8}", 0usize..4), 0..12)) {
        let marks = ["", "*", "_", "~"];
        let text: Vec<String> = words.iter().map(|(w, m)| format!("{0}{w}{0}", marks[*m])).collect();
        let letters = |s: &str| s.chars().filter(char::is_ascii_lowercase).collect::<String>();
        prop_assert_eq!(letters(&flat(&parse(&text.join(" ")))), letters(&text.join(" ")));
    }
}
