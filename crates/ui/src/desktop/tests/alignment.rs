use std::time::{Duration, SystemTime};

use sigil::Host;
use sigil::app::FrameHooks;
use sigil::prelude::HostMode;
use sigil::scene::SceneCommand;
use tokio::sync::mpsc::unbounded_channel;

use super::super::sidebar::WIDTH;
use super::super::style::{GUTTER, ICON};
use super::super::{App, root};
use super::{VIEW, in_window};
use summonr_agent::hub::{Activity, Fact, Intent, Msg};
use summonr_agent::session::SessionInfo;
use summonr_agent::tools::Event;

const TOLERANCE: f32 = 0.5;
const LABELS: [&str; 5] = ["New agent", "Search", "summonr", "thomas", "Settings"];
const AGENTS: [&str; 3] = ["Respond to a greeting", "Explain the summonr layout", "Hi"];
const TRAILING: [&str; 6] = ["⌘N", "⌘K", "14h", "4h", "now", "⌘,"];

/// One drawn mark or control, with its window edges.
#[derive(Clone, Debug)]
struct Mark {
    kind: Kind,
    text: String,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Fill,
    Control,
}

impl Mark {
    fn mid(&self) -> f32 {
        (self.y0 + self.y1) / 2.0
    }

    fn holds(&self, inner: &Self) -> bool {
        self.x0 <= inner.x0 && self.x1 >= inner.x1 && self.y0 <= inner.y0 && self.y1 >= inner.y1
    }

    fn icon(&self) -> bool {
        self.kind == Kind::Fill
            && self.x1 - self.x0 <= ICON + TOLERANCE
            && self.y1 - self.y0 <= ICON + TOLERANCE
    }
}

fn agent(id: &str, cwd: &str, preview: &str, age: Duration) -> (SessionInfo, bool) {
    (
        SessionInfo {
            id: id.to_owned(),
            path: std::path::PathBuf::new(),
            cwd: Some(cwd.to_owned()),
            model: Some("m".to_owned()),
            preview: preview.to_owned(),
            name: None,
            parent: None,
            depth: 0,
            interactive: true,
            modified: SystemTime::now().checked_sub(age),
        },
        true,
    )
}

fn working_reply(app: &App) {
    app.window.sidebar.set(true);
    app.update(Msg::Fact(Fact::Sessions(Ok(vec![
        agent(
            "01a1000000000000000000000000aaaa",
            "/work/summonr",
            AGENTS[1],
            Duration::from_secs(4 * 3_600),
        ),
        agent(
            "01a1000000000000000000000000bbbb",
            "/work/summonr",
            AGENTS[0],
            Duration::from_secs(14 * 3_600),
        ),
        agent(
            "01a1000000000000000000000000cccc",
            "/work/thomas",
            AGENTS[2],
            Duration::from_secs(5),
        ),
    ]))));
    app.update(Msg::Intent(Intent::User("Hi".to_owned())));
    app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
    app.update(Msg::Fact(Fact::Event(Event::Assistant {
        text: "Hi, Thomas.".to_owned(),
    })));
}

fn marks(setup: impl FnOnce(&App)) -> Vec<Mark> {
    sigil::reactive::Owner::scope("summonr-alignment", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            crate::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        super::fit(&app);
        setup(&app);
        app.refresh();
        let mut host = Host::new(HostMode::Pixel);
        let tree = app.clone();
        host.mount(move || root(tree.clone()));
        for _ in 0..4 {
            host.frame(VIEW, 1.0, FrameHooks::default());
        }
        let mut offset = vec![(0.0_f32, 0.0_f32)];
        let mut marks = Vec::new();
        for command in host.scene(VIEW).expect("scene").commands {
            let (dx, dy) = *offset.last().expect("offset");
            let mut push = |kind, text: String, r: sigil::geometry::Rect| {
                marks.push(Mark {
                    kind,
                    text,
                    x0: r.origin.x + dx,
                    y0: r.origin.y + dy,
                    x1: r.origin.x + r.width + dx,
                    y1: r.origin.y + r.height + dy,
                });
            };
            match command {
                SceneCommand::PushTransform(transform) => {
                    offset.push((dx + transform[0][2], dy + transform[1][2]))
                }
                SceneCommand::PopTransform => drop(offset.pop()),
                SceneCommand::GlyphRun(run) => push(Kind::Text, run.text.clone(), run.rect),
                SceneCommand::Sprite(sprite) => push(Kind::Fill, String::new(), sprite.destination),
                SceneCommand::Rect(rect) => push(Kind::Fill, String::new(), rect.rect),
                _ => {}
            }
        }
        for (_, node) in &host.accessibility_tree().nodes {
            if let (Some(label), Some(bounds)) = (node.label(), node.bounds()) {
                marks.push(Mark {
                    kind: Kind::Control,
                    text: label.to_string(),
                    x0: bounds.x0 as f32,
                    y0: bounds.y0 as f32,
                    x1: bounds.x1 as f32,
                    y1: bounds.y1 as f32,
                });
            }
        }
        marks
    })
}

