//! Prints how each app's menu-ready commands spread over ribbon tabs (for designing menus).
use craftworks_core::{Kind, catalog};
fn main() {
    for kind in [Kind::Document, Kind::Sheet, Kind::Presentation] {
        let mut tabs: Vec<(String, usize, usize)> = Vec::new();
        for e in catalog::entries(kind).into_iter().filter(|e| !e.needs_args) {
            let t = e.place.first().cloned().unwrap_or_else(|| "(none)".into());
            match tabs.iter_mut().find(|x| x.0 == t) {
                Some(x) => {
                    x.1 += 1;
                    x.2 += usize::from(e.shortcut.is_some());
                }
                None => tabs.push((t, 1, usize::from(e.shortcut.is_some()))),
            }
        }
        println!("{kind:?}: {}", tabs.iter().map(|(t, n, s)| format!("{t}={n}/{s}")).collect::<Vec<_>>().join(", "));
    }
}
