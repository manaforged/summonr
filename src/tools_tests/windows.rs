use super::*;

#[test]
fn a_powershell_error_reaches_the_model_as_text() {
    let (_dir, tools) = workspace(&[]);
    let missing = outcome(&tools, "Get-Item missing-zz.txt");
    assert!(missing.starts_with("exit 1"), "{missing}");
    assert!(missing.contains("Cannot find path"), "{missing}");
    let thrown = outcome(&tools, "throw 'stopped here'");
    assert!(
        thrown.starts_with("exit 1") && thrown.contains("stopped here"),
        "{thrown}"
    );
    for text in [missing, thrown] {
        assert!(!text.contains("CLIXML"), "{text}");
    }
}

#[test]
fn commands_keep_the_powershell_module_path() {
    let parent = std::env::var("PSModulePath").expect("Windows sets PSModulePath");
    let kept = crate::tools::command::command_env();
    assert!(
        kept.iter()
            .any(|(key, value)| key == "PSModulePath" && *value == parent),
        "{kept:?}"
    );
}
