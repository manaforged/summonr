pub(crate) fn read() -> String {
    format!(
        "read op reference: every form honors ignore rules unless no_ignore is true; search files with the search tool.

op=file (default with path): N:text lines, one page of up to {kb} KiB by default; offset starts at line N, limit caps lines (max {max}); paths[] reads several files and reports missing ones inline; tags true adds hash tags for tag-edits.
op=list: directory listing; recursive true or depth N returns a tree; truncation notes say how to narrow.
op=image: returns the image so you can see it; a model without image input gets a description or a note instead.",
        max = crate::limits::limits().read_line_limit,
        kb = super::budget::SHAPED_BUDGET / 1024,
    )
}

pub(crate) const EDIT: &str = "edit grammar: tags are N:hash strings that a read with tags true returns.

- old+new: replace the unique substring (replace_all true replaces every occurrence); a whitespace-only mismatch self-corrects and says so; zero matches errors with the closest lines, several matches lists their line numbers
- tag+new: replace the tagged line(s), end_tag extends the range
- tag+text: insert after the tagged line
- op=delete: remove tagged lines
- edits[]: apply several of the above atomically; if two overlap they apply in order, each against the result of the one before
- op=sequence: resolve edits in order
- patch: a full *** Begin Patch / Update|Add|Delete File document with @@ context lines; multiple files per document; every file is staged first, so a failed stage changes nothing
- path+to: rename or move the file
New or empty files: use write.";

pub(crate) const RUN_COMMAND: &str = "run_command reference.

- command runs via the shell in the workspace; workdir changes the directory; cwd does not persist between calls; stdin is closed; the environment is a safe allowlist
- at the wait limit a command keeps running as a background job and returns its id; an explicit timeout_secs stops it instead; when the shell exits, leftover processes it started with & are stopped; background true returns a job id at once and rejects timeout_secs; jobs keep their log and exit status after summonr exits; pass job to wait for it (a poll waits until the job exits or the wait limit), kill true to stop; cancelling a reply keeps background jobs, so inspect or wait on an existing id before launching another
- output is the exit code plus about 16 KB of head and tail; only when that cuts something is the full log path named; read it with the read tool; search command output gets clipped per file with an omission note
- for a password use `sudo -A` or plain ssh (masked prompt); `summonr askpass \"<label>\"` prints a user-typed secret to pipe into a command; never echo or store it";

pub(crate) fn search() -> String {
    format!(
        "search reference: case is ignored unless case_sensitive is true; a call returns what it found after {secs}s.

- pattern: regex over file contents, a|b for alternatives; literal true takes it as plain text; word true matches whole words
- name: words that must all appear in the file name, any order (\"q3 report\"); glob: a file pattern such as *.rs or src/**/*.ts; with pattern, both narrow the files searched
- path: folder or file to search; ~ and absolute paths work; default the workspace; depth N limits folder levels; no_ignore true includes gitignored files and build folders
- output: lines (default) shows matches, files lists matching files, count counts per file, folders counts files per folder; context N adds lines around each match (max {context})
- newer: only files changed within 30m, 2h, 3d, or 2w
- limit: results to return (default {default}, max {max}); offset skips files for the next page; tags true prints N:hash tags that edit takes",
        secs = crate::limits::limits().search_secs,
        context = super::workspace::search::CONTEXT_MAX,
        default = super::workspace::search::GREP_DEFAULT,
        max = super::workspace::search::GREP_MAX,
    )
}
