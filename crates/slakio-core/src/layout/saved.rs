//! The tabs as they are saved between runs: their order and names, which one is shown, each
//! tab's layout with its shares, its active pane and zoom, and what each pane shows (a
//! conversation or a thread, and the thread panel a pane opened). Nothing a pane holds besides
//! (its selection, its history, what is being written) is saved.
//!
//! ```toml
//! version = 1
//! current = 0
//!
//! [[tab]]
//! name = "Incident"          # left out: named after its active pane
//! active = 1                 # the pane the keyboard goes to (an index into its panes)
//! zoomed = false
//! tree = ["row 1 2 24 65535 24", "pane 0", "pane 1"]
//!
//! [[tab.pane]]
//! workspace = "T01"
//! conversation = "C01"
//! panel = 1                  # the thread panel this pane opened (an index), if any
//!
//! [[tab.pane]]
//! workspace = "T01"
//! conversation = "C01"
//! thread = 1700000000000100  # a thread: its first message's timestamp
//! ```
//!
//! The tree is written first-node-first: a split is `row` or `column` with its share's five
//! numbers (`num den min max keep`, [`Share`]), followed by its two sides; a pane is `pane` and
//! its index. A file is either read whole or not at all ([`Saved::parse`] says why): this
//! version's number, ids of the shape Slack gives, a tree whose panes are its tab's panes each
//! once, and limits on how much there is. A thread panel that is not beside its pane in the tree
//! is read as an ordinary pane.

use super::{Dir, Node, PaneId, Share};
use crate::model::{ConversationId, Target, Ts, WorkspaceId};
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

/// The version of the file this code writes and reads.
pub const VERSION: i64 = 1;
/// The most tabs a file may hold.
pub const MAX_TABS: usize = 64;
/// The most panes a tab may hold.
pub const MAX_PANES: usize = 64;
/// The longest a tab's name is kept (in characters).
pub const MAX_NAME: usize = 100;
/// The longest an id may be.
const MAX_ID: usize = 64;

/// The tabs, in their order, and the one shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Saved {
    pub tabs: Vec<SavedTab>,
    /// The tab shown (an index into `tabs`).
    pub current: usize,
}

/// A tab: its layout, by the panes' indices ([`PaneId`] `i` is `panes[i]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedTab {
    pub name: Option<String>,
    pub root: Node,
    pub active: PaneId,
    pub zoomed: bool,
    pub panes: Vec<SavedPane>,
}

/// A pane: what it shows, and the thread panel it opened (a pane of the same tab).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedPane {
    pub target: Target,
    pub panel: Option<PaneId>,
}

impl Saved {
    /// The file's text.
    pub fn to_text(&self) -> String {
        let mut doc = DocumentMut::new();
        doc["version"] = value(VERSION);
        doc["current"] = value(self.current as i64);
        let mut tabs = ArrayOfTables::new();
        for tab in &self.tabs {
            let mut t = Table::new();
            if let Some(name) = &tab.name {
                t["name"] = value(name.as_str());
            }
            t["active"] = value(tab.active.0 as i64);
            t["zoomed"] = value(tab.zoomed);
            let mut tree = toml_edit::Array::new();
            encode(&tab.root, &mut tree);
            t["tree"] = value(tree);
            let mut panes = ArrayOfTables::new();
            for p in &tab.panes {
                let mut pt = Table::new();
                pt["workspace"] = value(p.target.workspace().as_str());
                pt["conversation"] = value(p.target.conversation().as_str());
                if let Target::Thread { thread, .. } = &p.target {
                    pt["thread"] = value(thread.0 as i64);
                }
                if let Some(panel) = p.panel {
                    pt["panel"] = value(panel.0 as i64);
                }
                panes.push(pt);
            }
            t["pane"] = Item::ArrayOfTables(panes);
            tabs.push(t);
        }
        doc["tab"] = Item::ArrayOfTables(tabs);
        doc.to_string()
    }

    /// The tabs of a file's text; why not when it cannot be read whole.
    pub fn parse(text: &str) -> Result<Saved, String> {
        // The parser's words quote the file: only their start is kept.
        let doc = text.parse::<DocumentMut>().map_err(|e| format!("not TOML: {}", quoted(&e.to_string())))?;
        if doc.get("version").and_then(Item::as_integer) != Some(VERSION) {
            return Err("not version 1".to_string());
        }
        let tabs = match doc.get("tab") {
            None => Vec::new(),
            Some(item) => {
                let tabs = item.as_array_of_tables().ok_or("tab is not a list of tables")?;
                if tabs.len() > MAX_TABS {
                    return Err(format!("more than {MAX_TABS} tabs"));
                }
                tabs.iter().map(tab).collect::<Result<Vec<_>, _>>()?
            }
        };
        let current = index(doc.get("current"), "current")?.unwrap_or(0);
        let current = if current < tabs.len() { current } else { 0 };
        Ok(Saved { tabs, current })
    }
}

/// The tree first-node-first into `out`.
fn encode(node: &Node, out: &mut toml_edit::Array) {
    match node {
        Node::Leaf(id) => out.push(format!("pane {}", id.0)),
        Node::Split { dir, share: s, first, second } => {
            let d = if *dir == Dir::Row { "row" } else { "column" };
            out.push(format!("{d} {} {} {} {} {}", s.num, s.den, s.min, s.max, s.keep));
            encode(first, out);
            encode(second, out);
        }
    }
}

