//! The tab bar, the work area's first row while there are two tabs or more: each tab
//! ` <n> <title> [●<unread>] × `, the tab shown raised and bold, `‹` / `›` where tabs are left
//! out. The geometry is [`crate::tabbar`]'s, the same the mouse reads.
//!
//! ```text
//!  1 #backend ×  2 ⤷ Deploy rollback ●3 ×  3 @Minsu Kim ×
//! ```

use crate::app::App;
use crate::tabbar::{Bar, MORE_LEFT, MORE_RIGHT, Part};
use ratatui::Frame;

pub(super) fn draw(f: &mut Frame, app: &App, bar: &Bar) {
    let t = &app.theme;
    let buf = f.buffer_mut();
    buf.set_style(bar.area, t.tab_bar());
    let tabs = app.work.tabs();
    let current = tabs.current();
    for p in &bar.pieces {
        let shown = current == Some(p.tab);
        let style = match p.part {
            Part::Number => t.tab_number(shown),
            Part::Title | Part::Blank => t.tab(shown),
            Part::Badge => {
                let mention = tabs.all().get(p.tab).is_some_and(|tab| app.tab_unread(tab).1);
                t.tab_badge(shown, mention)
            }
            Part::Close => t.tab_close(shown),
        };
        buf.set_string(p.x, bar.area.y, &p.text, style);
    }
    for (mark, at) in [(MORE_LEFT, bar.left), (MORE_RIGHT, bar.right)] {
        if let Some((x, _)) = at {
            buf.set_string(x, bar.area.y, mark, t.tab_more());
        }
    }
}
