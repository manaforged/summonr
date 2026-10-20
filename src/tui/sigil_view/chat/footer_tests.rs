use super::*;
use crate::tui::sigil_view::app::{Notice, NoticeKind};
use crate::tui::sigil_view::profile::info;
use sigil::reactive::Runtime;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn a_notice_takes_its_width_and_a_gap_from_the_workspace_path() {
    Runtime::scope("footer-place-notice", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = unbounded_channel();
        let mut details = info();
        details.workspace = "w".repeat(200);
        let app = App::new(details, workspace.path().to_owned(), commands);
        assert!(app.git_branch.is_none());
        let notice = Notice {
            text: "Copied".to_owned(),
            kind: NoticeKind::Copied,
        };
        let room = 60 - (cell_width(&notice.text) + 2);
        let view: FooterView = (Some(notice), None, None, None, None, None);
        let place = place_of(&app, &view, 60);
        assert_eq!(place, crate::text::ellipsize(&app.info.workspace, room));
        assert_eq!(cell_width(&place), room);
    });
}
