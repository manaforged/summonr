use super::*;

#[test]
fn picker_wraps_and_accepts_the_selected_row() {
    sigil::reactive::Runtime::scope("mage-command-picker-test", || {
        let picker = Picker::new();
        let model = Model::new(&super::super::tests::info());
        assert!(picker.open("/", 0, &model));
        assert!(picker.shift("/", 0, &model, -1));
        assert_eq!(picker.accept("/", 0, &model).as_deref(), Some("/exit"));
        assert!(picker.shift("/", 0, &model, 1));
        assert_eq!(picker.accept("/", 0, &model).as_deref(), Some("/help"));
    });
}

#[test]
fn picker_dismissal_resets_after_an_edit() {
    sigil::reactive::Runtime::scope("mage-command-dismiss-test", || {
        let picker = Picker::new();
        let model = Model::new(&super::super::tests::info());
        assert!(picker.hide("/mo", 0, &model));
        assert!(!picker.open("/mo", 0, &model));
        picker.change("/mo");
        assert!(picker.open("/mo", 0, &model));
        assert_eq!(picker.accept("/mo", 0, &model).as_deref(), Some("/model"));
        assert_eq!(picker.accept("/model", 0, &model).as_deref(), None);
    });
}

#[test]
fn popup_rows_stay_at_the_requested_cell_width() {
    let line = row("/model", "choose the active model", 42, true);
    assert_eq!(cell_width(line.as_str()), 42);
    assert_eq!(cell_width(border("commands", 42, true).as_str()), 42);
    let clipped = row("/view", "Read 日本語 files and café notes", 24, true);
    assert_eq!(cell_width(clipped.as_str()), 24);
    assert!(clipped.ends_with("…│"));
}

#[test]
fn arguments_close_the_picker_so_enter_submits() {
    use crate::tui::matching_commands;
    assert!(matching_commands("/steps PLAN.md").is_empty());
    assert!(matching_commands("/goal fix the plan").is_empty());
    assert!(matching_commands("/effort low").is_empty());
    assert_eq!(
        matching_commands("/plan acc")
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        ["/plan accept"]
    );
    assert!(
        matching_commands("/stp")
            .iter()
            .any(|row| row.0 == "/steps"),
        "single-token fuzzy matching stays"
    );
}
