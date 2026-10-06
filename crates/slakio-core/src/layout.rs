//! The layout of the work area as a tree, free of any UI crate: leaves are panes (by
//! [`PaneId`]), a split puts its two children side by side (or one above the other) and says
//! how much room the second one gets. [`Node::solve`] turns the tree into one rectangle per pane
//! shown; the UI draws them and the mouse finds panes in them.
//!
//! Which pane is what (a conversation, the thread panel a pane opened beside it) is not the
//! tree's business: the tree only places panes.

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

#[cfg(test)]
mod tests;
