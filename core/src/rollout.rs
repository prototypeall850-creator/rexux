use crate::config::Config;
pub use rexux_rollout::ARCHIVED_SESSIONS_SUBDIR;
pub use rexux_rollout::Cursor;
pub use rexux_rollout::INTERACTIVE_SESSION_SOURCES;
pub use rexux_rollout::RolloutRecorder;
pub use rexux_rollout::RolloutRecorderParams;
pub use rexux_rollout::SESSIONS_SUBDIR;
pub use rexux_rollout::SessionMeta;
pub use rexux_rollout::SortDirection;
pub use rexux_rollout::ThreadItem;
pub use rexux_rollout::ThreadSortKey;
pub use rexux_rollout::ThreadsPage;
pub use rexux_rollout::append_thread_name;
pub use rexux_rollout::find_archived_thread_path_by_id_str;
#[deprecated(note = "use find_thread_path_by_id_str")]
pub use rexux_rollout::find_conversation_path_by_id_str;
pub use rexux_rollout::find_thread_meta_by_name_str;
pub use rexux_rollout::find_thread_name_by_id;
pub use rexux_rollout::find_thread_names_by_ids;
pub use rexux_rollout::find_thread_path_by_id_str;
pub use rexux_rollout::parse_cursor;
pub use rexux_rollout::read_head_for_summary;
pub use rexux_rollout::read_session_meta_line;
pub use rexux_rollout::rollout_date_parts;

impl rexux_rollout::RolloutConfigView for Config {
    fn rexux_home(&self) -> &std::path::Path {
        self.rexux_home.as_path()
    }

    fn sqlite_config(&self) -> &rexux_state::SqliteConfig {
        self.sqlite_config()
    }

    fn cwd(&self) -> &std::path::Path {
        self.cwd.as_path()
    }

    fn model_provider_id(&self) -> &str {
        self.model_provider_id.as_str()
    }

    fn generate_memories(&self) -> bool {
        self.memories.generate_memories
    }
}

pub(crate) mod list {
    pub use rexux_rollout::find_thread_path_by_id_str;
}

#[cfg(test)]
pub(crate) mod recorder {
    pub use rexux_rollout::RolloutRecorder;
}

pub(crate) use crate::session_rollout_init_error::map_session_init_error;

pub(crate) mod truncation {
    pub(crate) use crate::thread_rollout_truncation::*;
}
