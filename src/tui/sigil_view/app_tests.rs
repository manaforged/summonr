use super::*;

#[test]
fn task_progress_uses_one_live_projection() {
    sigil::reactive::Runtime::scope("mage-task-projection-test", || {
        let (cmd, _rx) = unbounded_channel();
        let app = App::new(info(), PathBuf::from("/tmp/mage-view-test"), cmd);
        let (task, _) = crate::task::TaskState::transition(
            None,
            r#"{"action":"start","steps":["Inspect", "Verify"]}"#,
        )
        .unwrap_or_else(|error| panic!("{error}"));
        apply(
            &app,
            Msg::Event(Event::ToolResult {
                id: "task".to_string(),
                success: true,
                mutated: false,
                task: Some(task),
                output: "started".to_string(),
            }),
        );
        assert!(app.task.peek().contains("[>] 1. Inspect"));
        assert_eq!(app.model.borrow().entries.len(), 0);
        let tree = app.clone();
        let Ok(frame) = sigil_tui::present(move |cx: &mut Cx| cx.add(root(tree)), 80, 24, &[])
        else {
            panic!("layout failed");
        };
        assert!(frame.contains("[>] 1. Inspect"), "task missing:\n{frame}");
        assert!(frame.contains("[ ] 2. Verify"), "task missing:\n{frame}");
    });
}
