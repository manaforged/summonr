use super::{Part, Shown, Stream, parts, stats_of};

#[test]
fn a_quoted_stat_and_patch_use_the_diff_card() {
    let reply = "\
Stat view

```
$ summonr diff --stat
+3     −6     src/hashline.rs
1 file · +3 −6 · vs HEAD
```

Patch view

```
$ summonr diff
diff --git a/src/hashline.rs b/src/hashline.rs
index cb0af58..ed0a5a9 100644
--- a/src/hashline.rs
+++ b/src/hashline.rs
@@ -110,9 +110,6 @@ fn hits() {
-    let hash = line_hash(line);
+    line_hash(line)
```

```rust
fn hits() {}
```
";
    let parts = parts(reply);
    assert_eq!(
        parts.iter().map(kind).collect::<Vec<_>>(),
        ["prose", "stat", "prose", "patch", "code"]
    );
    let Part::Shown(Shown::Stat { rows, summary }) = &parts[1] else {
        panic!("stat");
    };
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "src/hashline.rs");
    assert_eq!((rows[0].added, rows[0].removed), (3, 6));
    assert_eq!(summary.as_deref(), Some("1 file · +3 −6 · vs HEAD"));
    let Part::Shown(Shown::Patch { path, patch }) = &parts[3] else {
        panic!("patch");
    };
    assert_eq!(path.as_deref(), Some("src/hashline.rs"));
    assert!(patch.contains("@@ -110,9 +110,6 @@ fn hits() {"), "{patch}");
    assert!(patch.contains("+    line_hash(line)"), "{patch}");
    assert!(!patch.contains("diff --git"), "{patch}");
    assert!(!patch.contains("index "), "{patch}");
    let Part::Prose(code) = &parts[4] else {
        panic!("code");
    };
    assert!(code.contains("fn hits() {}"), "{code}");
    assert!(code.contains("```rust"), "{code}");

    let (rows, _) = stats_of("+3 −6 src/hashline.rs\n1 file · +3 −6 · vs HEAD").unwrap();
    assert_eq!(rows[0].path, "src/hashline.rs");
    assert_eq!((rows[0].added, rows[0].removed), (3, 6));
}

#[test]
fn a_command_log_is_not_a_stat_card() {
    assert!(stats_of("cargo test failed\nerror: could not compile").is_none());
}

fn kind(part: &Part) -> &'static str {
    match part {
        Part::Prose(text) if text.contains("```") => "code",
        Part::Prose(_) => "prose",
        Part::Shown(Shown::Stat { .. }) => "stat",
        Part::Shown(Shown::Patch { .. }) => "patch",
    }
}

#[test]
fn a_streamed_reply_parses_like_the_whole_text() {
    let reply = "Intro line\r\nmore prose\n\n```\n$ summonr diff --stat\n+3     −6     src/hashline.rs\n1 file · +3 −6 · vs HEAD\n```\n\n```text\nplain fence\n```\ntail prose\n```rust\nfn open() {\n";
    let mut stream = Stream::default();
    for end in (0..=reply.len()).filter(|&end| reply.is_char_boundary(end)) {
        assert!(
            stream.parts(&reply[..end]) == parts(&reply[..end]),
            "differs after {end} bytes"
        );
    }
    assert!(stream.parts("other text") == parts("other text"));
}
