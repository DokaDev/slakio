//! Display widths and wrapping, by grapheme cluster with `unicode-width` 0.2 (the function
//! Ratatui's buffer uses), so wrapped lines fill exactly the cells they are given: W/F = 2,
//! emoji presentation, ZWJ and flag sequences = 2, combining marks = 0.

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

/// `s` cut to at most `w` columns, ending with `…` when cut. Never splits a grapheme.
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
    out.push('…');
    out
}

/// Wrap `s` into lines of at most `w` columns, keeping its line breaks; words stay whole where
/// they fit, a longer word breaks between graphemes. Stops after `max_lines` lines: the second
/// value says whether text was left out.
pub fn wrap(s: &str, w: usize, max_lines: usize) -> (Vec<String>, bool) {
    let w = w.max(2);
    let mut out: Vec<String> = Vec::new();
    let paras: Vec<&str> = s.split('\n').collect();
    for (k, para) in paras.iter().enumerate() {
        let mut line = String::new();
        let mut acc = 0;
        for word in para.split_word_bounds() {
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
                    out.push(std::mem::take(&mut line));
                    acc = 0;
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

#[cfg(test)]
mod tests;