/// The tree whose first node is `words`' next, of panes below `panes`.
fn decode<'a>(words: &mut impl Iterator<Item = &'a str>, panes: usize, depth: usize) -> Result<Node, String> {
    if depth > MAX_PANES {
        return Err("a tree too deep".to_string());
    }
    let word = words.next().ok_or("a tree cut short")?;
    let mut parts = word.split(' ');
    let kind = parts.next().unwrap_or_default();
    let numbers = parts.map(str::parse::<u16>).collect::<Result<Vec<_>, _>>().map_err(|_| quoted(word))?;
    match (kind, numbers.as_slice()) {
        ("pane", &[i]) if usize::from(i) < panes => Ok(Node::Leaf(PaneId(u64::from(i)))),
        ("row" | "column", &[num, den, min, max, keep]) if den > 0 => {
            let dir = if kind == "row" { Dir::Row } else { Dir::Column };
            let first = decode(words, panes, depth + 1)?;
            let second = decode(words, panes, depth + 1)?;
            Ok(Node::split(dir, Share { num, den, min, max, keep }, first, second))
        }
        _ => Err(quoted(word)),
    }
}

/// A tab's name as it is kept (when given, and when read back): the user's own words without
/// control characters, at most [`MAX_NAME`] characters, without blanks at its ends; `None` when
/// nothing is left.
pub fn tab_name(name: &str) -> Option<String> {
    let name: String = name.chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    Some(name.trim().to_string()).filter(|n| !n.is_empty())
}

/// `word` of a file, quoted in why it cannot be read (a line of the error log): at most a few
/// dozen characters of it, control characters as blanks.
fn quoted(word: &str) -> String {
    const MOST: usize = 40;
    let cut: String = word.chars().take(MOST).map(|c| if c.is_control() { ' ' } else { c }).collect();
    let more = if word.chars().nth(MOST).is_some() { "…" } else { "" };
    format!("`{cut}{more}`")
}

/// A tab of the file.
fn tab(t: &Table) -> Result<SavedTab, String> {
    let panes = match t.get("pane") {
        None => Vec::new(),
        Some(item) => {
            let panes = item.as_array_of_tables().ok_or("pane is not a list of tables")?;
            panes.iter().map(pane).collect::<Result<Vec<_>, _>>()?
        }
    };
    if panes.is_empty() || panes.len() > MAX_PANES {
        return Err(format!("a tab of {} panes", panes.len()));
    }
    let tree = t.get("tree").and_then(Item::as_array).ok_or("a tab without a tree")?;
    let mut words = tree.iter().map(|v| v.as_str().unwrap_or_default());
    let root = decode(&mut words, panes.len(), 0)?;
    if words.next().is_some() {
        return Err("more after the tree".to_string());
    }
    let mut leaves: Vec<u64> = root.leaves().into_iter().map(|p| p.0).collect();
    leaves.sort_unstable();
    if leaves != (0..panes.len() as u64).collect::<Vec<_>>() {
        return Err("a tree that is not its panes each once".to_string());
    }
    let active = index(t.get("active"), "active")?.filter(|a| *a < panes.len()).unwrap_or(0);
    let zoomed = t.get("zoomed").and_then(Item::as_bool).unwrap_or(false) && matches!(root, Node::Split { .. });
    let name = match t.get("name") {
        None => None,
        Some(n) => Some(n.as_str().ok_or("a name that is not text")?),
    };
    let name = name.and_then(tab_name);
    let mut tab = SavedTab { name, root, active: PaneId(active as u64), zoomed, panes };
    keep_panels(&mut tab);
    Ok(tab)
}

/// A thread panel is kept as one only where it is beside the pane that opened it (the two sides
/// of one split, the panel second) and opened by that pane alone.
fn keep_panels(tab: &mut SavedTab) {
    for i in 0..tab.panes.len() {
        let Some(panel) = tab.panes[i].panel else { continue };
        let owner = PaneId(i as u64);
        let beside = tab.root.around(&[owner, panel]).is_some_and(|n| {
            matches!(n, Node::Split { first, second, .. }
                if **first == Node::Leaf(owner) && **second == Node::Leaf(panel))
        });
        let once = tab.panes.iter().filter(|p| p.panel == Some(panel)).count() == 1;
        if !beside || !once || panel == owner {
            tab.panes[i].panel = None;
        }
    }
}

/// A pane of the file.
fn pane(t: &Table) -> Result<SavedPane, String> {
    let id = |key: &str| {
        t.get(key)
            .and_then(Item::as_str)
            .filter(|s| !s.is_empty() && s.len() <= MAX_ID)
            .filter(|s| s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
            .map(str::to_string)
            .ok_or_else(|| format!("a pane without a {key}"))
    };
    let (workspace, conversation) = (WorkspaceId::new(id("workspace")?), ConversationId::new(id("conversation")?));
    let target = match t.get("thread") {
        None => Target::Conversation { workspace, conversation },
        Some(ts) => {
            let ts = ts.as_integer().and_then(|n| u64::try_from(n).ok()).ok_or("a thread that is no timestamp")?;
            Target::Thread { workspace, conversation, thread: Ts(ts) }
        }
    };
    let panel = index(t.get("panel"), "panel")?.map(|i| PaneId(i as u64));
    Ok(SavedPane { target, panel })
}

/// A whole number at least zero, when there is one.
fn index(item: Option<&Item>, key: &str) -> Result<Option<usize>, String> {
    match item {
        None => Ok(None),
        Some(i) => {
            let n = i.as_integer().and_then(|n| usize::try_from(n).ok()).ok_or_else(|| format!("{key} is no index"))?;
            Ok(Some(n))
        }
    }
}

#[cfg(test)]
mod tests;
