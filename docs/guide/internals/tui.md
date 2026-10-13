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

## 1a. Child status rows

Owner: `src/tui/sigil_view/chat/strip.rs`, with display state in `model.rs`
and actions in `app/adjust.rs`.

| Row | Job | Callers |
| --- | --- | --- |
| `App::clear_agent` | dismiss a background status row; keep execution, accounting, and transcript reports | `chat/strip.rs` |
| `Model::agents_hidden` | child tags dismissed in the current view; late events do not reveal them | `App::clear_agent`, `chat/strip.rs` |

## Composer

| Job | Owner | Callers |
| --- | --- | --- |
| Prompt editor | `view/surface.rs::editor`, Sigil `text_edit` | transcript and splash; auto height with a viewport-bounded scroll area |
| Inline composer | `chat/surface.rs::inline_layer` | inline transcript and splash; shared command completion, composer, and footer |
| Splash placement | `scene/input.rs::prompt_area` | `view/splash.rs`; the composer holds its row under the card, grows down, and moves up only when it reaches the bottom of the screen |

Typing, wrapping, newlines, and resizing do not dismiss the splash. A transcript entry or explicit navigation does.

## Composer Tab bindings

Owner: `view/input.rs::tab_shortcuts`, attached to `view/keys.rs::shell`
through Sigil capture handlers. There are no hidden Tab shortcut nodes.
Tab navigates an open overlay or accepts a completion before cycling effort.
Shift+Tab navigates an open overlay or sends `Cmd::ToggleApproval`.
`Snapshot::approval` reflects effective policy; it is separate from work mode
and is updated by the worker, not optimistically by the key handler.

## Composer keyboard routing

| Job | Owner | Callers |
| --- | --- | --- |
| Ctrl bindings and readline motion | `view/input.rs::leader_shortcuts` | `view/keys.rs::shell`; rewrites motion keys for Sigil's editor |
| Newline binding | `view/input.rs::hotkey_shortcuts` | `view/keys.rs::shell`; rewrites the configured newline key to Enter |
| Chord continuation | `view/input.rs::leader_plain`, `App::{arm_leader,take_chord,route_chord}` | `view/keys.rs::shell`, `view/input.rs`; Ctrl+X arms one following letter, Escape cancels |
| Hotkey lookup and rebinding | `prefs::Hotkeys::{action,chord,assign,label}`, `App::record_chord` | `app/adjust.rs`, `view/input.rs`, `settings/hotkeys.rs`, composer and help; plain Ctrl keys and Ctrl+X chords remain distinct |

## 1b. Overlay helpers (2026-09-19)

Owner: `src/tui/sigil_view/overlay.rs`. The one visual language for every
overlay list: accent selection band, `❯` cursor gutter, right-aligned
status, hint bar, themed field. Chat roles (`user_bg`, `tool_bg`) never
appear outside the transcript.

| Row | Job | Callers |
| --- | --- | --- |
| `overlay::shell` | the modal frame: title, width, focus trap | every overlay |
| `overlay::pick` | one list row: cursor gutter, name, context, right status | `settings/auth.rs`, `settings/models.rs`, `sessions.rs` |
| `overlay::group` | a dim lowercase section header row | `settings/models.rs`, `settings/auth.rs` |
| `overlay::hint` | the overlay footer: dim keys left, accent counter right | `settings/auth.rs`, `settings/models.rs`, `sessions.rs`, `search.rs` |
| `overlay::band` | a tinted status band (errors, empty states) | `settings/models.rs` |
| `settings/auth.rs::field` | the one text field: placeholder, focus trigger, optional mask | `settings/auth.rs`, `settings/models.rs` |
| Sigil `Tone` | status vocabulary: `Ok` green, `Dim` gray, `Warn` yellow, `Err` red | all of the above |

## 2. Target set (redesign)

The redesign reduces §1 to this set. Each row
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
`e09637ac`.

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
