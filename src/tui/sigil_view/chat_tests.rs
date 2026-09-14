use super::*;

#[test]
fn wide_diagrams_keep_readable_source_when_the_terminal_is_narrow() {
    sigil::reactive::Runtime::scope("diagram-width", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        let source = "graph LR\nA[Read every input record and preserve its complete label] --> B[Show the result]";
        app.model.borrow_mut().entries.push(Entry::Assistant {
            text: format!("before\n```mermaid\n{source}\n```\nafter"),
            done: true,
        });
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(42, 30);
        for width in [42, 140, 42] {
            app.resize(width, 30);
            app.refresh();
            let frame = screen
                .frame(&mut host, &mut router, [], (width, 30))
                .expect("diagram");
            let text = frame.text();
            let body = text.lines().skip(1).take(24).collect::<Vec<_>>().join("\n");
            assert!(body.contains("before") && body.contains("after"), "{body}");
            if width == 42 {
                assert!(body.contains("widen to view diagram"), "{body}");
                assert!(!body.contains('┌'), "broken diagram fragments: {body}");
                assert!(
                    body.split_whitespace()
                        .collect::<String>()
                        .contains(&source.split_whitespace().collect::<String>()),
                    "source was lost: {body}"
                );
            } else {
                assert!(body.contains("│ Read every input record and preserve its complete label │────▶│ Show the result │"), "{body}");
                assert!(!body.contains("widen to view diagram"), "{body}");
            }
        }
    });
}

#[test]
fn transcript_backgrounds_fill_wrapped_rows_without_filling_prose() {
    for width in [24, 67, 120] {
        sigil::reactive::Runtime::scope("transcript-background", || {
            let (cmd, _rx) = unbounded_channel();
            let app = App::new(info(), PathBuf::from("."), cmd);
            app.model.borrow_mut().entries.extend([
                Entry::User("Explain the café 日本語 example and preserve every word of this long prompt across wrapped rows.".to_string()),
                Entry::Assistant {
                    text: "before\nA normal paragraph about the café 日本語 example stays on the plain transcript background across multiple rows.\n`inline code only`\n```python\ndef describe(record):\n    return f\"Name: {record['name']} | Label: {record['label']} | Details: {record['details']}\"\n\n```\nafter\n```mermaid\ngraph LR\nA[Read every input record and preserve its complete label] --> B[Show the result]\n```\nend".to_string(),
                    done: true,
                },
                Entry::Assistant {
                    text: "```rust\nlet x = 1;".to_string(),
                    done: false,
                },
            ]);
            app.resize(width, 80);
            app.refresh();
            let tree = app.clone();
            let mut host = Host::cells();
            host.mount(move |cx: &mut Cx| cx.add(root(tree)));
            let mut router = EventRouter::new();
            let mut screen = Screen::new(width, 80);
            let frame = screen
                .frame(&mut host, &mut router, [], (width, 80))
                .expect("transcript");
            let text = frame.text();
            let rows: Vec<_> = text.lines().collect();
            let at = |needle: &str| {
                rows.iter()
                    .position(|row| row.trim() == needle)
                    .expect(needle)
            };
            let margin = chat::margin(usize::from(width)) as u16;
            let code = chat::rgb(30, 32, 42);
            let base = chat::rgb(12, 12, 20);
            for (start, end, color) in [
                (2, at("before") - 1, chat::rgb(42, 44, 58)),
                (at("before"), at("inline code only") + 1, base),
                (at("inline code only") + 1, at("after"), code),
                (at("after") + 1, at("end"), code),
                (at("end") + 2, at("let x = 1;▌") + 1, code),
            ] {
                for y in start..end {
                    for x in margin..width - margin {
                        assert_eq!(
                            frame.get(usize::from(x), y).expect("background cell").bg,
                            Ink::Exact(color),
                            "width {width}, row {y}, column {x}: {}",
                            rows[y]
                        );
                    }
                }
            }
            for y in [at("before"), at("inline code only"), at("after"), at("end")] {
                assert_eq!(
                    frame
                        .get(usize::from(width - margin - 1), y)
                        .expect("prose cell")
                        .bg,
                    Ink::Exact(base)
                );
            }
        });
    }
}

#[test]
fn transcript_wraps_words_across_style_boundaries() {
    sigil::reactive::Runtime::scope("transcript-word-wrap", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.model.borrow_mut().entries.push(Entry::Assistant {
            text: "Read extra**ordi**nary words across styled spans\n\n0123456789012345678901234567890123456789".to_string(),
            done: true,
        });
        app.resize(24, 20);
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(24, 20);
        let frame = screen
            .frame(&mut host, &mut router, [], (24, 20))
            .expect("wrapped transcript");
        let text = frame.text();
        let rows: Vec<_> = text.lines().skip(2).take(6).map(str::trim).collect();
        assert_eq!(
            rows,
            [
                "Read extraordinary",
                "words across styled",
                "spans",
                "",
                "0123456789012345678901",
                "234567890123456789"
            ]
        );
        for column in 11..15 {
            assert!(frame.get(column, 2).expect("styled letter").bold);
        }
        assert!(!frame.get(1, 2).expect("plain letter").bold);
    });
}

