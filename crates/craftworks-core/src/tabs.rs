//! The window's tabs: which engine document each one shows, their order, and the active one.
//! Pure data, shared by the shell (through the FFI) and the test harness.

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
}

/// The tab strip's model.
#[derive(Debug, Default)]
pub struct Tabs {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next: u64,
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
        if let Some(id) = self.find(kind, doc) {
            self.active = Some(id);
            return id;
        }
        self.next = self.next.saturating_add(1);
        let id = TabId(self.next);
        let at = self.active_index().map_or(self.tabs.len(), |i| i.saturating_add(1));
        self.tabs.insert(at.min(self.tabs.len()), Tab { id, kind, doc });
        self.active = Some(id);
        id
    }

    /// Makes `id` the active tab. False when there is no such tab.
    pub fn activate(&mut self, id: TabId) -> bool {
        let found = self.get(id).is_some();
        if found {
            self.active = Some(id);
        }
        found
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

    /// Moves tab `id` to position `to` (clamped to the strip). False when there is no such tab.
    pub fn move_to(&mut self, id: TabId, to: usize) -> bool {
        let Some(i) = self.tabs.iter().position(|t| t.id == id) else {
            return false;
        };
        let tab = self.tabs.remove(i);
        let to = to.min(self.tabs.len());
        self.tabs.insert(to, tab);
        true
    }

    /// The tab `steps` to the right of the active one, wrapping around (negative: to the left);
    /// ⌃Tab and ⌃⇧Tab.
    pub fn cycle(&mut self, steps: isize) -> Option<TabId> {
        let n = isize::try_from(self.tabs.len()).ok().filter(|n| *n > 0)?;
        let i = isize::try_from(self.active_index().unwrap_or(0)).ok()?;
        let j = usize::try_from((i + steps % n + n) % n).ok()?;
        let id = self.tabs.get(j)?.id;
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
    fn ids_are_never_reused() {
        let mut t = Tabs::new();
        let a = t.open(Kind::Document, DocId(1));
        t.remove(a);
        let b = t.open(Kind::Document, DocId(1));
        assert_ne!(a, b);
    }
}
