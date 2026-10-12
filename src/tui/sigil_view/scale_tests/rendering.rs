use super::*;

fn reply(body: &str, cols: u16) -> sigil::terminal::Grid {
    paint(busy_info(), cols, 60, State::Idle, |app| {
        app.update(Msg::Event(Event::Assistant {
            text: body.to_string(),
        }));
    })
}

#[test]
fn supported_mermaid_diagrams_render_in_actual_replies() {
    for (source, labels) in [
        ("graph TD\nA[Prompt] --> B[Reply]", ["Prompt", "Reply"]),
        (
            "sequenceDiagram\nparticipant Client\nparticipant Server\nClient->>Server: request\nServer-->>Client: response",
            ["Client", "Server"],
        ),
        (
            "stateDiagram-v2\n[*] --> Working\nWorking --> Done\nDone --> [*]",
            ["Working", "Done"],
        ),
        ("pie\n\"Passed\" : 8\n\"Failed\" : 2", ["Passed", "Failed"]),
    ] {
        let body = format!("```mermaid\n{source}\n```");
        let shown = reply(&body, 110).text();
        for label in labels {
            assert!(shown.contains(label), "{source}:\n{shown}");
        }
        let directive = source.lines().next().expect("diagram directive");
        assert!(
            !shown.contains("```") && !shown.contains(directive),
            "{shown}"
        );
        assert!(
            shown.contains('─') || shown.contains('█'),
            "diagram has no marks:\n{shown}"
        );
    }
}

#[test]
fn supported_chart_fences_render_bars_and_preserve_labels() {
    for body in [
        "```chart\nResults\nPassed: 8\nFailed: 2\n```",
        "```xychart\ntitle \"Results\"\nx-axis [Passed, Failed]\nbar [8, 2]\n```",
        "```xychart-beta\ntitle \"Results\"\nx-axis [Passed, Failed]\nbar [8, 2]\n```",
    ] {
        for cols in [60, 110] {
            let grid = reply(body, cols);
            let shown = grid.text();
            assert!(shown.contains("Results"), "{cols}:\n{shown}");
            assert!(
                shown.contains("Passed") && shown.contains("Failed"),
                "{cols}:\n{shown}"
            );
            assert!(
                !shown.contains("bar [8, 2]") && !shown.contains("Passed: 8"),
                "raw chart source:\n{shown}"
            );
            let labels = (0..grid.rows())
                .find(|&row| {
                    grid.row(row).iter().any(|cell| cell.text == "P")
                        && grid.row(row).iter().any(|cell| cell.text == "F")
                })
                .expect("chart labels row");
            let row = grid.row(labels);
            let passed = row
                .iter()
                .position(|cell| cell.text == "P")
                .expect("Passed")
                + 3;
            let failed = row
                .iter()
                .position(|cell| cell.text == "F")
                .expect("Failed")
                + 3;
            let plot = grid.row(labels.checked_sub(7).expect("plot above labels"));
            assert_ne!(
                plot[passed].bg, plot[failed].bg,
                "8 and 2 must paint unequal bars:\n{shown}"
            );
        }
    }
}

#[test]
fn unknown_diagrams_and_invalid_charts_keep_their_source() {
    for body in [
        "```mermaid\nunsupportedDiagram\nReadable fallback\n```",
        "```chart\nReadable fallback\nnot: a-number\n```",
    ] {
        let shown = reply(body, 110).text();
        assert!(shown.contains("Readable fallback"), "{shown}");
        assert!(
            shown.contains('┌') && shown.contains('└'),
            "no fallback frame:\n{shown}"
        );
    }
}

#[test]
fn a_streamed_diagram_reaches_the_same_completed_reply_renderer() {
    let body = "```mermaid\ngraph LR\nA[Start] --> B[Finish]\n```";
    let shown = frame_with(busy_info(), 110, 60, State::Idle, |app| {
        app.update(Msg::Event(Event::Delta {
            text: body[..25].to_string(),
        }));
        app.update(Msg::Event(Event::Delta {
            text: body[25..].to_string(),
        }));
        app.update(Msg::Event(Event::Assistant {
            text: body.to_string(),
        }));
    })
    .join("\n");
    assert!(
        shown.contains("Start") && shown.contains("Finish"),
        "{shown}"
    );
    assert!(
        !shown.contains("graph LR") && !shown.contains("```"),
        "{shown}"
    );
}
