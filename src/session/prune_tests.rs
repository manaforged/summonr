use std::time::Duration;

use super::{STAMP_FILE, due};
use crate::durable::log;

#[test]
fn pruning_is_due_again_exactly_one_period_after_the_last_run() {
    let home = tempfile::tempdir().unwrap();
    let sessions = log::sessions_dir(home.path());
    std::fs::create_dir_all(&sessions).unwrap();
    let period = Duration::from_secs(3600);
    assert!(due(home.path(), period, std::time::SystemTime::now()));
    let stamped = std::fs::metadata(sessions.join(STAMP_FILE))
        .unwrap()
        .modified()
        .unwrap();
    assert!(!due(
        home.path(),
        period,
        stamped + period - Duration::from_secs(1)
    ));
    assert!(due(home.path(), period, stamped + period));
}
