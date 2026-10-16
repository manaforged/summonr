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

#[test]
fn a_native_command_that_writes_stderr_keeps_its_own_exit_code() {
    let (_dir, tools) = workspace(&[]);
    let quiet = outcome(&tools, "cmd /c \"echo note 1>&2\"");
    assert!(quiet.starts_with("exit 0"), "{quiet}");
    assert!(quiet.contains("note"), "{quiet}");
    let blank = outcome(&tools, "cmd /c \"echo. 1>&2\"");
    assert!(!blank.contains("RemoteException"), "{blank}");
    let failed = outcome(&tools, "cmd /c \"echo boom 1>&2 & exit 3\"");
    assert!(failed.starts_with("exit 3"), "{failed}");
}
