use super::*;

#[test]
fn the_environment_joins_the_bus_when_the_module_is_off() {
    assert!(bus_enabled(Some(OsStr::new("on")), false));
    assert!(!bus_enabled(Some(OsStr::new("off")), true));
    assert!(bus_enabled(None, true));
    assert!(!bus_enabled(None, false));
}
