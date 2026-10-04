use crate::command::Category;
use crate::prefs::{Hotkey, Hotkeys};
use crate::provider::Effort;

pub(crate) fn help_text(keys: &Hotkeys) -> String {
    let specs: Vec<_> = crate::command_specs()
        .iter()
        .filter(|spec| spec.shown())
        .map(|spec| (spec.category, spec.name, spec.description))
        .collect();
    let width = specs.iter().map(|spec| spec.1.len()).max().unwrap_or(0);
    let mut lines = Vec::new();
    for category in Category::ALL {
        if !specs.iter().any(|spec| spec.0 == category) {
            continue;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(category.title().to_string());
        lines.extend(
            specs
                .iter()
                .filter(|spec| spec.0 == category)
                .map(|spec| format!("  {:<width$}  {}", spec.1, spec.2)),
        );
    }
    lines.extend(
        [
            "".to_string(),
            "composer".to_string(),
            "  enter        send; during a turn, queue, then enter again sends it now".to_string(),
            "  tab          complete a /command or @path".to_string(),
            "  ?            on an empty draft, show this help".to_string(),
            "  ↑            on an empty draft, pull queued input back".to_string(),
            format!(
                "  shift+tab    cycle reasoning effort: {}",
                Effort::ALL.map(Effort::as_str).join(", ")
            ),
            "  esc          cancel; twice on an idle composer opens the session tree".to_string(),
            "  ctrl+c       clear the draft; twice on an empty draft quits".to_string(),
            "  ctrl+d       quit when the draft is empty".to_string(),
            format!("  {:<12} newline", keys.label(Hotkey::Newline)),
            format!("  {:<12} edit the draft in $EDITOR", keys.label(Hotkey::Editor)),
            "  ctrl+z/y     undo or redo the draft".to_string(),
            "  alt+←/→      word left or right · alt+b/f the same".to_string(),
            "  ctrl+w       delete the word before the caret · alt+d after".to_string(),
            "  ctrl+u/k     delete to the line start or end".to_string(),
            "  ↑↓           recall prompts at the first or last line".to_string(),
            "  @path        attach text or an image".to_string(),
            "  !cmd         run in the sandbox and send the output; !!cmd runs only".to_string(),
            format!(
                "  a paste of {}+ lines or {}+ characters folds into [pasted N lines #k] until sent",
                crate::limits::PASTE_FOLD_LINES,
                crate::limits::PASTE_FOLD_CHARS
            ),
            "".to_string(),
            "interface".to_string(),
            format!("  {:<12} settings, same as /settings (hotkeys are editable there)", keys.label(Hotkey::Options)),
            format!("  {:<12} search the conversation · /search TEXT", keys.label(Hotkey::Search)),
            format!("  {:<12} prompt history · /history TEXT", keys.label(Hotkey::History)),
            format!("  {:<12} fold thinking, then tool output; again shows all", keys.label(Hotkey::Fold)),
            format!("  {:<12} files · {} ledger · {} diff · /pane worktrees", keys.label(Hotkey::Files), keys.label(Hotkey::Ledger), keys.label(Hotkey::Diff)),
            format!("  {:<12} hide or show the pane · ctrl+→ focus it · ctrl+← composer", keys.label(Hotkey::Pane)),
            format!("  {:<12} copy the last reply · /copy all · {} select text", keys.label(Hotkey::Copy), keys.label(Hotkey::Select)),
            "  ctrl+shift+←/→".to_string(),
            "               resize the split · pgup/pgdn or wheel scroll".to_string(),
            "  home/end     on an empty draft, jump to the top or the latest reply · ctrl+home/end always".to_string(),
            "  in a pane    ↑↓ move · ←→ collapse/expand or hunks/list · enter · right-click menu · esc".to_string(),
            "  view         → on a file or /view PATH · o opens in $EDITOR · e edits in place".to_string(),
            "  diff         t last turn only · c copy the patch".to_string(),
            "  sessions     ↑↓ · enter resume · tab scope · alt+d twice deletes · esc".to_string(),
            "  models       ↑↓ · enter use · alt+p favorite · alt+o older · alt+d default · F5 refresh".to_string(),
            "  workflow     ↑↓ role · enter seat · d driving · ←→ saved · s save".to_string(),
        ],
    );
    lines.join("\n")
}
