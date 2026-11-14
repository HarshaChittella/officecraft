//! What the host does with the window commands the apps send (docs/decisions.md D18).
//!
//! The window belongs to the shell, so commands that move, size or title it are dropped on
//! purpose; close commands feed the tab's close state machine; anything else is logged at
//! `warn` so missing support shows up (D13).

use craftworks_core::Kind;
use egui::{ViewportCommand, ViewportId};

/// The close commands among a frame's commands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Handled {
    pub close: bool,
    pub cancel_close: bool,
}

/// How the host treats one command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Treatment {
    /// Feeds the close state machine.
    Close,
    CancelClose,
    /// Dropped on purpose: the window is the shell's (D18).
    Dropped,
    /// Not handled yet: logged at `warn`.
    Unhandled,
}

/// How the host treats `cmd`.
pub fn treatment(cmd: &ViewportCommand) -> Treatment {
    use ViewportCommand as C;
    match cmd {
        C::Close => Treatment::Close,
        C::CancelClose => Treatment::CancelClose,
        C::StartDrag
        | C::BeginResize(_)
        | C::Maximized(_)
        | C::Minimized(_)
        | C::Fullscreen(_)
        | C::OuterPosition(_)
        | C::InnerSize(_)
        | C::MinInnerSize(_)
        | C::MaxInnerSize(_)
        | C::Decorations(_)
        | C::Title(_)
        | C::Icon(_)
        | C::Focus
        | C::RequestUserAttention(_)
        | C::Visible(_)
        | C::Resizable(_)
        | C::Transparent(_)
        // The window's appearance is the shell's; Phase 8 passes the system theme to each app.
        | C::SetTheme(_) => Treatment::Dropped,
        _ => Treatment::Unhandled,
    }
}

/// Sorts `commands` from `kind`'s viewport `id`, logs them, and returns the close commands.
pub fn apply(kind: Kind, id: ViewportId, commands: Vec<ViewportCommand>) -> Handled {
    let mut handled = Handled::default();
    for cmd in commands {
        match treatment(&cmd) {
            Treatment::Close => handled.close = true,
            Treatment::CancelClose => handled.cancel_close = true,
            Treatment::Dropped => log::debug!("{kind:?} viewport {id:?}: dropped {cmd:?} (the window is the shell's)"),
            Treatment::Unhandled => log::warn!("{kind:?} viewport {id:?}: unhandled {cmd:?}"),
        }
    }
    handled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_commands_are_dropped_and_close_commands_kept() {
        assert_eq!(treatment(&ViewportCommand::StartDrag), Treatment::Dropped);
        assert_eq!(treatment(&ViewportCommand::Maximized(true)), Treatment::Dropped);
        assert_eq!(treatment(&ViewportCommand::Title("x".into())), Treatment::Dropped);
        assert_eq!(treatment(&ViewportCommand::Close), Treatment::Close);
        assert_eq!(treatment(&ViewportCommand::CancelClose), Treatment::CancelClose);
        let h = apply(Kind::Document, ViewportId::ROOT, vec![ViewportCommand::StartDrag, ViewportCommand::CancelClose]);
        assert_eq!(h, Handled { close: false, cancel_close: true });
    }

    #[test]
    fn anything_else_is_reported_as_unhandled() {
        assert_eq!(treatment(&ViewportCommand::IMEAllowed(true)), Treatment::Unhandled);
    }
}
