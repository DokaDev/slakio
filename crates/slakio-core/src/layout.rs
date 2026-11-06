//! The layout of the work area as a tree, free of any UI crate: leaves are panes (by
//! [`PaneId`]), a split puts its two children side by side (or one above the other) and says
//! how much room the second one gets. [`Node::solve`] turns the tree into one rectangle per pane
//! shown; the UI draws them and the mouse finds panes in them.
//!
//! Which pane is what (a conversation, the thread panel a pane opened beside it) is not the
//! tree's business: the tree only places panes. A tab is a tree with a name ([`tabs`]).

/// A pane, for as long as it is open (ids are never reused).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(pub u64);

/// A rectangle of terminal cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// How a split places its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir {
    /// Side by side, the first on the left.
    Row,
    /// One above the other, the first on top.
    Column,
}

/// The room the second child of a split gets: `num/den` of the split's length, between `min`
/// and `max` cells, less where the first would keep under `keep`. Where both do not fit, the
/// child with the focused pane takes the whole split (the first when neither has it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Share {
    pub num: u16,
    pub den: u16,
    pub min: u16,
    pub max: u16,
    pub keep: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Leaf(PaneId),
    Split { dir: Dir, share: Share, first: Box<Node>, second: Box<Node> },
}

impl Node {
    /// `first` and `second` split along `dir`, the second sized by `share`.
    pub fn split(dir: Dir, share: Share, first: Node, second: Node) -> Self {
        Self::Split { dir, share, first: Box::new(first), second: Box::new(second) }
    }

    /// The panes, in reading order (left to right, top to bottom).
    pub fn leaves(&self) -> Vec<PaneId> {
        match self {
            Self::Leaf(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut out = first.leaves();
                out.extend(second.leaves());
                out
            }
        }
    }

    pub fn contains(&self, id: PaneId) -> bool {
        match self {
            Self::Leaf(l) => *l == id,
            Self::Split { first, second, .. } => first.contains(id) || second.contains(id),
        }
    }

    /// The tree without pane `id`: a split left with one child becomes that child; `None` when
    /// nothing is left.
    pub fn without(&self, id: PaneId) -> Option<Node> {
        match self {
            Self::Leaf(l) if *l == id => None,
            Self::Leaf(_) => Some(self.clone()),
            Self::Split { dir, share, first, second } => match (first.without(id), second.without(id)) {
                (Some(a), Some(b)) => Some(Self::split(*dir, *share, a, b)),
                (a, b) => a.or(b),
            },
        }
    }

    /// The same tree with each pane `f` names instead (a tab reopened gets new panes).
    pub fn map(&self, f: &impl Fn(PaneId) -> PaneId) -> Node {
        match self {
            Self::Leaf(id) => Self::Leaf(f(*id)),
            Self::Split { dir, share, first, second } => Self::split(*dir, *share, first.map(f), second.map(f)),
        }
    }

    /// The tree with the leaf of pane `id` replaced by `with` (unchanged when `id` is not in it).
    pub fn replace(&self, id: PaneId, with: &Node) -> Node {
        match self {
            Self::Leaf(l) if *l == id => with.clone(),
            Self::Leaf(_) => self.clone(),
            Self::Split { dir, share, first, second } => {
                Self::split(*dir, *share, first.replace(id, with), second.replace(id, with))
            }
        }
    }

    /// The smallest part of the tree that holds every pane of `ids` (none: `None`).
    pub fn around(&self, ids: &[PaneId]) -> Option<&Node> {
        if ids.is_empty() || !ids.iter().all(|id| self.contains(*id)) {
            return None;
        }
        match self {
            Self::Split { first, second, .. } => first.around(ids).or_else(|| second.around(ids)).or(Some(self)),
            Self::Leaf(_) => Some(self),
        }
    }

    /// The tree with its part `old` (as [`Node::around`] found it) replaced by `with`.
    pub fn replace_part(&self, old: &Node, with: &Node) -> Node {
        match self {
            _ if self == old => with.clone(),
            Self::Leaf(_) => self.clone(),
            Self::Split { dir, share, first, second } => {
                Self::split(*dir, *share, first.replace_part(old, with), second.replace_part(old, with))
            }
        }
    }

