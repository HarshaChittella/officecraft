//! The window's tabs: which engine document each one shows, their order, the active one, and
//! tab groups (one per open project, as Chrome groups tabs). A group's tabs always sit next to
//! each other; a collapsed group's tabs are hidden behind its label.

use crate::{DocId, Kind};

/// Identifies a tab for as long as the window lives; never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabId(pub u64);

/// One tab: a document in one engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub kind: Kind,
    pub doc: DocId,
    pub group: Option<GroupId>,
}

/// Identifies a tab group; never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(pub u64);

/// A tab group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    /// What the group stands for, for its owner (a project's folder).
    pub key: String,
    pub name: String,
    /// An index into the strip's group colours.
    pub color: u8,
    pub collapsed: bool,
}

/// The tab strip's model.
#[derive(Debug, Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next: u64,
    groups: Vec<Group>,
    next_group: u64,
}

impl Tabs {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every tab, left to right.
    pub fn iter(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter()
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    pub fn get(&self, id: TabId) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }

    /// The tab that shows `doc` of `kind`, if any.
    pub fn find(&self, kind: Kind, doc: DocId) -> Option<TabId> {
        self.tabs.iter().find(|t| t.kind == kind && t.doc == doc).map(|t| t.id)
    }

    pub fn active(&self) -> Option<&Tab> {
        self.active.and_then(|id| self.get(id))
    }

    /// Adds a tab for `doc` right after the active one (or at the end) and makes it active. A
    /// document that already has a tab gets that tab activated instead of a second one.
    pub fn open(&mut self, kind: Kind, doc: DocId) -> TabId {
        self.open_in(kind, doc, None)
    }

    /// [`Tabs::open`] into `group`: at the end of the group's tabs. Without a group, right of
    /// the active tab, or after its group when it has one (a group's tabs stay together).
    pub fn open_in(&mut self, kind: Kind, doc: DocId, group: Option<GroupId>) -> TabId {
        if let Some(id) = self.find(kind, doc) {
            self.activate(id);
            return id;
        }
        let group = group.filter(|g| self.group(*g).is_some());
        self.next = self.next.saturating_add(1);
        let id = TabId(self.next);
        let at = match group {
            Some(g) => self.run_end(g).unwrap_or(self.tabs.len()),
            None => match self.active() {
                Some(Tab { group: Some(g), .. }) => self.run_end(*g).unwrap_or(self.tabs.len()),
                _ => self.active_index().map_or(self.tabs.len(), |i| i.saturating_add(1)),
            },
        };
        self.tabs.insert(at.min(self.tabs.len()), Tab { id, kind, doc, group });
        if let Some(g) = group.and_then(|g| self.group_mut(g)) {
            g.collapsed = false;
        }
        self.active = Some(id);
        id
    }

    /// Makes `id` the active tab, expanding its group if it was collapsed. False when there is
    /// no such tab.
    pub fn activate(&mut self, id: TabId) -> bool {
        let Some(group) = self.get(id).map(|t| t.group) else { return false };
        if let Some(g) = group.and_then(|g| self.group_mut(g)) {
            g.collapsed = false;
        }
        self.active = Some(id);
        true
    }

    /// Whether tab `id` is shown (its group, if any, isn't collapsed).
    pub fn is_visible(&self, id: TabId) -> bool {
        self.get(id).is_some_and(|t| t.group.and_then(|g| self.group(g)).is_none_or(|g| !g.collapsed))
    }

    // ── Groups ──

