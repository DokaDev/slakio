//! The colors of workspaces: the theme's palette, less the colors that read as its accent.

use super::Theme;
use ratatui::style::{Color, Style};
use slakio_core::model::WorkspaceColor;

/// How far (in Oklab) a workspace's color stays from the theme's accent, so its stripe never
/// reads as part of a focus frame beside it.
pub(super) const APART: f64 = 0.1;

impl Theme {
    /// The style of workspace color `c`.
    pub fn workspace(&self, c: WorkspaceColor) -> Style {
        let slot = usize::from(c.0);
        let n = self.workspace_colors().count();
        let color = self.workspace_colors().nth(slot % n.max(1));
        Style::new().fg(color.unwrap_or(self.workspaces[slot % self.workspaces.len()]))
    }

    /// The colors workspaces take, by slot (wrapping around): the palette in its order, without
    /// those near the accent, so the next one takes their place.
    pub(super) fn workspace_colors(&self) -> impl Iterator<Item = Color> + '_ {
        let apart = |c: &&Color| distance(**c, self.accent).is_none_or(|d| d >= APART);
        self.workspaces.iter().filter(apart).copied()
    }
}

/// How far apart two RGB colors look (Oklab), `None` for a terminal color.
pub(super) fn distance(a: Color, b: Color) -> Option<f64> {
    let (a, b) = (oklab(a)?, oklab(b)?);
    Some(((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt())
}

/// `c` in Oklab, `None` for a terminal color.
fn oklab(c: Color) -> Option<(f64, f64, f64)> {
    let Color::Rgb(r, g, b) = c else { return None };
    let lin = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (lin(r), lin(g), lin(b));
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    Some((
        0.210_454_255_3 * l + 0.793_617_785 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766 * s,
    ))
}
