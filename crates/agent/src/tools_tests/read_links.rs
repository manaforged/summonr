use std::os::unix::fs::symlink;

use super::*;

struct Site {
    dir: tempfile::TempDir,
    other: tempfile::TempDir,
    policy: Policy,
}

impl Site {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir_in(dir.path().parent().unwrap()).unwrap();
        Self {
            dir,
            other,
            policy: Policy::builtin(),
        }
    }

    fn unrestricted() -> Self {
        let mut site = Self::new();
        site.policy.set_unrestricted();
        site
    }

    fn file(&self, name: &str, body: impl AsRef<[u8]>) {
        std::fs::write(self.dir.path().join(name), body).unwrap();
    }

    fn outside(&self, name: &str, body: impl AsRef<[u8]>) {
        std::fs::write(self.other.path().join(name), body).unwrap();
    }

    fn link(&self, target: impl AsRef<std::path::Path>, name: &str) {
        symlink(target, self.dir.path().join(name)).unwrap();
    }

    fn name(dir: &tempfile::TempDir) -> String {
        dir.path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    fn tools(&self) -> ToolRegistry {
        let mut tools =
            ToolRegistry::with_policy_and_search(self.dir.path(), self.policy.clone(), None)
                .unwrap();
        tools.set_images(true, None);
        tools
    }

    fn read(&self, args: serde_json::Value) -> std::result::Result<ToolOutput, String> {
        let call = ToolCall {
            id: "t1".to_string(),
            kind: ToolKind::Function,
            function: ToolFunction {
                checked: Default::default(),
                name: "read".to_string(),
                arguments: args.to_string(),
            },
            continuation: None,
        };
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap()
            .block_on(self.tools().execute_outcome(&call))
            .map_err(|error| error.to_string())
    }

    fn reads(&self, path: &str, expected: &str) {
        for args in every_read_of(path) {
            let output = self
                .read(args.clone())
                .unwrap_or_else(|error| panic!("{args}: {error}"));
            assert!(output.text.contains(expected), "{args}: {}", output.text);
        }
    }

    fn refuses(&self, path: &str, secret: &str) {
        for args in every_read_of(path) {
            let shown = match self.read(args.clone()) {
                Ok(output) => output.text,
                Err(error) => error,
            };
            assert!(
                shown.contains(&format!("`{path}` is a protected path")),
                "{args}: {shown}"
            );
            assert!(!shown.contains(secret), "{args}: {shown}");
        }
    }
}

fn every_read_of(path: &str) -> [serde_json::Value; 3] {
    [
        serde_json::json!({ "path": path }),
        serde_json::json!({ "op": "file", "path": path }),
        serde_json::json!({ "paths": [path] }),
    ]
}

fn png() -> Vec<u8> {
    let mut bytes = Vec::new();
    ::image::RgbImage::new(2, 2)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            ::image::ImageFormat::Png,
        )
        .unwrap();
    bytes
}

#[test]
fn a_symlink_inside_the_workspace_reads_its_target() {
    let site = Site::new();
    site.file("real.txt", "the target body\n");
    std::fs::create_dir(site.dir.path().join("src")).unwrap();
    site.file("src/deep.txt", "deep body\n");
    site.link(site.dir.path().join("real.txt"), "alias.txt");
    site.link("src", "linked");
    site.reads("alias.txt", "the target body");
    site.reads("linked/deep.txt", "deep body");
}

#[test]
fn a_symlink_to_an_outside_file_reads_it_unless_the_target_is_protected() {
    for site in [Site::new(), Site::unrestricted()] {
        site.outside("notes.md", "kept outside\n");
        site.outside("credentials", "TOKEN=hunter2\n");
        site.link(site.other.path().join("notes.md"), "notes.md");
        site.link(site.other.path().join("credentials"), "readme.txt");
        site.reads("notes.md", "kept outside");
        if site.policy.is_unrestricted() {
            site.reads("readme.txt", "TOKEN=hunter2");
        } else {
            site.refuses("readme.txt", "hunter2");
        }
    }
}

#[test]
fn a_dotdot_path_reads_what_it_lands_on_and_refuses_a_protected_landing() {
    let site = Site::new();
    site.file("real.txt", "inside body\n");
    std::fs::create_dir(site.dir.path().join("sub")).unwrap();
    site.outside("notes.md", "kept outside\n");
    site.outside("credentials", "TOKEN=hunter2\n");
    let (home, away) = (Site::name(&site.dir), Site::name(&site.other));
    site.reads("sub/../real.txt", "inside body");
    site.reads(&format!("../{home}/real.txt"), "inside body");
    site.reads(&format!("../{home}/sub/../real.txt"), "inside body");
    site.reads(&format!("../{away}/notes.md"), "kept outside");
    site.refuses(&format!("../{away}/credentials"), "hunter2");
}

#[test]
fn a_dotdot_after_a_symlinked_folder_is_judged_where_the_os_lands() {
    let site = Site::new();
    std::fs::create_dir(site.other.path().join("inner")).unwrap();
    site.outside("notes.md", "kept outside\n");
    site.outside("credentials", "TOKEN=hunter2\n");
    site.link(site.other.path().join("inner"), "door");
    site.reads("door/../notes.md", "kept outside");
    site.refuses("door/../credentials", "hunter2");
}

#[test]
fn a_protected_file_behind_an_innocent_symlink_is_refused_like_the_direct_path() {
    let site = Site::new();
    site.file(".env", "API_KEY=hunter2\n");
    std::fs::create_dir(site.dir.path().join("docs")).unwrap();
    site.link(site.dir.path().join(".env"), "config.txt");
    site.link("../.env", "docs/setup.md");
    site.refuses(".env", "hunter2");
    site.refuses("config.txt", "hunter2");
    site.refuses("docs/setup.md", "hunter2");
}

#[test]
fn an_image_read_through_a_symlink_follows_the_same_rules_as_text() {
    let site = Site::new();
    site.file("board.png", png());
    site.outside("outside.png", png());
    site.outside("credentials", png());
    site.link(site.dir.path().join("board.png"), "pic.png");
    site.link(site.other.path().join("outside.png"), "far.png");
    site.link(site.other.path().join("credentials"), "photo.png");
    for op in [None, Some("image")] {
        let ask = |path: &str| match op {
            Some(op) => serde_json::json!({ "op": op, "path": path }),
            None => serde_json::json!({ "path": path }),
        };
        for path in ["pic.png", "far.png"] {
            let output = site
                .read(ask(path))
                .unwrap_or_else(|error| panic!("{path}: {error}"));
            assert_eq!(output.images.len(), 1, "{op:?} {path}: {}", output.text);
        }
        let refused = site.read(ask("photo.png")).err().unwrap_or_default();
        assert!(
            refused.contains("`photo.png` is a protected path"),
            "{op:?}: {refused}"
        );
    }
}
