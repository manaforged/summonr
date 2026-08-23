use super::*;

#[test]
fn picker_wraps_and_accepts_the_selected_row() {
    sigil::reactive::Runtime::scope("mage-command-picker-test", || {
        let picker = Picker::new();
        assert!(picker.open("/"));
        assert!(picker.shift("/", -1));
        assert_eq!(picker.accept("/"), Some("/quit"));
        assert!(picker.shift("/", 1));
        assert_eq!(picker.accept("/"), Some("/help"));
    });
}

#[test]
fn picker_dismissal_resets_after_an_edit() {
    sigil::reactive::Runtime::scope("mage-command-dismiss-test", || {
        let picker = Picker::new();
        assert!(picker.hide("/mo"));
        assert!(!picker.open("/mo"));
        picker.change("/mo");
        assert!(picker.open("/mo"));
        assert_eq!(picker.accept("/mo"), Some("/model"));
        assert_eq!(picker.accept("/model"), None);
    });
}

#[test]
fn popup_rows_stay_at_the_requested_cell_width() {
    let line = row("/model", "choose the active model", 42, true);
    assert_eq!(UnicodeWidthStr::width(line.as_str()), 42);
    assert_eq!(
        UnicodeWidthStr::width(border("commands", 42, true).as_str()),
        42
    );
}
