use std::ffi::OsStr;
use std::io::IsTerminal;
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

use crate::process::{Deadline, Spec, output_within};

const MARKER: &str = "__SUMMONR_LOGIN_ENV__";
const SKIP: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];
const DEADLINE: Deadline = Deadline {
    after: Duration::from_secs(10),
    poll: Duration::from_millis(50),
};

#[expect(
    unsafe_code,
    reason = "std::env::set_var is unsafe, and an app launched from Finder has no other way to give its children the login environment"
)]
pub(super) fn adopt() {
    if std::io::stdin().is_terminal() || std::io::stdout().is_terminal() {
        return;
    }
    for (name, value) in read().unwrap_or_default() {
        // SAFETY: start calls adopt before it builds the runtime; read has joined its threads, so this is the only thread in the process.
        unsafe { std::env::set_var(OsStr::from_bytes(&name), OsStr::from_bytes(&value)) };
    }
}

fn read() -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/zsh".into());
    let script = format!("printf {MARKER}; /usr/bin/env -0; printf {MARKER}");
    let spec = Spec::new(shell, ["-ilc", script.as_str()]);
    let output = output_within(spec, None, DEADLINE).ok()??;
    output.status.success().then(|| parse(&output.stdout))?
}

fn parse(stdout: &[u8]) -> Option<Vec<(Vec<u8>, Vec<u8>)>> {
    let marker = MARKER.as_bytes();
    let start = stdout.windows(marker.len()).position(|part| part == marker)? + marker.len();
    let rest = &stdout[start..];
    let end = rest.windows(marker.len()).position(|part| part == marker)?;
    Some(
        rest[..end]
            .split(|byte| *byte == 0)
            .filter_map(|entry| {
                let equals = entry.iter().position(|byte| *byte == b'=')?;
                let (name, value) = (&entry[..equals], &entry[equals + 1..]);
                let skipped = name.is_empty() || SKIP.iter().any(|skip| skip.as_bytes() == name);
                (!skipped).then(|| (name.to_vec(), value.to_vec()))
            })
            .collect(),
    )
}
