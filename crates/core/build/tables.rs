use std::path::Path;

pub(crate) fn compile() {
    let out = Path::new(&std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR")).join("tables");
    println!("cargo:rerun-if-changed=src");
    walk(Path::new("src"), Path::new("src"), &out);
}

fn walk(root: &Path, dir: &Path, out: &Path) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot list {}: {error}", dir.display()))
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, out);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "toml")
        {
            convert(root, &path, out);
        }
    }
}

fn convert(root: &Path, path: &Path, out: &Path) {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let value: serde_json::Value = toml::from_str(&text).unwrap_or_else(|error| {
        panic!("the built-in table {} is invalid: {error}", path.display())
    });
    let relative = path.strip_prefix(root).expect("the table is under src");
    let target = out.join(format!("{}.json", relative.display()));
    std::fs::create_dir_all(target.parent().expect("a table file has a folder"))
        .unwrap_or_else(|error| panic!("cannot create {}: {error}", target.display()));
    std::fs::write(&target, value.to_string())
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", target.display()));
}
