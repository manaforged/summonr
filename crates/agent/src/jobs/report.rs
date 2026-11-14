use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::{ENDED, JobState, KILLED, Record, Status, alive_since, exit_path, log_path, tree};
use summonr_core::error::Result;

type Table = std::cell::OnceCell<Vec<(u32, u32, String)>>;

pub(crate) fn status(workspace: &Path, record: &Record) -> Result<Status> {
    report(workspace, record, Some(&std::cell::OnceCell::new()))
}

/// Returns every record's status, reading the process table at most once.
pub(crate) fn statuses(workspace: &Path, records: Vec<Record>) -> Vec<(Record, Status)> {
    let table = std::cell::OnceCell::new();
    records
        .into_iter()
        .filter_map(|record| {
            let status = report(workspace, &record, Some(&table)).ok()?;
            Some((record, status))
        })
        .collect()
}

pub(super) fn report(workspace: &Path, record: &Record, table: Option<&Table>) -> Result<Status> {
    let log = log_path(workspace, record);
    let exit = exit_path(workspace, &record.id);
    let meta = std::fs::metadata(&log).ok();
    let bytes = meta.as_ref().map_or(0, std::fs::Metadata::len);
    let quiet = meta
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.elapsed().ok())
        .map_or(0, |quiet| quiet.as_secs());
    let mut children = String::new();
    let state = if !exit.is_file() && alive_since(record.pid, record.started) {
        if let Some(table) = table {
            let table = table.get_or_init(crate::process::table);
            children = tree::block("children", &tree::lines_in(table, record.pid, workspace));
        }
        JobState::Running
    } else if exit.is_file() {
        let text = crate::durable::store::read_or_empty(&exit)?;
        let mut lines = text.lines();
        let first = lines.next().unwrap_or_default().trim();
        if first == KILLED {
            let killed: Vec<String> = lines.map(str::to_string).collect();
            children = tree::block("children at kill", &killed);
            JobState::Killed
        } else if first == ENDED {
            let notes: Vec<String> = lines.map(str::to_string).collect();
            children = tree::block("note", &notes);
            JobState::Dead
        } else {
            first
                .parse::<i32>()
                .map_or(JobState::Dead, JobState::Exited)
        }
    } else {
        JobState::Dead
    };
    let quiet =
        if state == JobState::Running && quiet >= crate::limits::limits().job_quiet().as_secs() {
            format!(", no new output for {}m", quiet / 60)
        } else {
            String::new()
        };
    let preview = if log.is_file() {
        preview(&log, 8 * 1024)?
    } else {
        String::new()
    };
    Ok(Status {
        text: format!(
            "job {id}\ncommand: {command}\nworkspace: {cwd}\npid: {pid}\nstate: {state}\n{children}log: {log} ({bytes} bytes{quiet})\n{preview}",
            id = record.id,
            command = record.command,
            cwd = record.workspace,
            pid = record.pid,
            log = log.display(),
        ),
        state,
    })
}

pub(crate) fn preview(path: &Path, cap: usize) -> Result<String> {
    let meta = std::fs::metadata(path)?;
    let len = meta.len();
    let mut file = std::fs::File::open(path)?;
    if len == 0 {
        return Ok(String::new());
    }
    if len <= cap as u64 {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        return Ok(String::from_utf8_lossy(&buf).into_owned());
    }
    let head_len = cap * 3 / 4;
    let tail_len = cap - head_len;
    let mut head = vec![0_u8; head_len];
    file.read_exact(&mut head)?;
    file.seek(SeekFrom::Start(len.saturating_sub(tail_len as u64)))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    Ok(format!(
        "{}\n\n[... {len} bytes in {} ...]\n\n{}",
        String::from_utf8_lossy(&head),
        path.display(),
        String::from_utf8_lossy(&tail)
    ))
}
