use super::Sidecar;

const FILE: &str = "sidecar.json";

#[test]
fn a_forgotten_entry_is_gone_after_save_even_when_still_live() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(crate::durable::log::sessions_dir(home.path())).unwrap();
    let mut cache = Sidecar::<u64>::load(home.path(), FILE);
    cache.put("a", 7);
    cache.save(["a"]);
    let mut cache = Sidecar::<u64>::load(home.path(), FILE);
    assert_eq!(cache.get("a"), Some(&7));
    cache.forget("a");
    cache.save(["a"]);
    assert_eq!(Sidecar::<u64>::load(home.path(), FILE).get("a"), None);
}
