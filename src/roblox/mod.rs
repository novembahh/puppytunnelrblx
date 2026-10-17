pub mod detect;
pub mod map;
pub mod plugin;
pub mod studio;
pub mod watcher;

pub use detect::{autoconfigure, detect_roblox_user, DEFAULT_RELAY};
pub use map::{get_runtime_server_place, inject_map};
pub use plugin::{install_studio_plugin, studio_plugins_dir};
pub use studio::{get_studio_path, launch_client, launch_server, VINEGAR};
pub use watcher::{roblox_log_dir, watch_media_requests};
