# Summonr TUI API

Status: the shared surface, recorded 2026-09-18. The target set in §2 is a
draft; §1 is what other TUI modules may call today.

Rule: a helper another TUI module calls is a row on this page in the same
change, or it stays private to its file. A job not on this page is not a shared
job. One job, one name.

Scope: this page covers `crates/ui`. The crate-wide surface stays on
[`module-api.md`](module-api.md).

## Application hosts

| Row | Job | Callers |
| --- | --- | --- |
| `tui::desktop(prepare)` | launch the native Sigil application; select a project, prepare its runtime off the UI thread, and mount the existing desktop root | CLI desktop startup |
| `tui::run` | run the terminal interface with the existing session hub | CLI terminal startup |
| `desktop::welcome::view` | project selection through the native folder dialog, typed paths, and searchable history from the session catalog; recoverable startup errors and the website-derived S entrance | native application host |

The desktop initializer receives the chosen project path and UI channel and returns the existing `Loop`, `Info`, and hub `Factory`. It must not run workspace tools before selection. The host owns startup cancellation and worker shutdown. Backend selection stays in the existing endpoint/model UI, including the Claude Code binary transport.

## 1. Canvas rows (today)

Owner: `crates/ui/src/canvas.rs`. These were spread over `chat.rs` and
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

## 1a. Status bar

Owner: `crates/ui/src/chat/strip.rs`; `strip/details.rs` renders the
shared Tasks/Agents popup with Sigil's measured virtual list.

| Row | Job | Callers |
| --- | --- | --- |
| `strip::{bar, rows}` | render and measure one adaptive status row for session state, estimated live throughput, task progress, agents, and failures | `chat/panels.rs`, `host/run.rs`, desktop thread |
| `strip::working` | detect parent or visible child work for the animation clock | `app/frame.rs`, status bar |
| `strip::sheet`, `App::{status_sheet_open, close_status_sheet}` | render, open, and dismiss the shared details popup | `app/overlays.rs` |
| `StatusView`, `App::show_status` | select the closed, Tasks, or Agents view without changing transcript layout | status bar and popup |
| `App::{toggle_agents, toggle_todos}` | open or close the corresponding status popup view | configured hotkeys |
| `App::{step_status, move_status, expand_status}` | switch views, select a row, and expand its details | overlay keyboard routing |
| `Model::clear_agent` | dismiss a background agent from the status bar and popup; keep execution, accounting, and transcript reports | `App::clear_agent` for the button; `Model::apply_child` for native `task` dismissal events |
| `Model::agents_hidden` | child tags dismissed in the current view; late events do not reveal them | `Model::clear_agent`, status bar and popup |
| `todos::tally` | count completed and total task rows from the existing ledger | status bar, ledger pane |

Live throughput belongs to the main response, not an aggregate of child rates.
It is hidden during tools, approvals, and idle periods. Task-only activity shows
the current step; tasks with agents use a compact count. Completed results remain
available until the transcript is cleared or reset. Approvals, forms, and attached
images keep their existing controls.

## Composer

| Job | Owner | Callers |
| --- | --- | --- |
| Prompt editor | `view/surface.rs::editor`, Sigil `text_edit` | transcript and splash; auto height with a viewport-bounded scroll area |
| Inline composer | `chat/surface.rs::inline_layer` | inline transcript and splash; shared command completion, composer, and footer |
| Splash placement | `scene/input.rs::prompt_area` | `view/splash.rs`; the composer holds its row under the card, grows down, and moves up only when it reaches the bottom of the screen |

Typing, wrapping, newlines, and resizing do not dismiss the splash. A transcript entry or explicit navigation does.

## Themes

Owner: `theme.rs` loads the existing built-in palettes and `theme.toml`
overrides; `theme/tokens.rs` maps that palette to Sigil tokens for both the
terminal and desktop roots. Overlay surfaces use the palette's base RGB with an
opaque alpha, including when the terminal background is transparent.