fn find<'a>(marks: &'a [Mark], kind: Kind, wanted: impl Fn(&Mark) -> bool) -> &'a Mark {
    marks
        .iter()
        .find(|mark| mark.kind == kind && wanted(mark))
        .unwrap_or_else(|| panic!("no {kind:?} mark matches in {}", table(marks)))
}

fn text<'a>(marks: &'a [Mark], content: &str, sidebar: bool) -> &'a Mark {
    find(marks, Kind::Text, |mark| {
        mark.text == content && (mark.x0 < WIDTH) == sidebar
    })
}

/// The icon drawn on the same line as `label`, just before it.
fn icon_before<'a>(marks: &'a [Mark], label: &Mark) -> &'a Mark {
    marks
        .iter()
        .filter(|mark| mark.icon() && mark.x1 <= label.x0 && (mark.mid() - label.mid()).abs() < 3.0)
        .max_by(|a, b| a.x1.total_cmp(&b.x1))
        .unwrap_or_else(|| panic!("no icon before {:?} in {}", label.text, table(marks)))
}

fn table(marks: &[Mark]) -> String {
    marks
        .iter()
        .map(|mark| {
            format!(
                "\n{:?} {:.1} {:.1} {:.1} {:.1} {:?}",
                mark.kind, mark.x0, mark.y0, mark.x1, mark.y1, mark.text
            )
        })
        .collect()
}

fn same(what: &str, edges: &[(&str, f32)], at: Option<f32>) {
    let want = at.unwrap_or(edges[0].1);
    let off: Vec<_> = edges
        .iter()
        .filter(|(_, edge)| (edge - want).abs() > TOLERANCE)
        .collect();
    assert!(
        off.is_empty(),
        "{what}: want {want:.1}, off {off:?}, all {edges:?}"
    );
}

#[test]
fn sidebar_rows_share_one_icon_label_and_trailing_column() {
    if !in_window("alignment::sidebar_rows_share_one_icon_label_and_trailing_column") {
        return;
    }
    let marks = marks(working_reply);
    let labels: Vec<_> = LABELS.iter().map(|name| text(&marks, name, true)).collect();
    same(
        "labels start",
        &labels
            .iter()
            .map(|m| (m.text.as_str(), m.x0))
            .collect::<Vec<_>>(),
        None,
    );
    let icons: Vec<_> = labels
        .iter()
        .map(|label| (label.text.as_str(), icon_before(&marks, label).x0))
        .collect();
    same("icons start", &icons, None);
    let column = labels[0].x0;
    let agents: Vec<_> = AGENTS
        .iter()
        .map(|name| (*name, text(&marks, name, true).x0))
        .collect();
    same(
        "agent titles start in the label column",
        &agents,
        Some(column),
    );
    let trailing: Vec<_> = TRAILING
        .iter()
        .map(|key| (*key, text(&marks, key, true).x1))
        .collect();
    same("trailing meta ends", &trailing, None);
}

#[test]
fn the_reading_column_starts_and_ends_every_mark_at_the_gutter() {
    if !in_window("alignment::the_reading_column_starts_and_ends_every_mark_at_the_gutter") {
        return;
    }
    let marks = marks(working_reply);
    let placeholder = text(&marks, "Message", false);
    let frame = marks
        .iter()
        .filter(|mark| {
            mark.kind == Kind::Fill
                && mark.holds(placeholder)
                && mark.x0 >= WIDTH
                && mark.x1 - mark.x0 > 300.0
        })
        .min_by(|a, b| (a.x1 - a.x0).total_cmp(&(b.x1 - b.x0)))
        .unwrap_or_else(|| panic!("no composer frame in {}", table(&marks)));
    let attach = find(&marks, Kind::Control, |mark| mark.text == "Attach a file");
    let attach_ink = find(&marks, Kind::Fill, |mark| mark.icon() && attach.holds(mark));
    let elapsed = find(&marks, Kind::Text, |mark| {
        mark.x0 >= WIDTH
            && mark.text.ends_with('s')
            && mark.text[..mark.text.len() - 1]
                .chars()
                .all(|c| c.is_ascii_digit())
    });
    let spinner = find(&marks, Kind::Fill, |mark| {
        (mark.mid() - elapsed.mid()).abs() < 3.0
            && mark.x0 >= frame.x0
            && mark.x1 < (frame.x0 + frame.x1) / 2.0
    });
    let starts = [
        ("reply", text(&marks, "Hi, Thomas.", false).x0),
        ("placeholder", placeholder.x0),
        ("attach icon", attach_ink.x0),
        ("spinner", spinner.x0),
    ];
    same("column start", &starts, Some(frame.x0 + GUTTER));
    let own = text(&marks, "Hi", false);
    let bubble = marks
        .iter()
        .filter(|mark| mark.kind == Kind::Fill && mark.holds(own) && mark.x1 - mark.x0 < 300.0)
        .min_by(|a, b| (a.x1 - a.x0).total_cmp(&(b.x1 - b.x0)))
        .unwrap_or_else(|| panic!("no bubble in {}", table(&marks)));
    let stop = find(&marks, Kind::Control, |mark| mark.text == "Stop");
    let ends = [
        ("bubble", bubble.x1),
        ("elapsed", elapsed.x1),
        ("stop", stop.x1),
    ];
    same("column end", &ends, Some(frame.x1 - GUTTER));
}
