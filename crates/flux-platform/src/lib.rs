//! flux-platform: platform adapters. All OS-conditional code lives here.

pub mod clipboard;
pub mod icons;
pub mod open;
pub mod sysdirs;
pub mod trash;
pub mod volumes;
pub mod watch;

pub use icons::{FileIcon, file_icon, type_label};
pub use open::{open_in_terminal, open_path, open_paths, reveal_path};
pub use sysdirs::{FreeSpace, free_space, home_dir, known_dirs};
pub use trash::{TrashError, trash_items};
pub use volumes::{Volume, mounted_volumes};
pub use watch::{FsWatcher, WatchEvent};