    /// The tree with each split sharing its length evenly: a run of splits the same way gives
    /// each of its parts the same room. A split `keep` gives a share for keeps that share, and
    /// counts as one part. `share(dir, first, second)` is the share of a split whose two sides
    /// hold `first` and `second` such parts.
    pub fn equalized(&self, keep: &impl Fn(&Share) -> Option<Share>, share: &impl Fn(Dir, u16, u16) -> Share) -> Node {
        match self {
            Self::Leaf(_) => self.clone(),
            Self::Split { dir, share: s, first, second } => {
                let kept = |s: &Share| keep(s).is_some();
                let s = keep(s).unwrap_or_else(|| share(*dir, first.parts(*dir, &kept), second.parts(*dir, &kept)));
                Self::split(*dir, s, first.equalized(keep, share), second.equalized(keep, share))
            }
        }
    }

    /// The parts this side of a split along `dir` holds: a split the same way (that `keep`
    /// does not hold) counts the parts of both its sides; anything else is one.
    pub fn parts(&self, dir: Dir, keep: &impl Fn(&Share) -> bool) -> u16 {
        match self {
            Self::Split { dir: d, share, first, second } if *d == dir && !keep(share) => {
                first.parts(dir, keep).saturating_add(second.parts(dir, keep))
            }
            _ => 1,
        }
    }

    /// The least room along `dir` this tree takes with every pane shown: `least(dir)` a pane,
    /// both sides of a split that way, the larger side of a split the other way. A split `kept`
    /// says (a pane and its thread panel, which gives way) takes what one pane does.
    pub fn needs(&self, dir: Dir, kept: &impl Fn(&Share) -> bool, least: &impl Fn(Dir) -> u16) -> u16 {
        match self {
            Self::Split { dir: d, share, first, second } if !kept(share) => {
                let (a, b) = (first.needs(dir, kept, least), second.needs(dir, kept, least));
                if *d == dir { a.saturating_add(b) } else { a.max(b) }
            }
            _ => least(dir),
        }
    }

    /// The tree with each split's `min` and `keep` the room its sides need ([`Node::needs`]):
    /// a smaller area takes room from every pane alike until one would go under its least, and
    /// only then does a side give way. The shares (`num/den`) and the splits `kept` says stay.
    pub fn fitted(&self, kept: &impl Fn(&Share) -> bool, least: &impl Fn(Dir) -> u16) -> Node {
        match self {
            Self::Leaf(_) => self.clone(),
            Self::Split { dir, share, first, second } => {
                let share = match kept(share) {
                    true => *share,
                    false => {
                        Share { min: second.needs(*dir, kept, least), keep: first.needs(*dir, kept, least), ..*share }
                    }
                };
                Self::split(*dir, share, first.fitted(kept, least), second.fitted(kept, least))
            }
        }
    }

    /// The other side of the split one of whose sides is `part` (as [`Node::around`] found it):
    /// what takes its room when it goes. `None` when `part` is the whole tree, or not in it.
    pub fn sibling(&self, part: &Node) -> Option<&Node> {
        match self {
            Self::Leaf(_) => None,
            Self::Split { first, second, .. } if **first == *part => Some(second),
            Self::Split { first, second, .. } if **second == *part => Some(first),
            Self::Split { first, second, .. } => first.sibling(part).or_else(|| second.sibling(part)),
        }
    }

    /// Where each pane shown is, in `area`; `focused` decides which one stays where two do not
    /// fit.
    pub fn solve(&self, area: Area, focused: Option<PaneId>) -> Vec<(PaneId, Area)> {
        let mut out = Vec::new();
        self.solve_into(area, focused, &mut out);
        out
    }

    fn solve_into(&self, area: Area, focused: Option<PaneId>, out: &mut Vec<(PaneId, Area)>) {
        match self {
            Self::Leaf(id) => out.push((*id, area)),
            Self::Split { dir, share, first, second } => match cut(*dir, share, area) {
                Some((a, b)) => {
                    first.solve_into(a, focused, out);
                    second.solve_into(b, focused, out);
                }
                None => {
                    let alone = if focused.is_some_and(|f| second.contains(f)) { second } else { first };
                    alone.solve_into(area, focused, out);
                }
            },
        }
    }

