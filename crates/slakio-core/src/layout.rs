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
/// and `max` cells, while the first keeps at least `keep`. Where both do not fit, the child
/// with the focused pane takes the whole split (the first when neither has it).
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
    /// each of its parts the same room. A split whose share `keep` holds stays as it is, and
    /// counts as one part. `share(dir, first, second)` is the share of a split whose two sides
    /// hold `first` and `second` such parts.
    pub fn equalized(&self, keep: &impl Fn(&Share) -> bool, share: &impl Fn(Dir, u16, u16) -> Share) -> Node {
        match self {
            Self::Leaf(_) => self.clone(),
            Self::Split { dir, share: s, first, second } => {
                let s = if keep(s) { *s } else { share(*dir, first.parts(*dir, keep), second.parts(*dir, keep)) };
                Self::split(*dir, s, first.equalized(keep, share), second.equalized(keep, share))
            }
        }
    }

    /// The parts this side of a split along `dir` holds: a split the same way (that `keep`
    /// does not hold) counts the parts of both its sides; anything else is one.
    fn parts(&self, dir: Dir, keep: &impl Fn(&Share) -> bool) -> u16 {
        match self {
            Self::Split { dir: d, share, first, second } if *d == dir && !keep(share) => {
                first.parts(dir, keep).saturating_add(second.parts(dir, keep))
            }
            _ => 1,
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
            Self::Split { dir, share, first, second } => {
                let total = match dir {
                    Dir::Row => area.width,
                    Dir::Column => area.height,
                };
                let wanted = u32::from(total) * u32::from(share.num) / u32::from(share.den.max(1));
                let len = wanted.clamp(u32::from(share.min), u32::from(share.max.max(share.min)));
                if u32::from(total) < len + u32::from(share.keep) {
                    let alone = if focused.is_some_and(|f| second.contains(f)) { second } else { first };
                    return alone.solve_into(area, focused, out);
                }
                let len = len as u16;
                let (a, b) = match dir {
                    Dir::Row => {
                        (Area { width: total - len, ..area }, Area { x: area.x + total - len, width: len, ..area })
                    }
                    Dir::Column => {
                        (Area { height: total - len, ..area }, Area { y: area.y + total - len, height: len, ..area })
                    }
                };
                first.solve_into(a, focused, out);
                second.solve_into(b, focused, out);
            }
        }
    }
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
/// (columns), the nearest; then the one sharing the most; then the first (top, left). `None`
/// when there is none that way, or `from` is not laid out.
pub fn toward(areas: &[(PaneId, Area)], from: PaneId, toward: Toward) -> Option<PaneId> {
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
            (shared > 0).then_some((gap, std::cmp::Reverse(shared), span(a, rows).0, *id))
        })
        .min()
        .map(|(.., id)| id)
}

pub mod tabs;

#[cfg(test)]
mod tests;