    /// Every group, in the order they were made.
    pub fn groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter()
    }

    pub fn group(&self, id: GroupId) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: GroupId) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub fn group_by_key(&self, key: &str) -> Option<GroupId> {
        self.groups.iter().find(|g| g.key == key).map(|g| g.id)
    }

    /// Adds a group (empty until tabs join it), or returns the one with this key.
    pub fn add_group(&mut self, key: &str, name: &str, color: u8) -> GroupId {
        if let Some(id) = self.group_by_key(key) {
            return id;
        }
        self.next_group = self.next_group.saturating_add(1);
        let id = GroupId(self.next_group);
        self.groups.push(Group { id, key: key.to_string(), name: name.to_string(), color, collapsed: false });
        id
    }

    /// Removes a group; its tabs stay, ungrouped.
    pub fn remove_group(&mut self, id: GroupId) {
        self.groups.retain(|g| g.id != id);
        for t in self.tabs.iter_mut().filter(|t| t.group == Some(id)) {
            t.group = None;
        }
    }

    /// The tabs in group `id`, left to right.
    pub fn in_group(&self, id: GroupId) -> Vec<TabId> {
        self.tabs.iter().filter(|t| t.group == Some(id)).map(|t| t.id).collect()
    }

    /// Puts tab `id` in `group` (at the end of its tabs; an empty group's first tab stays where
    /// it is), or takes it out of its group (to just after the group's tabs).
    pub fn set_group(&mut self, id: TabId, group: Option<GroupId>) -> bool {
        let Some(i) = self.tabs.iter().position(|t| t.id == id) else { return false };
        let group = group.filter(|g| self.group(*g).is_some());
        let old = self.tabs[i].group;
        if old == group {
            return true;
        }
        let mut tab = self.tabs.remove(i);
        let at = match (group, old) {
            // An empty group starts where its first tab already is.
            (Some(g), _) => self.run_end(g).unwrap_or(i),
            (None, Some(o)) => self.run_end(o).unwrap_or(i),
            (None, None) => i,
        };
        tab.group = group;
        self.tabs.insert(at.min(self.tabs.len()), tab);
        true
    }

    /// Collapses or expands a group. Collapsing the active tab's group moves to the nearest
    /// tab outside it (right first); with no such tab it stays expanded (false).
    pub fn set_collapsed(&mut self, id: GroupId, collapsed: bool) -> bool {
        if self.group(id).is_none() {
            return false;
        }
        if collapsed && self.active().is_some_and(|t| t.group == Some(id)) {
            let i = self.active_index().unwrap_or(0);
            let outside = |t: &&Tab| t.group != Some(id) && t.group.and_then(|g| self.group(g)).is_none_or(|g| !g.collapsed);
            let next = self.tabs[i..].iter().find(outside).or_else(|| self.tabs[..i].iter().rev().find(outside)).map(|t| t.id);
            match next {
                Some(n) => self.active = Some(n),
                None => return false,
            }
        }
        if let Some(g) = self.group_mut(id) {
            g.collapsed = collapsed;
        }
        true
    }

    /// Index just past the last tab of group `g`, if it has tabs.
    fn run_end(&self, g: GroupId) -> Option<usize> {
        self.tabs.iter().rposition(|t| t.group == Some(g)).map(|i| i + 1)
    }

    /// Index of the first tab of group `g`, if it has tabs.
    fn run_start(&self, g: GroupId) -> Option<usize> {
        self.tabs.iter().position(|t| t.group == Some(g))
    }

    /// Removes tab `id`. When it was active, the tab to its right becomes active, else the one
    /// to its left (as browsers do). Returns the removed tab.
    pub fn remove(&mut self, id: TabId) -> Option<Tab> {
        let i = self.tabs.iter().position(|t| t.id == id)?;
        let tab = self.tabs.remove(i);
        if self.active == Some(id) {
            self.active = self.tabs.get(i).or_else(|| i.checked_sub(1).and_then(|j| self.tabs.get(j))).map(|t| t.id);
        }
        Some(tab)
    }

    /// Moves tab `id` to position `to` (clamped to the strip), keeping groups together: a
    /// grouped tab stays among its group's tabs, and an ungrouped one never lands inside a
    /// group (it goes to that group's nearer edge). False when there is no such tab.
    pub fn move_to(&mut self, id: TabId, to: usize) -> bool {
        let Some(i) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let tab = self.tabs.remove(i);
        let mut to = to.min(self.tabs.len());
        match tab.group {
            Some(g) => {
                if let (Some(a), Some(b)) = (self.run_start(g), self.run_end(g)) {
                    to = to.clamp(a, b);
                } else {
                    to = i.min(self.tabs.len());
                }
            }
            None => {
                // Between two tabs of the same group: out to the nearer edge of that group.
                let left = to.checked_sub(1).and_then(|k| self.tabs.get(k)).and_then(|t| t.group);
                let right = self.tabs.get(to).and_then(|t| t.group);
                if let (Some(l), Some(r)) = (left, right)
                    && l == r
                    && let (Some(a), Some(b)) = (self.run_start(l), self.run_end(l))
                {
                    // Equally near: the side it came from.
                    to = match (to - a).cmp(&(b - to)) {
                        std::cmp::Ordering::Less => a,
                        std::cmp::Ordering::Greater => b,
                        std::cmp::Ordering::Equal => {
                            if i >= b {
                                b
                            } else {
                                a
                            }
                        }
                    };
                }
            }
        }
        self.tabs.insert(to, tab);
        true
    }

    /// The tab `steps` to the right of the active one, wrapping around (negative: to the left);
    /// ⌃Tab and ⌃⇧Tab.
    /// Collapsed groups' tabs are skipped.
    pub fn cycle(&mut self, steps: isize) -> Option<TabId> {
        let shown: Vec<TabId> = self.tabs.iter().filter(|t| self.is_visible(t.id)).map(|t| t.id).collect();
        let n = isize::try_from(shown.len()).ok().filter(|n| *n > 0)?;
        let i = isize::try_from(self.active.and_then(|a| shown.iter().position(|t| *t == a)).unwrap_or(0)).ok()?;
        let j = usize::try_from((i + steps % n + n) % n).ok()?;
        let id = *shown.get(j)?;
        self.active = Some(id);
        Some(id)
    }

    fn active_index(&self) -> Option<usize> {
        let id = self.active?;
        self.tabs.iter().position(|t| t.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(t: &Tabs) -> Vec<u64> {
        t.iter().map(|t| t.id.0).collect()
    }

    #[test]
    fn a_new_tab_opens_right_of_the_active_one_and_becomes_active() {
        let mut t = Tabs::new();
        let a = t.open(Kind::Document, DocId(1));
        let b = t.open(Kind::Sheet, DocId(1));
        t.activate(a);
        let c = t.open(Kind::Pdf, DocId(9));
        assert_eq!(ids(&t), [a.0, c.0, b.0]);
        assert_eq!(t.active().map(|t| t.id), Some(c));
    }

    #[test]
    fn a_document_with_a_tab_gets_that_tab_not_another() {
        let mut t = Tabs::new();
        let a = t.open(Kind::Document, DocId(1));
        t.open(Kind::Document, DocId(2));
        assert_eq!(t.open(Kind::Document, DocId(1)), a);
        assert_eq!(t.len(), 2);
        assert_eq!(t.active().map(|t| t.id), Some(a));
        // The same id in another engine is another document.
        t.open(Kind::Sheet, DocId(1));
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn closing_the_active_tab_activates_its_right_neighbour_then_its_left() {
        let mut t = Tabs::new();
        let a = t.open(Kind::Document, DocId(1));
        let b = t.open(Kind::Document, DocId(2));
        let c = t.open(Kind::Document, DocId(3));
        t.activate(b);
        t.remove(b);
        assert_eq!(t.active().map(|t| t.id), Some(c));
        t.remove(c);
        assert_eq!(t.active().map(|t| t.id), Some(a));
        t.remove(a);
        assert!(t.active().is_none() && t.is_empty());
        assert!(t.remove(a).is_none(), "removing twice is harmless");
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let mut t = Tabs::new();
        let a = t.open(Kind::Document, DocId(1));
        let b = t.open(Kind::Document, DocId(2));
        t.remove(a);
        assert_eq!(t.active().map(|t| t.id), Some(b));
    }

    #[test]
    fn reordering_clamps_and_keeps_the_active_tab() {
        let mut t = Tabs::new();
        let a = t.open(Kind::Document, DocId(1));
        let b = t.open(Kind::Document, DocId(2));
        let c = t.open(Kind::Document, DocId(3));
        assert!(t.move_to(c, 0));
        assert_eq!(ids(&t), [c.0, a.0, b.0]);
        assert!(t.move_to(c, 99));
        assert_eq!(ids(&t), [a.0, b.0, c.0]);
        assert!(!t.move_to(TabId(42), 0));
        assert_eq!(t.active().map(|t| t.id), Some(c));
    }

    #[test]
    fn cycling_wraps_both_ways() {
        let mut t = Tabs::new();
        assert_eq!(t.cycle(1), None, "nothing to cycle through");
        let a = t.open(Kind::Document, DocId(1));
        let b = t.open(Kind::Document, DocId(2));
        let c = t.open(Kind::Document, DocId(3));
        assert_eq!(t.cycle(1), Some(a));
        assert_eq!(t.cycle(-1), Some(c));
        assert_eq!(t.cycle(-1), Some(b));
        assert_eq!(t.cycle(7), Some(c), "7 steps on 3 tabs is 1, from b");
        // isize::MIN % 3 is -2: two to the left of c is a. Huge steps don't overflow.
        assert_eq!(t.cycle(isize::MIN), Some(a));
        assert_eq!(t.cycle(isize::MAX), Some(b), "isize::MAX % 3 is 1");
    }

    #[test]
    fn grouped_tabs_stay_together() {
        let mut t = Tabs::new();
        let g = t.add_group("/p", "Plans", 1);
        assert_eq!(t.add_group("/p", "Other name", 2), g, "one group per key");
        let a = t.open(Kind::Document, DocId(1));
        let b = t.open_in(Kind::Sheet, DocId(1), Some(g));
        let c = t.open_in(Kind::Pdf, DocId(1), Some(g));
        // Opened from inside the group, an ungrouped file goes after the group.
        let d = t.open(Kind::Document, DocId(2));
        assert_eq!(ids(&t), [a.0, b.0, c.0, d.0]);
        // A grouped tab can't be dragged out of its group; an ungrouped one can't land inside.
        assert!(t.move_to(c, 0));
        assert_eq!(ids(&t), [a.0, c.0, b.0, d.0]);
        assert!(t.move_to(d, 2));
        assert_eq!(ids(&t), [a.0, c.0, b.0, d.0], "the nearer edge is after the group");
        assert!(t.move_to(a, 2));
        assert_eq!(ids(&t), [c.0, b.0, a.0, d.0]);
        assert!(t.set_group(a, Some(g)));
        assert_eq!(t.in_group(g), vec![c, b, a]);
        assert!(t.set_group(c, None));
        assert_eq!(ids(&t), [b.0, a.0, c.0, d.0], "taken out, it goes just after the group");
        t.remove_group(g);
        assert!(t.iter().all(|x| x.group.is_none()));
    }

    #[test]
    fn collapsed_groups_hide_their_tabs() {
        let mut t = Tabs::new();
        let g = t.add_group("/p", "P", 0);
        let a = t.open(Kind::Document, DocId(1));
        let b = t.open_in(Kind::Sheet, DocId(1), Some(g));
        let c = t.open_in(Kind::Sheet, DocId(2), Some(g));
        assert!(t.set_collapsed(g, true));
        assert_eq!(t.active().map(|x| x.id), Some(a), "collapsing the active tab's group moves off it");
        assert!(!t.is_visible(b) && !t.is_visible(c) && t.is_visible(a));
        assert_eq!(t.cycle(1), Some(a), "hidden tabs are skipped");
        assert!(t.activate(c));
        assert!(t.group(g).is_some_and(|x| !x.collapsed), "showing a tab expands its group");
        t.remove(a);
        assert!(!t.set_collapsed(g, true), "nothing else to show: it stays open");
    }

    #[test]
    fn ids_are_never_reused() {
        let mut t = Tabs::new();
        let a = t.open(Kind::Document, DocId(1));
        t.remove(a);
        let b = t.open(Kind::Document, DocId(1));
        assert_ne!(a, b);
    }
}
