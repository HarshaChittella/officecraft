//! The blank document an app keeps for itself: GridCraft and WordCraft open an untitled one at
//! start, and make a new one when their last document closes (they never have none). In the
//! app's own window that's the empty window you start from; in Craftworks there's no tab for it,
//! so it's set aside: never listed as a document, reused by the next New, closed when a file
//! opens. Without this, closing your new sheet left the app's own "Book1" in its place, as a
//! tab you hadn't made.

/// One document as this helper sees it: its id, and whether it's untouched (no file, nothing
/// changed).
pub(crate) type Seen = (u64, bool);

#[derive(Debug, Default)]
pub(crate) struct Blanks {
    /// The set-aside blank documents (`None` until the app's start-up ones were noted).
    ids: Option<Vec<u64>>,
    /// The last close left the app with nothing: the blank it makes next is set aside too.
    expect: bool,
}

impl Blanks {
    /// Notes the untouched documents the app opened by itself, the first time it's asked.
    pub(crate) fn note_startup(&mut self, docs: &[Seen]) {
        if self.ids.is_none() {
            self.ids = Some(docs.iter().filter(|d| d.1).map(|d| d.0).collect());
        }
    }

    /// The set-aside documents that are still there and still untouched.
    pub(crate) fn pristine(&self, docs: &[Seen]) -> Vec<u64> {
        self.ids.iter().flatten().copied().filter(|id| docs.iter().any(|d| d.0 == *id && d.1)).collect()
    }

    /// A set-aside blank to use as a new document, if there is one (it's yours from now on).
    pub(crate) fn take(&mut self, docs: &[Seen]) -> Option<u64> {
        let id = self.pristine(docs).first().copied()?;
        if let Some(list) = self.ids.as_mut() {
            list.retain(|x| *x != id);
        }
        Some(id)
    }

    /// After a file opened: the blanks are closed by the caller; forget them.
    pub(crate) fn clear(&mut self) {
        self.ids = Some(Vec::new());
    }

    /// After a close: untouched documents that weren't there before stand in for "nothing
    /// open"; and with nothing left at all, the next blank is too.
    pub(crate) fn after_close(&mut self, before: &[u64], docs: &[Seen]) {
        let fresh = docs.iter().filter(|d| d.1 && !before.contains(&d.0)).map(|d| d.0);
        self.ids.get_or_insert_with(Vec::new).extend(fresh);
        self.expect = docs.is_empty();
    }

    /// Each frame: sets aside the blank the app made after its last document closed.
    pub(crate) fn adopt(&mut self, docs: &[Seen]) {
        if !self.expect || docs.is_empty() {
            return;
        }
        self.expect = false;
        self.ids.get_or_insert_with(Vec::new).extend(docs.iter().filter(|d| d.1).map(|d| d.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blanks_are_set_aside_reused_and_replaced() {
        let mut b = Blanks::default();
        b.note_startup(&[(1, true)]);
        b.note_startup(&[(1, true), (9, true)]);
        assert_eq!(b.pristine(&[(1, true)]), vec![1], "only what was there at start");
        assert_eq!(b.pristine(&[(1, false)]), Vec::<u64>::new(), "an edited one is yours");
        assert_eq!(b.take(&[(1, true)]), Some(1));
        assert_eq!(b.take(&[(1, true)]), None, "taken once");
        // Closing the last one: the app is empty, and the blank it makes next is set aside.
        b.after_close(&[1], &[]);
        b.adopt(&[]);
        b.adopt(&[(2, true)]);
        assert_eq!(b.pristine(&[(2, true)]), vec![2]);
        b.adopt(&[(2, true), (3, true)]);
        assert_eq!(b.pristine(&[(2, true), (3, true)]), vec![2], "only the one made right after");
        // A close that leaves documents: a new untouched one standing in is set aside too.
        b.after_close(&[2, 4], &[(4, false), (5, true)]);
        assert_eq!(b.pristine(&[(4, false), (5, true)]), vec![5]);
    }
}
