# Summonr TUI API

Status: the shared surface, recorded 2026-09-18. The target set in §2 is a
draft; §1 is what other TUI modules may call today.

Rule: a helper another TUI module calls is a row on this page in the same
change, or it stays private to its file. A job not on this page is not a shared
job. One job, one name.

Scope: this page covers `src/tui`. The crate-wide surface stays on
[`module-api.md`](module-api.md).

## 1. Canvas rows (today)

Owner: `src/tui/sigil_view/canvas.rs`. These were spread over `chat.rs` and
`settings.rs` until 2026-09-18 and are the primitives the redesign unwinds.

| Row | Job | Callers |
| --- | --- | --- |
| `Cell`, `Cell::blank` | one grid cell | 16 files |
| `Span` (`text`, `styled`, `surface`, `bold`, `italic`, `width`, `source`, `slice`) | one styled run | 8 files |
| `Line` | one painted row | 8 files |
| `write_at` | text at (col, row) of a grid | 7 files |
| `write_line_at` | spans at (col, row) of a grid | 5 files |
| `ellipsize`, `truncate` | clip to a cell width | 15 files |
| `frame`, `footer`, `card` | a bordered panel, its hint row, its bounds | `settings.rs`, `usage.rs` |
| `palette` | the palette for one theme | 20 files |
| `rgb`, `blend`, `cell`, `cell_aspect`, `window_cells` | color and cell metrics | 8 files |
| `dialog_width` | modal card width from terminal cols | `settings/models.rs`, `settings/auth.rs` |

`Line` is defined twice: `canvas.rs` holds `Vec<Span>`, and `panes.rs:18`
holds `{ text, fg, bg }`. The second one is on the delete list in §3.

`ImageSource` and `ImageSource::id` are Sigil's.

## 1b. Overlay kit (2026-09-19)

Owner: `src/tui/sigil_view/kit.rs`. The one visual language for every
overlay list: accent selection band, `❯` cursor gutter, right-aligned
status, hint bar, themed field. Chat roles (`user_bg`, `tool_bg`) never
appear outside the transcript.

| Row | Job | Callers |
| --- | --- | --- |
| `kit::row` | one list row: cursor gutter, name, context, right status | `settings/auth.rs`, `settings/models.rs`, `sessions.rs` |
| `kit::group` | a dim lowercase section header row | `settings/models.rs`, `settings/auth.rs` |
| `kit::hint_bar` | the overlay footer: dim keys left, accent counter right | `settings/auth.rs`, `settings/models.rs`, `sessions.rs`, `search.rs` |
| `kit::field` | the one text field: placeholder, focus trigger, optional mask | `settings/auth.rs`, `settings/models.rs` |
| `kit::band` | a tone-tinted status band (errors, empty states) | `settings/models.rs` |
| `kit::Tone` | status vocabulary: `Ok` green, `Dim` gray, `Warn` yellow, `Err` red | all of the above |

## 2. Target set (redesign)

`docs/plans/2026-09-18-tui-widget-redesign.md` reduces §1 to this set. Each row
is one job with one owner; Sigil owns the rest.

| Row | Job | Owner |
| --- | --- | --- |
| `Overlay` | one overlay: `open`/`close`/`key`/`node` | one file per overlay |
| `List` | cursor + scroll over a length | `tui/list.rs` |
| `Query` | text + caret | `tui/query.rs` |
| `Focus` | which owner gets the key | `tui/focus.rs` |
| `Command` | one slash command: name, arg, run, help | `tui/command.rs` |
| `Surface` | one grid plus `write`/`ellipsis`/`into_cells` | `tui/canvas.rs`, splash only |

## 3. Names we delete

Verified against the pinned Sigil revision
`b5a7ccb23131d8b98590a7200cf75dff1a396483`.

| Delete | Keep | Why |
| --- | --- | --- |
| `panes::Line { text, fg, bg }` | one row type | a second `Line` for the same job as `canvas::Line` (`panes.rs:18`, `panes/ledger.rs:2`) — still open |
| `settings::frame`/`footer`/`card` inline in `sessions`, `search`, `help` | `canvas::frame`/`footer`/`card` | landed: `sessions` and `search` no longer paint cells at all |
| `suggest`'s picker list | Sigil `command_menu` | the slash palette is a command menu |
| `settings`/`sessions`/`search`/`help`/`usage` grid painters | Sigil `modal` + `table`/`virtual_list` | landed for `help`, `sessions`, `search`, `settings`, `hotkeys`; `usage` and the three settings sub-overlays still paint cells |
| `Entry::Note` drawn as a transcript line | Sigil `toast`/`toast_queue` | a note is a toast, not a transcript entry |
| `edits`' hand-drawn card rules | Sigil `panel` + `divider` | `panel.rs` and `divider.rs` ship the border |
| the `Tasks n/m` cell strip | Sigil `progress`/`gauge` | a count with a bar is a progress widget |
| pane switching by hotkey+cell hit | Sigil `tabs` | tabs own the selection |
| pane resize by `nudge_split` | Sigil `splitter` | a splitter owns the drag |
| diff rendering by hand | Sigil `code_edit` | the diff is a buffer with spans |
| `Cell`, `Span`, `write_at`, `write_line_at`, `ellipsize` | Sigil `label`/`markdown`/`code_edit` | once the panes are widgets, no caller paints cells except the splash |
