//! Where a view the wheel moved away from the selection goes: by rows, not by messages, from the
//! message on its last row and the rows of it cut off below. It stops with the oldest message's
//! first row on the first row, and at the newest message's last row on the last one: there it
//! follows the newest again.

/// The view whose last row shows message `bottom` of `n`, `cut` rows of it cut off below, moved
/// `by` rows (down when positive) in an area `h` rows high; `height` measures a message. The
/// new bottom and cut, or `None` at the newest (or when every message fits).
pub fn place(
    (bottom, cut): (usize, usize),
    by: isize,
    n: usize,
    h: usize,
    height: &mut impl FnMut(usize) -> usize,
) -> Option<(usize, usize)> {
    let (mut b, mut c) = (bottom.min(n.saturating_sub(1)), cut);
    if by > 0 {
        (b, c) = down((b, c), by.unsigned_abs(), n, height);
    } else if by < 0 {
        c += by.unsigned_abs();
        while b > 0 && c >= height(b) {
            c -= height(b);
            b -= 1;
        }
        c = c.min(height(b).saturating_sub(1));
    }
    // Rows from the oldest message's first one to the last row: fewer than the area holds,
    // and the view moves down until it is full from the top.
    let mut rows = height(b).saturating_sub(c);
    let mut i = b;
    while rows < h && i > 0 {
        i -= 1;
        rows += height(i);
    }
    if rows < h {
        (b, c) = down((b, c), h - rows, n, height);
    }
    (b + 1 < n || c > 0).then_some((b, c))
}

/// `rows` rows down from the view at `(bottom, cut)`, no further than the newest message's last row.
fn down(
    (mut b, mut c): (usize, usize),
    mut rows: usize,
    n: usize,
    height: &mut impl FnMut(usize) -> usize,
) -> (usize, usize) {
    loop {
        if c >= rows {
            return (b, c - rows);
        }
        rows -= c;
        if b + 1 >= n {
            return (b, 0);
        }
        b += 1;
        c = height(b);
    }
}

#[cfg(test)]
mod tests {
    use super::place;

    /// Ten messages of three rows each, in a view of seven rows.
    fn at(from: (usize, usize), by: isize) -> Option<(usize, usize)> {
        place(from, by, 10, 7, &mut |_| 3)
    }

    #[test]
    fn the_wheel_moves_by_rows_through_the_messages() {
        assert_eq!(at((9, 0), -3), Some((8, 0)));
        assert_eq!(at((9, 0), -4), Some((8, 1)));
        assert_eq!(at((8, 1), 1), Some((8, 0)));
        assert_eq!(at((8, 0), 2), Some((9, 1)));
    }

    #[test]
    fn it_stops_at_the_first_row_and_follows_the_newest_again_at_the_last() {
        // Thirty rows; the oldest one's first row on top leaves message 2, its last two rows
        // cut off.
        assert_eq!(at((9, 0), -100), Some((2, 2)));
        assert_eq!(at((2, 2), -3), Some((2, 2)));
        assert_eq!(at((2, 2), 100), None);
        assert_eq!(at((8, 2), 3), Some((9, 2)));
        assert_eq!(at((8, 2), 5), None);
    }

    #[test]
    fn a_history_that_fits_does_not_move() {
        assert_eq!(place((1, 0), -3, 2, 7, &mut |_| 3), None);
    }
}
