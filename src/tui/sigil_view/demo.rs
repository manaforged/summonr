use super::model::Entry;
use crate::tui::{Round, ToolState};

const MARKDOWN: &str = "# render demo\n\
\n\
Assistant markdown: **bold**, *italic*, `inline code`, and a [link](#demo).\n\
\n\
- first item\n\
- second item\n\
\n\
| name | kind | rows |\n\
| --- | --- | --- |\n\
| markdown | widget | live |\n\
| mermaid | diagram | boxes |\n\
\n\
```rust\n\
fn main() {\n\
    println!(\"fenced code\");\n\
}\n\
```\n\
\n\
```mermaid\n\
graph TD\n\
A[Prompt] --> B[Model]\n\
B --> C[Reply]\n\
```\n\
\n\
```chart\n\
Tokens\n\
prompt: 420\n\
completion: 260\n\
cached: 610\n\
```\n\
\n\
```xychart-beta\n\
title \"rounds\"\n\
x-axis [r1, r2, r3, r4]\n\
bar [3, 8, 5, 9]\n\
```\n\
\n\
![pixel](data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAFklEQVR4nGNwOLGPJMQwqmFUw/DVAADdusYQGlZlxwAAAABJRU5ErkJggg==)\n";

const DIFF: &str = "diff --git a/src/demo.rs b/src/demo.rs\n\
--- a/src/demo.rs\n\
+++ b/src/demo.rs\n\
@@ -1,3 +1,3 @@\n fn main() {\n-    let render = \"cells\";\n+    let render = \"sigil\";\n }\n";

pub(super) fn entries() -> Vec<Entry> {
    vec![
        Entry::Assistant {
            text: MARKDOWN.to_string(),
            done: true,
        },
        Entry::Tool {
            name: "edit".to_string(),
            arg: "src/demo.rs".to_string(),
            state: ToolState::Ok,
            output: DIFF.to_string(),
        },
        Entry::Chart(vec![
            Round {
                prompt: 420,
                completion: 260,
                reasoning: 40,
                cached: 300,
                prefix_hash: String::new(),
                cost: None,
            },
            Round {
                prompt: 610,
                completion: 190,
                reasoning: 20,
                cached: 480,
                prefix_hash: String::new(),
                cost: None,
            },
        ]),
    ]
}
