use std::path::Path;

use super::*;

#[test]
fn a_relative_shell_var_is_not_outside_the_workspace() {
    Policy::builtin()
        .command(
            "git cat-file -p 9caf0adc > crates/ide-core/$name",
            Path::new("/tmp"),
        )
        .unwrap();
}

#[test]
fn home_and_tmp_vars_stay_outside() {
    let policy = Policy::builtin();
    let error = policy
        .command("echo hi > $HOME/secret", Path::new("/tmp"))
        .unwrap_err();
    assert!(matches!(error, Error::Denied(_)), "{error}");
    let error = policy
        .command("chmod 777 ~/.ssh", Path::new("/tmp"))
        .unwrap_err();
    assert!(matches!(error, Error::Denied(_)), "{error}");
}
