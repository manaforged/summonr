pub(crate) fn read() -> String {
    format!(
        "read op reference: every form honors ignore rules unless no_ignore is true.

op=file (default with path): N:text lines, one page of up to 50 KB by default; offset starts at line N, limit caps lines (max {max}); paths[] reads several files and reports missing ones inline; tags true adds hash tags for tag-edits.
op=list: directory listing; recursive true or depth N returns a tree; truncation notes say how to narrow.
op=search (default when pattern or glob is set): paths[] searches each listed path; pattern is regex (literal true skips it; caseSensitive defaults true); glob/include filters paths, type names a file type (rust, py, js…), exclude drops a glob; files returns matching paths; count returns per-file match counts; context or before/after add lines; multiline matches across lines; hits are path:N:col:line grouped per file, where col is the match's 1-based UTF-16 column that lsp takes as character; tags true prints path:N:hash|line instead, and N:hash is the tag an edit takes.
op=image: returns the image so you can see it; a model without image input gets a description or a note instead.",
        max = crate::limits::READ_LINE_LIMIT
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
- at the wait limit a command keeps running as a background job and returns its id; an explicit timeout_secs stops it instead; background true returns a job id at once; pass job to wait for it (a poll waits until the job exits or the wait limit), kill true to stop
- output is the exit code plus about 16 KB of head and tail; only when that cuts something is the full log path named; read it with the read tool; search command output gets clipped per file with an omission note
- for a password use `sudo -A` or plain ssh (masked prompt); `summonr askpass \"<label>\"` prints a user-typed secret to pipe into a command; never echo or store it";
