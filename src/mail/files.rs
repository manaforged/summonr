use super::*;

impl Mailbox {
    pub(crate) fn pending(&self) -> Result<bool> {
        pending(&inbox_path(&self.dir, &self.id), self.cursor)
    }

    pub(crate) fn changed(&self) -> impl std::future::Future<Output = Result<()>> + use<> {
        let path = inbox_path(&self.dir, &self.id);
        let cursor = self.cursor;
        async move {
            loop {
                if pending(&path, cursor)? {
                    return Ok(());
                }
                tokio::time::sleep(crate::limits::limits().mail_poll()).await;
            }
        }
    }
}

fn pending(path: &Path, cursor: u64) -> Result<bool> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let len = file.metadata()?.len();
    if len == cursor {
        return Ok(false);
    }
    file.seek(SeekFrom::Start(if len < cursor { 0 } else { cursor }))?;
    let mut unread = Vec::new();
    file.read_to_end(&mut unread)?;
    Ok(unread.contains(&b'\n'))
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
    std::fs::create_dir_all(dir.join("board"))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(board_path(dir, cwd))?;
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
