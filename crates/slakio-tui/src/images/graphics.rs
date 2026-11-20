//! The kitty graphics protocol, as far as slakio uses it: a picture is sent to the terminal once
//! (PNG, base64, in pieces) as a *virtual placement* of so many cells ([`transmit`]), and shown
//! wherever the screen holds its *placeholder* cells ([`cell`]): the character U+10EEEE with
//! two marks that say the cell's row and column in the picture, drawn in a foreground colour
//! that says which picture ([`color`]). The pictures are thus ordinary cells of the screen
//! buffer: whatever is drawn over them (a popup, another pane, a scroll) replaces them, and
//! nothing of a picture is left where its cells are gone. Every command asks the terminal not to
//! answer (`q=2`): an answer would come on standard input, behind the keys' reader.

use crate::terminal::base64;
use ratatui::style::Color;

/// The character a placeholder cell holds.
const PLACEHOLDER: char = '\u{10EEEE}';

/// The marks that number a placeholder's row and column (the protocol's table, in its order;
/// the first [`MARKS`] of it).
const DIACRITICS: [char; 40] = [
    '\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}', '\u{033F}', '\u{0346}',
    '\u{034A}', '\u{034B}', '\u{034C}', '\u{0350}', '\u{0351}', '\u{0352}', '\u{0357}', '\u{035B}', '\u{0363}',
    '\u{0364}', '\u{0365}', '\u{0366}', '\u{0367}', '\u{0368}', '\u{0369}', '\u{036A}', '\u{036B}', '\u{036C}',
    '\u{036D}', '\u{036E}', '\u{036F}', '\u{0483}', '\u{0484}', '\u{0485}', '\u{0486}', '\u{0487}', '\u{0592}',
    '\u{0593}', '\u{0594}', '\u{0595}', '\u{0597}',
];

/// The most rows and columns a picture may take (the marks there are).
pub const MARKS: u16 = DIACRITICS.len() as u16;

/// The largest piece of a picture's base64 sent in one command (the protocol's limit).
const PIECE: usize = 4096;

/// The highest picture id: it is said in a 24-bit colour.
pub const ID_MAX: u32 = 0x00FF_FFFF;

/// The foreground colour of picture `id`'s placeholders.
pub fn color(id: u32) -> Color {
    Color::Rgb((id >> 16) as u8, (id >> 8) as u8, id as u8)
}

/// `symbol` (a cell's) is a picture's placeholder.
pub fn is_placeholder(symbol: &str) -> bool {
    symbol.starts_with(PLACEHOLDER)
}

/// The placeholder of row `row`, column `col` of a picture (both under [`MARKS`]).
pub fn cell(row: u16, col: u16) -> String {
    let mark = |i: u16| DIACRITICS[usize::from(i.min(MARKS - 1))];
    [PLACEHOLDER, mark(row), mark(col)].iter().collect()
}

/// Row `row` of a picture `cols` wide, as placeholders.
pub fn row(row: u16, cols: u16) -> String {
    (0..cols).map(|c| cell(row, c)).collect()
}

/// The commands that send `png` as picture `id`, placed virtually over `cols` × `rows` cells.
pub fn transmit(id: u32, cols: u16, rows: u16, png: &[u8]) -> String {
    let data = base64(png);
    let pieces: Vec<&str> = data.as_bytes().chunks(PIECE).map(|p| std::str::from_utf8(p).unwrap_or_default()).collect();
    let mut out = String::with_capacity(data.len() + pieces.len() * 24 + 64);
    for (i, piece) in pieces.iter().enumerate() {
        let more = u8::from(i + 1 < pieces.len());
        if i == 0 {
            out.push_str(&format!("\x1b_Ga=T,U=1,f=100,t=d,i={id},c={cols},r={rows},q=2,m={more};{piece}\x1b\\"));
        } else {
            out.push_str(&format!("\x1b_Gq=2,m={more};{piece}\x1b\\"));
        }
    }
    out
}

/// The command that removes picture `id` and frees its data.
pub fn delete(id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

/// The command that removes the pictures with ids `from` to `to` and frees their data.
pub fn delete_range(from: u32, to: u32) -> String {
    format!("\x1b_Ga=d,d=R,x={from},y={to},q=2\x1b\\")
}

/// The command that removes every picture and frees their data.
pub fn delete_all() -> &'static str {
    "\x1b_Ga=d,d=A,q=2\x1b\\"
}

#[cfg(test)]
mod tests;