#[test]
fn headless_frame_is_painted_by_sigil() {
    sigil::reactive::Runtime::scope("mage-sigil-view-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.model.borrow_mut().entries.push(Entry::Assistant {
            text: "Hello from Mage".to_string(),
            done: true,
        });
        app.resize(100, 32);
        app.refresh();
        let tree = app.clone();
        let Ok(frame) = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
        else {
            panic!("layout failed");
        };
        if std::env::var_os("MAGE_DUMP_FRAME").is_some() {
            println!("{frame}");
        }
        assert!(
            frame.contains("summoner glm-5.3"),
            "header missing:\n{frame}"
        );
        assert!(
            frame.contains("Hello from Mage"),
            "transcript missing:\n{frame}"
        );
        assert!(
            frame.contains("enter send · ctrl+o options"),
            "Sigil composer missing:\n{frame}"
        );
        assert!(
            !frame.contains("ctrl+j newline"),
            "footer is noisy:\n{frame}"
        );
        assert!(!frame.contains("ctrl+e files"), "footer is noisy:\n{frame}");
    });
}

#[test]
fn chat_composer_keeps_the_original_three_row_height() {
    sigil::reactive::Runtime::scope("mage-chat-composer-height-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        app.model.borrow_mut().entries.push(Entry::Assistant {
            text: "Hello from Mage".to_string(),
            done: true,
        });
        app.resize(100, 32);
        app.refresh();
        let tree = app.clone();
        let frame = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 100, 32, &[])
            .unwrap_or_else(|error| panic!("layout failed: {error}"));
        let lines: Vec<&str> = frame.lines().collect();
        let row = |needle: &str| {
            lines
                .iter()
                .position(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("`{needle}` missing:\n{frame}"))
        };

        assert_eq!(
            row("~/src/mage") - row("╭ ● auto"),
            3,
            "composer expanded beyond the original three-row contract:\n{frame}"
        );
    });
}

#[test]
fn numbered_lists_keep_their_numbers() {
    use crate::tui::sigil_view::chat::transcript::numbered;
    assert_eq!(numbered("1. first"), Some(("1", "first")));
    assert_eq!(numbered("12. twelfth"), Some(("12", "twelfth")));
    assert_eq!(numbered("1234. not a list"), None);
    assert_eq!(numbered("1.no space"), None);
    assert_eq!(numbered("plain"), None);
}

#[test]
fn streaming_deltas_rerender_only_the_growing_tail() {
    sigil::reactive::Runtime::scope("streaming-tail", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        {
            let mut model = app.model.borrow_mut();
            for round in 0..4 {
                model.entries.push(Entry::Assistant {
                    text: format!("entry {round} carries a whole sentence of body text"),
                    done: true,
                });
            }
        }
        let stamp = app.model.borrow().entries.stamp;
        let width = 60;
        let mut history = app.history.borrow_mut();
        let tail_rows = history.lines(&app.model.borrow(), width, stamp).len();
        assert_eq!(history.rendered, 4, "initial render covers every entry");
        assert_eq!(history.last_entry(), Some(3));
        if let Some(Entry::Assistant { text, .. }) = app.model.borrow_mut().entries.last_mut() {
            text.push_str(" plus freshly streamed words");
        }
        let stamp = app.model.borrow().entries.stamp;
        let streamed = history.lines(&app.model.borrow(), width, stamp).len();
        assert_eq!(
            history.rendered, 5,
            "a delta must re-render only the tail entry"
        );
        assert!(streamed > tail_rows, "streamed rows were appended");
        assert_eq!(history.last_entry(), Some(3));
        app.model
            .borrow_mut()
            .entries
            .push(Entry::User("next question".into()));
        let stamp = app.model.borrow().entries.stamp;
        history.lines(&app.model.borrow(), width, stamp);
        assert_eq!(
            history.rendered, 7,
            "a push re-renders the sealed tail and the new entry"
        );
        assert_eq!(history.last_entry(), Some(4));
        history.lines(&app.model.borrow(), 40, stamp);
        assert_eq!(history.rendered, 12, "a width change re-renders everything");
    });
}
#[test]
fn incremental_stream_matches_a_full_render() {
    use crate::tui::sigil_view::chat::History;

    sigil::reactive::Runtime::scope("stream-incremental", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.model.borrow_mut().entries.push(Entry::Assistant {
            text: String::new(),
            done: false,
        });
        let width = 48;
        let chunks = [
            "## Head\n",
            "A paragraph with **bold**, `code`, and café 日本語 text.\n",
            "- first\n",
            "- second item that is long enough to wrap across the width\n",
            "\n",
            "```rust\n",
            "pub fn main() { let count = 1; }\n",
            "```\n",
            "trailing partial line",
        ];
        let incremental;
        {
            let mut history = app.history.borrow_mut();
            for chunk in chunks {
                match app.model.borrow_mut().entries.last_mut() {
                    Some(Entry::Assistant { text, .. }) => text.push_str(chunk),
                    _ => unreachable!(),
                }
                let stamp = app.model.borrow().entries.stamp;
                history.lines(&app.model.borrow(), width, stamp);
            }
            incremental = history.fingerprint();
        }
        let mut full = History::default();
        let stamp = app.model.borrow().entries.stamp;
        full.lines(&app.model.borrow(), width, stamp);
        assert_eq!(incremental, full.fingerprint());
    });
}