    /// The splits laid out in `area` with both their sides shown, as [`Node::solve`] lays them
    /// out: where each is in the tree (`false` its first side, `true` its second), its area
    /// and the length its second side gets.
    fn laid_splits(&self, area: Area, focused: Option<PaneId>) -> Vec<(Vec<bool>, Area, u16)> {
        let mut out = Vec::new();
        self.splits_into(area, focused, &mut Vec::new(), &mut out);
        out
    }

    fn splits_into(
        &self,
        area: Area,
        focused: Option<PaneId>,
        path: &mut Vec<bool>,
        out: &mut Vec<(Vec<bool>, Area, u16)>,
    ) {
        let Self::Split { dir, share, first, second } = self else { return };
        let parts = match cut(*dir, share, area) {
            Some((a, b)) => {
                out.push((path.clone(), area, along(*dir, b)));
                vec![(false, first, a), (true, second, b)]
            }
            None if focused.is_some_and(|f| second.contains(f)) => vec![(true, second, area)],
            None => vec![(false, first, area)],
        };
        for (side, node, a) in parts {
            path.push(side);
            node.splits_into(a, focused, path, out);
            path.pop();
        }
    }

    /// The part of the tree at `path` (as [`Node::laid_splits`] gives it).
    fn at(&self, path: &[bool]) -> Option<&Node> {
        match (self, path.split_first()) {
            (_, None) => Some(self),
            (Self::Split { first, .. }, Some((false, rest))) => first.at(rest),
            (Self::Split { second, .. }, Some((true, rest))) => second.at(rest),
            (Self::Leaf(_), Some(_)) => None,
        }
    }

    /// The tree with the split at `path` giving its second side `len` of `total` cells, kept
    /// within its `min`, `max` and `keep`; `None` when that changes nothing.
    fn with_len(&self, path: &[bool], total: u16, len: i32) -> Option<Node> {
        let Self::Split { dir, share, first, second } = self else { return None };
        let Some((side, rest)) = path.split_first() else {
            let top = i32::from(share.max.max(share.min)).min(i32::from(total) - i32::from(share.keep));
            let len = len.min(top).max(i32::from(share.min));
            let old = cut(*dir, share, Area { width: total, height: total, ..Area::default() });
            if total == 0 || len < 0 || old.is_some_and(|(_, b)| i32::from(along(*dir, b)) == len) {
                return None;
            }
            let share = Share { num: len as u16, den: total, ..*share };
            return Some(Self::Split { dir: *dir, share, first: first.clone(), second: second.clone() });
        };
        Some(match side {
            false => Self::split(*dir, *share, first.with_len(rest, total, len)?, (**second).clone()),
            true => Self::split(*dir, *share, (**first).clone(), second.with_len(rest, total, len)?),
        })
    }

    /// The seams of the splits laid out in `area` (both sides shown): what the mouse drags.
    pub fn seams(&self, area: Area, focused: Option<PaneId>) -> Vec<Seam> {
        self.laid_splits(area, focused)
            .into_iter()
            .filter_map(|(path, a, len)| {
                let Some(Self::Split { dir, second, .. }) = self.at(&path) else { return None };
                let id = second.leaves()[0];
                Some(match dir {
                    Dir::Row => Seam {
                        dir: *dir,
                        at: (a.x + a.width - len).saturating_sub(1),
                        from: a.y,
                        to: a.y + a.height,
                        id,
                    },
                    Dir::Column => Seam {
                        dir: *dir,
                        at: (a.y + a.height - len).saturating_sub(1),
                        from: a.x,
                        to: a.x + a.width,
                        id,
                    },
                })
            })
            .collect()
    }

    /// The tree with seam `id` ([`Seam::id`]) moved to column (row) `at`, as far as the split's
    /// least sizes let it; `None` when it does not move (or is not laid out in `area`).
    pub fn drag(&self, area: Area, focused: Option<PaneId>, id: PaneId, at: u16) -> Option<Node> {
        let (path, a, _) = self
            .laid_splits(area, focused)
            .into_iter()
            .find(|(path, ..)| matches!(self.at(path), Some(Self::Split { second, .. }) if second.leaves()[0] == id))?;
        let Some(Self::Split { dir, .. }) = self.at(&path) else { return None };
        let (start, total) = match dir {
            Dir::Row => (a.x, a.width),
            Dir::Column => (a.y, a.height),
        };
        let len = i32::from(start) + i32::from(total) - i32::from(at) - 1;
        self.with_len(&path, total, len)
    }

