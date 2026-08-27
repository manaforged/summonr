use super::{load, record};

fn home() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("mage-recent-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap_or_else(|error| panic!("{error}"));
    dir
}

#[test]
fn missing_file_is_empty() {
    assert!(load(&home()).is_empty());
}

#[test]
fn records_newest_first_and_dedups() {
    let home = home();
    record(&home, "glm\tglm-5.3");
    record(&home, "qwen\tqwen3.8-27b");
    record(&home, "glm\tglm-5.3");
    assert_eq!(load(&home), vec!["glm\tglm-5.3", "qwen\tqwen3.8-27b"]);
}

#[test]
fn caps_the_history() {
    let home = home();
    for i in 0..12 {
        record(&home, &format!("e\tm{i}"));
    }
    let seats = load(&home);
    assert_eq!(seats.len(), 8);
    assert_eq!(seats[0], "e\tm11");
}
