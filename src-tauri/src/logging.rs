//! Rolling file logging into `<store>\logs\`.
//!
//! OWNER: worker W4.

use std::path::Path;

/// Installs the tracing subscriber. Returns the appender guard, which must stay
/// alive for the process lifetime or writes are dropped.
pub fn init(_log_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    todo!("W4")
}
