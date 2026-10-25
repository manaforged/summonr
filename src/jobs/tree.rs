use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Descendant {
    pub pid: u32,
    pub command: String,
}

pub(crate) fn descendants(root: u32) -> Vec<Descendant> {
    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    let mut commands: BTreeMap<u32, String> = BTreeMap::new();
    for (pid, parent, command) in table() {
        if pid != parent {
            children.entry(parent).or_default().push(pid);
        }
        commands.insert(pid, command);
    }
    let mut found = Vec::new();
    let mut pending: Vec<u32> = children.get(&root).cloned().unwrap_or_default();
    pending.reverse();
    while let Some(pid) = pending.pop() {
        if let Some(command) = commands.remove(&pid) {
            found.push(Descendant { pid, command });
        }
        if let Some(next) = children.get(&pid) {
            pending.extend(next.iter().rev());
        }
    }
    found
}

pub(crate) fn child_lines(root: u32, workspace: &std::path::Path) -> Vec<String> {
    let limits = crate::limits::limits();
    let found = descendants(root);
    let mut lines: Vec<String> = found
        .iter()
        .take(limits.job_child_rows)
        .map(|child| {
            let line = format!("{} {}", child.pid, crate::text::squash(&child.command));
            crate::text::ellipsize(
                &crate::secret::shield::scrub_found(&line, workspace),
                limits.job_child_width,
            )
        })
        .collect();
    if found.len() > lines.len() {
        lines.push(format!("… {} more", found.len() - lines.len()));
    }
    lines
}

pub(crate) fn block(heading: &str, lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut text = format!("{heading}:\n");
    for line in lines {
        text.push_str("  ");
        text.push_str(line);
        text.push('\n');
    }
    text
}

#[cfg(unix)]
fn table() -> Vec<(u32, u32, String)> {
    let Ok(output) = std::process::Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, rest) = line.trim_start().split_once(char::is_whitespace)?;
            let rest = rest.trim_start();
            let (parent, command) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            Some((
                pid.parse().ok()?,
                parent.parse().ok()?,
                command.trim().to_string(),
            ))
        })
        .collect()
}

#[cfg(not(unix))]
fn table() -> Vec<(u32, u32, String)> {
    Vec::new()
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "std has no process info query")]
pub(super) fn bsd_info(pid: u32) -> Option<libc::proc_bsdinfo> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: proc_bsdinfo is plain integers and byte arrays; all-zero is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = libc::c_int::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
    // SAFETY: the buffer pointer and size describe one owned proc_bsdinfo for the call.
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            std::ptr::from_mut(&mut info).cast(),
            size,
        )
    };
    (written == size).then_some(info)
}
