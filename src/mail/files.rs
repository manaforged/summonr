use super::*;

pub(super) enum Unread {
    Missing,
    Tail(Vec<u8>),
    Whole(Vec<u8>),
}

impl Mailbox {
    pub(crate) fn pending(&self) -> Result<bool> {
        pending(&inbox_path(&self.dir, &self.id), self.cursor, &self.mark)
    }

    pub(crate) fn changed(&self) -> impl std::future::Future<Output = Result<()>> + use<> {
        let path = inbox_path(&self.dir, &self.id);
        let (cursor, mark) = (self.cursor, self.mark.clone());
        async move {
            loop {
                if pending(&path, cursor, &mark)? {
                    return Ok(());
                }
                tokio::time::sleep(crate::limits::limits().mail_poll()).await;
            }
        }
    }
}

fn pending(path: &Path, cursor: u64, mark: &[u8]) -> Result<bool> {
    Ok(match unread(path, cursor, mark)? {
        Unread::Missing => false,
        Unread::Tail(bytes) | Unread::Whole(bytes) => bytes.contains(&b'\n'),
    })
}

pub(super) fn unread(path: &Path, cursor: u64, mark: &[u8]) -> Result<Unread> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Unread::Missing),
        Err(error) => return Err(error.into()),
    };
    let mut bytes = Vec::new();
    if file.metadata()?.len() >= cursor {
        file.seek(SeekFrom::Start(cursor.saturating_sub(mark.len() as u64)))?;
        file.read_to_end(&mut bytes)?;
        if bytes.starts_with(mark) {
            return Ok(Unread::Tail(bytes.split_off(mark.len())));
        }
        bytes.clear();
    }
    file.seek(SeekFrom::Start(0))?;
    file.read_to_end(&mut bytes)?;
    Ok(Unread::Whole(bytes))
}

pub(super) fn end(path: &Path) -> Result<(u64, Vec<u8>)> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok((0, Vec::new())),
        Err(error) => return Err(error.into()),
    };
    let len = file.metadata()?.len();
    let start = len.saturating_sub(MARK_BYTES as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut mark = Vec::new();
    file.take(len - start).read_to_end(&mut mark)?;
    Ok((start + mark.len() as u64, mark))
}

fn board_path(dir: &Path, cwd: &Path) -> PathBuf {
    dir.join("board")
        .join(format!("{}.jsonl", slug(&cwd.display().to_string())))
}

pub(super) fn inbox_path(dir: &Path, id: &str) -> PathBuf {
    dir.join("inbox").join(format!("{}.jsonl", clean_id(id)))
}

pub(crate) fn read_board(dir: &Path, cwd: &Path) -> Result<Vec<(String, String, String)>> {
    let path = board_path(dir, cwd);
    let _lock = store::lock(&path, true)?;
    let text = store::read_or_empty(&path)?;
    let mut board: Vec<(String, String, String)> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let post = serde_json::from_str::<Post>(line).map_err(|error| {
            Error::Config(format!("{} line {}: {error}", path.display(), index + 1))
        })?;
        match board.iter_mut().find(|(key, _, _)| *key == post.key) {
            Some(slot) => *slot = (post.key, post.value, post.from),
            None => board.push((post.key, post.value, post.from)),
        }
    }
    if text.len() > BOARD_COMPACT_BYTES {
        let folded = board
            .iter()
            .filter_map(|(key, value, from)| {
                serde_json::to_string(&Post {
                    ts: now(),
                    from: from.clone(),
                    key: key.clone(),
                    value: value.clone(),
                })
                .ok()
            })
            .collect::<Vec<_>>()
            .join("\n");
        store::write(&path, format!("{folded}\n"), store::Durability::Synced)?;
    }
    Ok(board)
}

pub(crate) fn post_board(dir: &Path, cwd: &Path, from: &str, key: &str, value: &str) -> Result<()> {
    let post = Post {
        ts: now(),
        from: clean_id(from),
        key: key.to_string(),
        value: value.to_string(),
    };
    let line = serde_json::to_string(&post)?;
    use std::io::Write as _;
    let path = board_path(dir, cwd);
    let _lock = store::lock(&path, true)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

pub(crate) fn read_inbox(dir: &Path, id: &str) -> Result<Vec<Mail>> {
    let path = inbox_path(dir, id);
    let _lock = store::lock(&path, true)?;
    Ok(parse_inbox(&store::read_or_empty(&path)?))
}

pub(crate) fn take_inbox(dir: &Path, id: &str) -> Result<Vec<Mail>> {
    if live(dir).iter().any(|agent| agent.id == clean_id(id)) {
        return Err(Error::Config(format!(
            "`{id}` is a live session; it reads its own inbox, so --clear would drop its mail"
        )));
    }
    let path = inbox_path(dir, id);
    let _lock = store::lock(&path, true)?;
    let mail = parse_inbox(&store::read_or_empty(&path)?);
    store::write(&path, "", store::Durability::Synced)?;
    Ok(mail)
}

fn parse_inbox(text: &str) -> Vec<Mail> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Mail>(line).ok())
        .collect()
}
