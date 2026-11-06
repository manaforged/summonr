use sigil::prelude::MenuItem;

use super::*;

pub(crate) type Act = fn(&App, &PaneRow, &str);

const HUNK: &[(&str, Act)] = &[
    ("accept hunk", |app, row, hunk| {
        app.hunk_toggle(&row.path, hunk)
    }),
    ("reject hunk", |app, row, hunk| {
        app.hunk_mark(&row.path, hunk, summonr_agent::hunks::Op::Reject);
    }),
    ("note hunk", |app, row, hunk| {
        app.note_start(row.path.clone(), hunk.to_string());
    }),
];
pub(crate) const JOB: &[(&str, Act)] = &[
    ("kill job", |app, row, _| app.job_kill(&row.path)),
    ("view log", |app, row, _| {
        app.view_open(app.job_log(&row.path))
    }),
];
pub(crate) const MCP: &[(&str, Act)] = &[("view server", |app, row, _| app.open_row(row))];
pub(crate) const PEER: &[(&str, Act)] =
    &[("mention @name", |app, row, _| app.peer_mention(&row.path))];
pub(crate) const TREE: &[(&str, Act)] = &[
    ("show diff", |app, row, _| {
        app.tree_show(PathBuf::from(&row.path), row.name.clone());
    }),
    ("merge (ff-only)", |app, row, _| {
        app.tree_merge(PathBuf::from(&row.path))
    }),
    ("remove worktree", |app, row, _| {
        app.tree_discard(PathBuf::from(&row.path));
    }),
    ("insert path", App::insert_path),
];
pub(crate) const CHANGE: &[(&str, Act)] = &[
    ("view hunks", |app, row, _| app.diff_open(row.path.clone())),
    ("view file", |app, row, _| app.view_open(row.path.clone())),
    ("attach @path", |app, row, _| app.mention(&row.path)),
    ("open in $EDITOR", App::open_path),
    ("insert path", App::insert_path),
];
pub(crate) const FILE: &[(&str, Act)] = &[
    ("attach @path", |app, row, _| app.mention(&row.path)),
    ("view file", |app, row, _| app.view_open(row.path.clone())),
    ("open in $EDITOR", App::open_path),
    ("insert path", App::insert_path),
];
pub(crate) const DIR: &[(&str, Act)] = &[("insert path", App::insert_path)];

impl App {
    pub(crate) fn pane_point(&self, kind: Pane, index: usize) {
        self.pane_list(kind).select(index);
        self.pane_focus.set(true);
        self.refresh();
    }

    pub(crate) fn row_menu(&self, kind: Pane, index: usize) -> Vec<MenuItem> {
        let Some(row) = self.pane_rows(kind).into_iter().nth(index) else {
            return Vec::new();
        };
        let mut items = self.items(self.pane_menu(kind, &row), &row, "");
        if row.dir {
            let app = self.clone();
            let label = if row.open { "collapse" } else { "expand" };
            items.insert(0, MenuItem::new(label).activate(move || app.pane_enter()));
        }
        items
    }

    pub(crate) fn hunk_menu(&self, index: usize) -> Vec<MenuItem> {
        let (Some(path), Some(hunk)) = (self.diff_view.peek(), self.hunks().get(index).cloned())
        else {
            return Vec::new();
        };
        self.items(HUNK, &PaneRow::flat(path, hunk.header, false), &hunk.id)
    }

    fn items(&self, acts: &[(&str, Act)], row: &PaneRow, hunk: &str) -> Vec<MenuItem> {
        acts.iter()
            .map(|&(label, act)| {
                let (app, row, hunk) = (self.clone(), row.clone(), hunk.to_string());
                MenuItem::new(label).activate(move || act(&app, &row, &hunk))
            })
            .collect()
    }

    fn job_log(&self, id: &str) -> String {
        summonr_agent::jobs::load(&self.workspace(), id).map_or_else(
            |_| format!(".summonr/jobs/{id}.log"),
            |record| {
                summonr_agent::jobs::log_path(&self.workspace(), &record)
                    .display()
                    .to_string()
            },
        )
    }

    fn open_path(&self, row: &PaneRow, _: &str) {
        let root = self.root().unwrap_or_else(|| self.workspace());
        self.open_file.set(Some(root.join(&row.path)));
        self.refresh();
    }

    fn insert_path(&self, row: &PaneRow, _: &str) {
        let mut draft = self.draft.peek();
        draft.push_str(&row.path);
        self.replace(draft);
        self.pane_focus.set(false);
        self.refresh();
    }
}
