pub mod accessibility;
pub mod action;
pub mod capabilities;
pub mod event;
pub mod keymap;
pub mod layout;
pub mod modals;
pub mod model;
pub mod palette;
pub mod render;
pub mod screens;
pub mod terminal;
pub mod theme;
pub mod update;
pub mod widgets;

pub use action::{Action, Effect};
pub use event::run;
pub use model::{Focus, GridModel, Model};
pub use terminal::{
    CrosstermTerminal, RecordingTerminal, TerminalControl, TerminalGuard, TuiError,
    install_panic_hook,
};
pub use update::update;