| Job | Owner | Callers |
| --- | --- | --- |
| Selected palette to widget theme | `theme::style` | terminal root, splash composer, desktop root |
| Enumerate, edit, and save palette colors | `Palette::{colors, set, save}`, `theme::{load, hex, color_hex}` | theme loader and editor; save uses the existing atomic store and preserves TOML comments |
| Theme editor modal and draft state | `settings/theme.rs::{Editor, layer, keys}` | Options theme row, overlay routing |
| Preview, commit, or cancel theme edits | `App::{open_theme, preview_color, edit_theme, move_theme, step_theme, reset_theme, cancel_theme, close_theme, save_theme}` | theme editor and overlay routing |

Enter on the theme row opens the editor; left/right retains preset cycling.
Edits preview live without changing saved preferences. Save writes colors to
`.summonr/theme.toml`; Cancel restores the pre-editor palette. Invalid colors and
save errors stay visible without closing the editor.

## Splash animation

| Item | Owner | Callers |
| --- | --- | --- |
| `scene::reveal_ms` | `scene/logo.rs` | `app/frame.rs`; completion time for the visible card's entrance |

The entrance stages attention from the S to the wordmark to the session
facts. Related actions overlap; the face shimmer waits until the entrance
finishes. The composer remains available throughout.

| Card | S build | Wordmark sweep | Facts fade | Settled |
| --- | --- | --- | --- | --- |
| Lockup | 0–600 ms | 480–960 ms | 1120–1280 ms | 1280 ms |
| Mark only | 0–600 ms | Not shown | 600–760 ms | 760 ms |
| Banner only | Not shown | 0–480 ms | 640–800 ms | 800 ms |

The first S layer appears immediately; subsequent layers arrive every
120 ms. Each wordmark column fades in over 160 ms with sine ease-out,
then settles from the star highlight to the text color over 160 ms with
sine ease-in-out. Facts fade together with sine ease-out during the last
column's color settle. The durations align with the default 40 ms cadence.
Typing or disabling animations reveals the card immediately.
The completion timer waits for both the visible card's sequence and
`splash_reveal_ms`.

The choreography applies [staging, overlapping action, and easing](https://www.adobe.com/creativecloud/animation/discover/principles-of-animation.html),
with [sequencing](https://www.carbondesignsystem.com/building-blocks/foundations/motion/choreography)
to preserve information hierarchy. These principles guide the score; they
do not prescribe its exact durations.

## Headless terminal captures

The existing `--profile-tui` developer runner captures the actual terminal UI without a desktop, credentials, network, or live agents. Keep `--profile` as the customer session profiler; do not add a second capture command.

```sh
cargo run --features gui -- --profile-tui agents --frames 1 --history 0 --cols 120 --rows 40 --output /tmp/agents.png
cargo run --features gui -- --profile-tui help --frames 1 --history 0 --cols 120 --rows 40 --output /tmp/help.png
cargo run --features gui -- --profile-tui agents-failed --frames 1 --history 0 --cols 120 --rows 40 --output /tmp/agents-failed.png
```

`gui` supplies Sigil's existing CPU image renderer here; these commands do not open a window or require a GPU. Each run writes `.png`, `.json` (frame trace), `.ansi`, and `.txt` artifacts with the same stem. Images use the bundled monospace font at 10×20 pixels per terminal cell. A terminal-only build still writes traces, ANSI, and text; requesting a `.png` output on that build fails with a build instruction.

The `help`, `agents`, and `agents-failed` scenarios exercise production UI state through existing application events. They verify expected visible content before accepting a capture. A clipped or missing scene fails instead of reporting a successful screenshot. Add reproduction inputs in the private `profile_scenes.rs` owner, not a new replay framework. `idle`, `stream`, and `resize` retain their existing profiling behavior.

For A/B work, run the same scenario, dimensions, history, frame count, FPS, and build profile on each revision with different output stems. Compare the text and PNGs, inspect the matching frame traces, and run the relevant behavioral tests. A pixel difference is not proof of better UX or performance. Captures use Sigil's terminal cells, not a particular terminal application's font rasterization; PTY tests remain the end-to-end terminal boundary.

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

Owner: `crates/ui/src/overlay.rs`. The one visual language for every
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
