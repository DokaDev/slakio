//! Display widths and wrapping, by grapheme cluster with `unicode-width` 0.2 (the function
//! Ratatui's buffer uses), so wrapped lines fill exactly the cells they are given: W/F = 2,
//! emoji presentation, ZWJ and flag sequences = 2, combining marks = 0.

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Display width of a grapheme cluster.
pub fn grapheme_width(g: &str) -> usize {
    UnicodeWidthStr::width(g)
}

/// Display width of a string.
pub fn width(s: &str) -> usize {
    s.graphemes(true).map(grapheme_width).sum()
}

/// `s` cut to at most `w` columns, ending with `…` when cut (never after a blank). Never splits
/// a grapheme.
pub fn clip(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    if w == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut acc = 0;
    for g in s.graphemes(true) {
        let gw = grapheme_width(g);
        if acc + gw + 1 > w {
            break;
        }
        out.push_str(g);
        acc += gw;
    }
    let mut out = out.trim_end().to_string();
    out.push('…');
    out
}

/// Wrap `s` into lines of at most `w` columns, keeping its line breaks. Lines break at spaces:
/// a word keeps the punctuation written against it (`it.`), so a period never starts a line; a
/// word wider than a line breaks between graphemes, taking the grapheme before a closing mark
/// along with it. Stops after `max_lines` lines: the second value says whether text was left out.
pub fn wrap(s: &str, w: usize, max_lines: usize) -> (Vec<String>, bool) {
    let w = w.max(2);
    let mut out: Vec<String> = Vec::new();
    let paras: Vec<&str> = s.split('\n').collect();
    for (k, para) in paras.iter().enumerate() {
        let mut line = String::new();
        let mut acc = 0;
        for word in pieces(para, w) {
            let ww = width(word);
            if acc + ww <= w {
                line.push_str(word);
                acc += ww;
                continue;
            }
            // The word does not fit: start a new line with it (a space is dropped there), and
            // break it where it is wider than a line.
            if acc > 0 {
                out.push(std::mem::take(&mut line));
                acc = 0;
                if out.len() >= max_lines {
                    return (out, true);
                }
                if word.trim().is_empty() {
                    continue;
                }
            }
            for g in word.graphemes(true) {
                let gw = grapheme_width(g);
                if acc + gw > w && acc > 0 {
                    // A closing mark takes the grapheme before it (and any closing marks
                    // before that) to the next line.
                    let mut carried = String::new();
                    if closing(g) {
                        while let Some((i, last)) = line.grapheme_indices(true).next_back() {
                            if i == 0 {
                                break;
                            }
                            carried.insert_str(0, last);
                            line.truncate(i);
                            if !closing(&carried) {
                                break;
                            }
                        }
                    }
                    out.push(std::mem::take(&mut line));
                    acc = width(&carried);
                    line = carried;
                    if out.len() >= max_lines {
                        return (out, true);
                    }
                }
                line.push_str(g);
                acc += gw;
            }
        }
        out.push(line);
        if out.len() >= max_lines {
            return (out, k + 1 < paras.len());
        }
    }
    (out, false)
}

/// [`wrap`] for text in styled runs: the same lines, each as spans in the styles of the runs
/// its text came from. A grapheme takes the style of the run its first character is in (a
/// combining mark in a run of its own joins the grapheme before it); `fallback` where a line's
/// grapheme is not the text's next one. Linear in the text: each line is found where the one
/// before ended, only blanks and breaks left out between them.
pub fn wrap_styled(
    runs: &[(String, Style)],
    w: usize,
    max_lines: usize,
    fallback: Style,
) -> (Vec<Vec<Span<'static>>>, bool) {
    let plain: String = runs.iter().map(|(s, _)| s.as_str()).collect();
    let (lines, more) = wrap(&plain, w, max_lines);
    // Where each run starts in `plain`, and its style.
    let mut starts: Vec<(usize, Style)> = Vec::with_capacity(runs.len());
    let mut at = 0;
    for (s, st) in runs {
        if !s.is_empty() {
            starts.push((at, *st));
        }
        at += s.len();
    }
    let style_at = |byte: usize| match starts.partition_point(|(b, _)| *b <= byte) {
        0 => fallback,
        n => starts[n - 1].1,
    };
    let mut cursor = 0;
    let out = lines
        .iter()
        .map(|line| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            for g in line.graphemes(true) {
                // Pass over the blanks and breaks the wrap left out.
                while !plain[cursor..].starts_with(g)
                    && let Some(c) = plain[cursor..].chars().next().filter(|c| c.is_whitespace())
                {
                    cursor += c.len_utf8();
                }
                let style = if plain[cursor..].starts_with(g) {
                    let style = style_at(cursor);
                    cursor += g.len();
                    style
                } else {
                    fallback
                };
                match spans.last_mut() {
                    Some(last) if last.style == style => last.content.to_mut().push_str(g),
                    _ => spans.push(Span::styled(g.to_string(), style)),
                }
            }
            spans
        })
        .collect();
    (out, more)
}

/// `s` as runs of non-blanks and runs of blanks (a word keeps the punctuation written against
/// it). An arrow and the word after one blank are one word (`→ host`): an arrow never ends a
/// line apart from what it points to.
fn words(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        let first = rest.chars().next()?;
        let blank = first.is_whitespace();
        let run_end = |r: &str, blank: bool| r.find(|c: char| c.is_whitespace() != blank).unwrap_or(r.len());
        let mut end = run_end(rest, blank);
        if &rest[..end] == "→" && rest[end..].starts_with(' ') && !rest[end + 1..].starts_with(char::is_whitespace) {
            end += 1 + run_end(&rest[end + 1..], false);
        }
        let (run, tail) = rest.split_at(end);
        rest = tail;
        Some(run)
    })
}

/// The pieces lines are made of: words (with their punctuation) and blanks; a word wider than a
/// line comes in the parts a word boundary gives (`docs.example.com`, `/`, `runbooks`), a closing
/// mark kept on the part before it.
fn pieces(s: &str, w: usize) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for word in words(s) {
        if word.starts_with(char::is_whitespace) || width(word) <= w {
            out.push(word);
            continue;
        }
        let start = out.len();
        let mut at = 0;
        for part in word.split_word_bounds() {
            let end = at + part.len();
            if closing(part) && out.len() > start {
                let last = out.pop().expect("a part before");
                let from = at - last.len();
                out.push(&word[from..end]);
            } else {
                out.push(&word[at..end]);
            }
            at = end;
        }
    }
    out
}

/// Punctuation that closes what is before it and must not start a line.
fn closing(g: &str) -> bool {
    g.chars().next().is_some_and(|c| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '…' | '。' | '、' | '」' | '』' | '）' | '！' | '？'
        )
    })
}

#[cfg(test)]
mod tests;
