use sigil::widgets::CursorEdge;

use super::overlays::context::{Act, CHANGE, DIR, FILE, JOB, MCP, PEER, TREE};
use super::*;
use crate::tui::sigil_view::ui::List;

pub(super) const PANES: usize = 9;

type Menu = fn(&PaneRow) -> &'static [(&'static str, Act)];

struct PaneSpec {
    list: Pane,
    rows: fn(&App) -> Vec<PaneRow>,
    load: fn(&App) -> Option<Cmd>,
    shift: Option<fn(&App, isize)>,
    pick: fn(&App, &PaneRow),
    enter: Option<fn(&App)>,
    expand: Option<fn(&App)>,
    collapse: Option<fn(&App)>,
    click: Option<fn(&App, &PaneRow)>,
    menu: Menu,
    tree: bool,
    poll: bool,
}

const FILES: PaneSpec = PaneSpec {
    list: Pane::Files,
    rows: App::file_rows,
    load: |_| None,
    shift: None,
    pick: App::file_pick,
    enter: None,
    expand: Some(App::file_expand),
    collapse: Some(App::file_collapse),
    click: Some(|app, row| {
        if row.dir {
            app.file_pick(row);
        } else {
            app.view_open(row.path.clone());
        }
    }),
    menu: |row| if row.dir { DIR } else { FILE },
    tree: true,
    poll: false,
};

const fn listed(list: Pane, rows: fn(&App) -> Vec<PaneRow>, menu: Menu) -> PaneSpec {
    PaneSpec {
        list,
        rows,
        expand: None,
        collapse: None,
        click: None,
        menu,
        tree: false,
        ..FILES
    }
}

static PANE_SPECS: [PaneSpec; PANES] = [
    FILES,
    listed(Pane::Ledger, |_| Vec::new(), |_| &[]),
    PaneSpec {
        load: |_| Some(Cmd::ListFiles),
        ..FILES
    },
    PaneSpec {
        load: |app| Some(Cmd::Diff(app.root())),
        shift: Some(App::diff_move),
        pick: |app, row| app.diff_open(row.path.clone()),
        enter: Some(App::diff_enter),
        expand: Some(|app| {
            if app.diff_view.peek().is_none() {
                app.diff_enter();
            }
        }),
        collapse: Some(|app| {
            app.pane_back();
        }),
        ..listed(Pane::Diff, App::diff_rows, |_| CHANGE)
    },
    PaneSpec {
        load: |_| Some(Cmd::Trees),
        pick: |app, row| app.tree_show(PathBuf::from(&row.path), row.name.clone()),
        ..listed(Pane::Worktrees, App::tree_rows, |_| TREE)
    },
    PaneSpec {
        load: |_| Some(Cmd::Jobs),
        poll: true,
        pick: App::open_row,
        ..listed(Pane::Jobs, App::job_rows, |_| JOB)
    },
    PaneSpec {
        load: |_| Some(Cmd::Mcp),
        poll: true,
        pick: App::open_row,
        ..listed(Pane::Mcp, App::mcp_rows, |_| MCP)
    },
    PaneSpec {
        pick: |app, row| app.peer_mention(&row.path),
        ..listed(Pane::Peers, App::peer_rows, |_| PEER)
    },
    PaneSpec {
        list: Pane::View,
        shift: Some(App::view_move),
        enter: Some(|_| {}),
        expand: Some(|_| {}),
        collapse: Some(|app| {
            app.pane_back();
        }),
        click: None,
        ..FILES
    },
];

fn spec(pane: Pane) -> &'static PaneSpec {
    &PANE_SPECS[pane as usize]
}

impl App {
    fn current(&self) -> Pane {
        self.layout.peek().secondary.unwrap_or(Pane::Files)
    }

    pub(in crate::tui::sigil_view) fn pane_list(&self, pane: Pane) -> List {
        self.pane_lists[spec(pane).list as usize]
    }

    pub(in crate::tui::sigil_view) fn pane_rows(&self, pane: Pane) -> Vec<PaneRow> {
        (spec(pane).rows)(self)
    }

