use super::*;

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
            frame.contains("enter send"),
            "Sigil composer missing:\n{frame}"
        );
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