#[test]
fn approval_bar_shows_run_and_deny_buttons() {
    sigil::reactive::Runtime::scope("approval-bar", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 24);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.approval
            .set(Some(("git push origin main".to_string(), false)));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        let text = screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("approval frame")
            .text();
        assert!(text.contains("git push origin main"), "{text}");
        assert!(text.contains("[ allow ]"), "{text}");
        assert!(!text.contains("[ always ]"), "{text}");
        assert!(text.contains("[ deny ]"), "{text}");
        assert!(text.contains("y / n"), "{text}");
    });
}

#[test]
fn clicking_run_answers_the_approval() {
    sigil::reactive::Runtime::scope("approval-click", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(80, 24);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.approval
            .set(Some(("git push origin main".to_string(), false)));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(80, 24);
        screen
            .frame(&mut host, &mut router, [], (80, 24))
            .expect("approval frame");
        let margin = chat::margin(80);
        let row_layout = chat::approval_row("git push origin main", 80 - margin * 2, false);
        let row = chat::prompt_row(80, 24, "") - 1;
        let col = (margin + row_layout.allow.0 + 1) as u16;
        screen
            .frame(&mut host, &mut router, sigil_tui::click(col, row), (80, 24))
            .expect("run click");
        assert!(
            matches!(rx.try_recv(), Ok(crate::tui::Cmd::Steer(_))),
            "the run button answers the approval"
        );
    });
}

#[test]
fn approval_bar_shows_always_when_the_grant_can_persist() {
    sigil::reactive::Runtime::scope("approval-always", || {
        let (cmd, mut rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("."), cmd);
        app.resize(100, 24);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.approval.set(Some(("~/.cargo".to_string(), true)));
        app.refresh();
        let tree = app.clone();
        let mut host = Host::cells();
        host.mount(move |cx: &mut Cx| cx.add(root(tree)));
        let mut router = EventRouter::new();
        let mut screen = Screen::new(100, 24);
        let text = screen
            .frame(&mut host, &mut router, [], (100, 24))
            .expect("approval frame")
            .text();
        assert!(text.contains("[ always ]"), "{text}");
        assert!(text.contains("y / Y / n"), "{text}");
        let margin = chat::margin(100);
        let row_layout = chat::approval_row("~/.cargo", 100 - margin * 2, true);
        let always = row_layout.always.expect("always button");
        let row = chat::prompt_row(100, 24, "") - 1;
        let col = (margin + always.0 + 1) as u16;
        screen
            .frame(
                &mut host,
                &mut router,
                sigil_tui::click(col, row),
                (100, 24),
            )
            .expect("always click");
        match rx.try_recv() {
            Ok(crate::tui::Cmd::Steer(input)) => assert_eq!(input.text, "always"),
            Ok(_) => panic!("always click should steer always"),
            Err(_) => panic!("always click sent no command"),
        }
    });
}

#[test]
fn fenced_code_colours_keywords_and_leaves_names_plain() {
    use crate::tui::sigil_view::chat::transcript::code;
    let line = code("pub fn main() { let count = 1; }");
    let parts: Vec<(&str, _)> = line.iter().map(|span| span.parts()).collect();
    let colour = |word: &str| {
        parts
            .iter()
            .find(|(text, _)| *text == word)
            .map(|(_, fg)| *fg)
            .unwrap_or_else(|| panic!("no span for {word}: {parts:?}"))
    };
    assert_eq!(colour("pub"), colour("fn"));
    assert_eq!(colour("let"), colour("fn"));
    assert_ne!(colour("main"), colour("fn"));
    assert_eq!(colour("main"), colour("count"));
    let quoted = code("let name = \"fn\";");
    let parts: Vec<(&str, _)> = quoted.iter().map(|span| span.parts()).collect();
    assert!(parts.iter().any(|(text, _)| *text == "\"fn\""), "{parts:?}");
}
