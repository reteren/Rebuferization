//! Rolling file logging into `<store>\logs\`.
//!
//! OWNER: worker W4.
//!
//! Privacy boundary: this module never receives clipboard *content* — callers
//! log sizes, kinds, and IDs only. That rule is enforced by the shape of what
//! is passed in, not by filtering here.

use std::fs;
use std::path::Path;
use std::time::Duration;

/// Installs the tracing subscriber. Returns the appender guard, which must stay
/// alive for the process lifetime or writes are dropped.
///
/// Writes one `rebuffer.log.YYYY-MM-DD` file per day (`tracing-appender`'s
/// daily rotation) and prunes everything older than the 7 most recent files.
/// Returns `None` when the log directory cannot be created; the app keeps
/// running, it just logs to nowhere.
pub fn init(log_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    if let Err(e) = fs::create_dir_all(log_dir) {
        eprintln!("rebuffer: cannot create log dir {log_dir:?}: {e}");
        return None;
    }

    prune_old_logs(log_dir);

    let appender = tracing_appender::rolling::daily(log_dir, "rebuffer.log");
    let (writer, guard) = tracing_appender::non_blocking(appender);

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_writer(writer)
        .with_ansi(false)
        .init();

    // `tracing-appender` rotates at midnight but never deletes; prune once an
    // hour from a background thread so a month-old machine stays at 7 files.
    let prune_dir = log_dir.to_path_buf();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3600));
        prune_old_logs(&prune_dir);
    });

    Some(guard)
}

/// Keeps the 7 most recent dated logs, removes our own stale `.tmp` rotation
/// leftovers, and — deliberately — leaves everything else alone.
///
/// Retention counts *dated* logs only. `tracing-appender`'s daily rotation
/// writes a single `rebuffer.log.YYYY-MM-DD` per date (a burst of rotations
/// within one day still lands in the same file), so seven kept files is
/// exactly seven days and can never silently become more.
///
/// Anything not named `rebuffer.log.<date>` or `rebuffer.log.<date>.tmp` is a
/// file we did not create in a directory the user can open; deleting it would
/// be data loss, so it is never touched.
fn prune_old_logs(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut dates: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
        let Some(stem) = name.strip_prefix("rebuffer.log.").map(str::to_owned) else { continue };
        if let Some(date) = stem.strip_suffix(".tmp") {
            // Our own leftover from an interrupted rotation — cleaning it is right.
            if is_date(date) {
                let _ = fs::remove_file(dir.join(name));
            }
        } else if is_date(&stem) {
            dates.push((name, stem));
        }
    }
    dates.sort_by(|a, b| a.1.cmp(&b.1));
    for (name, _) in dates.into_iter().rev().skip(7) {
        let _ = fs::remove_file(dir.join(name));
    }
}

/// `YYYY-MM-DD` — whatever `tracing_appender::rolling::daily` emits.
fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retention keeps exactly the 7 newest dated logs. Two rules the next
    /// person must not "fix" back:
    ///
    /// 1. `not-a-log.txt` is a file we did not create, sitting in a directory
    ///    the user can open. A logger that deletes strangers' files is a data
    ///    loss bug waiting to be reported — it must survive.
    /// 2. `rebuffer.log.<date>.tmp` is our own leftover from an interrupted
    ///    rotation — deleting it is correct.
    #[test]
    fn keeps_newest_seven_logs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut expected_kept = Vec::new();
        for day in 1..=12u32 {
            let name = format!("rebuffer.log.2026-08-{:02}", day);
            fs::write(dir.path().join(&name), "x").expect("write");
            if day > 5 {
                expected_kept.push(name);
            }
        }
        fs::write(dir.path().join("rebuffer.log.2026-08-06.tmp"), "unrelated").expect("write");
        fs::write(dir.path().join("not-a-log.txt"), "unrelated").expect("write");
        fs::write(dir.path().join("rebuffer.log.notadate.tmp"), "unrelated").expect("write");
        expected_kept.push("not-a-log.txt".into());
        expected_kept.push("rebuffer.log.notadate.tmp".into());

        prune_old_logs(dir.path());

        let mut remaining: Vec<String> = fs::read_dir(dir.path())
            .expect("read")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();
        expected_kept.sort();
        assert_eq!(
            remaining, expected_kept,
            "newest 7 dated logs and the stranger's file survive; our own .tmp leftover does not"
        );
    }

    #[test]
    fn date_shape_is_strict() {
        assert!(is_date("2026-08-30"));
        assert!(is_date("2026-01-05"));
        assert!(!is_date("2026-8-30"));
        assert!(!is_date("2026-08-3"));
        assert!(!is_date("20260830"));
        assert!(!is_date("log-2026-08-30"));
    }

    #[test]
    fn fewer_than_seven_logs_are_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        for day in 1..=3u32 {
            fs::write(dir.path().join(format!("rebuffer.log.2026-07-0{day}")), "x").expect("write");
        }
        prune_old_logs(dir.path());
        let count = fs::read_dir(dir.path()).expect("read").count();
        assert_eq!(count, 3);
    }
}