    pub(in crate::tui::sigil_view) fn pane_tree(pane: Pane) -> bool {
        spec(pane).tree
    }

    pub(in crate::tui::sigil_view) fn pane_polls(pane: Pane) -> bool {
        spec(pane).poll
    }

    pub(in crate::tui::sigil_view) fn load(&self) {
        let Some(pane) = self.layout.peek().secondary else {
            return;
        };
        if let Some(command) = (spec(pane).load)(self) {
            drop(self.cmd.send(command));
        }
    }

    pub(in crate::tui::sigil_view) fn picked(&self, pane: Pane) -> Option<PaneRow> {
        let rows = self.pane_rows(pane);
        let at = self.pane_list(pane).at(rows.len());
        rows.into_iter().nth(at)
    }

    pub(in crate::tui::sigil_view) fn pane_move(&self, delta: isize) {
        let pane = self.current();
        match spec(pane).shift {
            Some(shift) => shift(self, delta),
            None => {
                self.pane_list(pane)
                    .shift(delta, self.pane_rows(pane).len(), CursorEdge::Clamp);
                self.refresh();
            }
        }
    }

    pub(in crate::tui::sigil_view) fn pane_page(&self, delta: isize) {
        let rows = isize::from(
            i16::try_from(self.viewport.peek().1.saturating_sub(4).max(1)).unwrap_or(1),
        );
        self.pane_move(delta * rows);
    }

    pub(in crate::tui::sigil_view) fn pane_edge(&self, end: bool) {
        self.pane_move(if end { isize::MAX } else { -isize::MAX });
    }

    pub(in crate::tui::sigil_view) fn pane_enter(&self) {
        let pane = self.current();
        if let Some(enter) = spec(pane).enter {
            return enter(self);
        }
        if let Some(picked) = self.picked(pane) {
            (spec(pane).pick)(self, &picked);
        }
    }

    pub(in crate::tui::sigil_view) fn pane_expand(&self) {
        spec(self.current()).expand.unwrap_or(Self::pane_enter)(self);
    }

    pub(in crate::tui::sigil_view) fn pane_collapse(&self) {
        let pane = self.current();
        match spec(pane).collapse {
            Some(collapse) => collapse(self),
            None => {
                self.pane_list(pane).collapse_all();
                self.refresh();
            }
        }
    }

    pub(in crate::tui::sigil_view) fn pane_click(&self, pane: Pane, index: usize) {
        self.pane_list(pane).select(index);
        let Some(picked) = self.pane_rows(pane).into_iter().nth(index) else {
            return;
        };
        spec(pane).click.unwrap_or(spec(pane).pick)(self, &picked);
    }

    pub(in crate::tui::sigil_view) fn pane_menu(
        &self,
        pane: Pane,
        picked: &PaneRow,
    ) -> &'static [(&'static str, Act)] {
        (spec(pane).menu)(picked)
    }

    pub(in crate::tui::sigil_view) fn open_row(&self, picked: &PaneRow) {
        self.pane_list(self.current()).toggle_only(&picked.path);
        self.refresh();
    }

    fn file_pick(&self, picked: &PaneRow) {
        if picked.dir {
            self.pane_list(Pane::Files).toggle(&picked.path);
        } else {
            self.mention(&picked.path);
        }
        self.refresh();
    }

    fn file_expand(&self) {
        match self.picked(Pane::Files) {
            Some(picked) if picked.dir && !picked.open => self.file_pick(&picked),
            Some(picked) if !picked.dir => self.view_open(picked.path),
            Some(_) | None => {}
        }
    }

    fn file_collapse(&self) {
        let rows = self.file_rows();
        let list = self.pane_list(Pane::Files);
        let Some(picked) = rows.get(list.at(rows.len())) else {
            return;
        };
        if picked.dir && picked.open {
            return self.file_pick(picked);
        }
        if let Some((parent, _)) = picked.path.rsplit_once('/')
            && let Some(index) = rows.iter().position(|candidate| candidate.path == parent)
        {
            list.select(index);
            self.refresh();
        }
    }
}

#[cfg(test)]
#[path = "panes_tests.rs"]
mod tests;