    /// The tree with pane `pane` given `by` cells more along `dir` (fewer, below zero), laid out
    /// in `area`: of the splits that way around it with both sides shown, the nearest that can
    /// give or take that room does, as far as its least sizes let it. `None` when none can.
    pub fn resize(&self, area: Area, focused: Option<PaneId>, pane: PaneId, dir: Dir, by: i32) -> Option<Node> {
        let mut splits = self.laid_splits(area, focused);
        splits.reverse();
        splits.into_iter().find_map(|(path, a, len)| {
            let Some(Self::Split { dir: d, first, .. }) = self.at(&path) else { return None };
            if *d != dir || !self.at(&path)?.contains(pane) {
                return None;
            }
            let by = if first.contains(pane) { -by } else { by };
            self.with_len(&path, along(dir, a), i32::from(len) + by)
        })
    }
}

/// The length of `a` along `dir`.
fn along(dir: Dir, a: Area) -> u16 {
    match dir {
        Dir::Row => a.width,
        Dir::Column => a.height,
    }
}

/// The areas of the two sides of a split along `dir` sized by `share` in `area`; `None` when
/// both do not fit.
fn cut(dir: Dir, share: &Share, area: Area) -> Option<(Area, Area)> {
    let total = along(dir, area);
    let wanted = u32::from(total) * u32::from(share.num) / u32::from(share.den.max(1));
    let (min, keep) = (u32::from(share.min), u32::from(share.keep));
    if u32::from(total) < min + keep {
        return None;
    }
    // Where the share would leave the first side less than it keeps, the second gives way.
    let len = wanted.min(u32::from(total) - keep).clamp(min, u32::from(share.max).max(min));
    let len = len as u16;
    Some(match dir {
        Dir::Row => (Area { width: total - len, ..area }, Area { x: area.x + total - len, width: len, ..area }),
        Dir::Column => (Area { height: total - len, ..area }, Area { y: area.y + total - len, height: len, ..area }),
    })
}

/// The line between the two sides of a split laid out: the last column (row) of its first
/// side, across the split from `from` to `to` (not included). The first pane of its second
/// side names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seam {
    pub dir: Dir,
    pub at: u16,
    pub from: u16,
    pub to: u16,
    pub id: PaneId,
}

/// A way to go from a pane to the one beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
}

/// The pane next to `from` going `toward`, among the panes laid out at `areas` ([`Node::solve`]):
/// of those whose side touches or faces that side of `from` and that share some of its rows
/// (columns), the nearest; then the one `recent` ranks highest (the one the keyboard was on
/// last, so going there and back comes back); then the one sharing the most; then the first
/// (top, left). `None` when there is none that way, or `from` is not laid out.
pub fn toward(
    areas: &[(PaneId, Area)],
    from: PaneId,
    toward: Toward,
    recent: impl Fn(PaneId) -> u64,
) -> Option<PaneId> {
    let (_, f) = areas.iter().find(|(id, _)| *id == from)?;
    let span = |a: &Area, rows: bool| {
        if rows { (a.y, a.y + a.height) } else { (a.x, a.x + a.width) }
    };
    let overlap = |a: (u16, u16), b: (u16, u16)| a.1.min(b.1).saturating_sub(a.0.max(b.0));
    areas
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, a)| {
            let gap = match toward {
                Toward::Left => f.x.checked_sub(a.x + a.width)?,
                Toward::Right => a.x.checked_sub(f.x + f.width)?,
                Toward::Up => f.y.checked_sub(a.y + a.height)?,
                Toward::Down => a.y.checked_sub(f.y + f.height)?,
            };
            let rows = matches!(toward, Toward::Left | Toward::Right);
            let shared = overlap(span(a, rows), span(f, rows));
            (shared > 0).then_some((
                gap,
                std::cmp::Reverse(recent(*id)),
                std::cmp::Reverse(shared),
                span(a, rows).0,
                *id,
            ))
        })
        .min()
        .map(|(.., id)| id)
}

pub mod saved;
pub mod tabs;

#[cfg(test)]
mod tests;
