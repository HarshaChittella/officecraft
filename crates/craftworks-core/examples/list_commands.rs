//! Prints every app's commands with their shortcuts (for choosing menu items).
fn main() {
    for kind in craftworks_core::Kind::ALL {
        for s in craftworks_core::keymaps::app_shortcuts(kind) {
            println!("{kind:?}\t{}\t{}", s.command, s.chord);
        }
    }
}
