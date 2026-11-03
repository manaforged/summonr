use super::*;

pub(super) enum Unread {
    Missing,
    Tail(Vec<u8>),
    Whole(Vec<u8>),
}

impl Mailbox {
    pub(crate) fn poll(&mut self) -> Result<Vec<Mail>> {
        let path = inbox_path(&self.dir, &self.id);
        let stamp = match std::fs::metadata(&path) {
            Ok(meta) => Stamp {
                len: meta.len(),
                modified: meta.modified().ok(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.rewind();
                self.stamp = None;
                return Ok(Vec::new());
            }
            Err(error) => return Err(error.into()),
        };
        if self.stamp == Some(stamp) {
            return Ok(Vec::new());
        }
        let Some(bytes) = self.read_new(&path)? else {
            return Ok(Vec::new());
        };
        let before = self.cursor;
        let parsed = self.parse(&path, &bytes);
        let used =
            usize::try_from(self.cursor - before).map_or(bytes.len(), |used| used.min(bytes.len()));
        self.remember(&bytes[..used]);
        let (mail, drained) = parsed?;
        if drained {
            self.stamp = Some(stamp);
        }
        Ok(mail)
    }

    fn read_new(&mut self, path: &Path) -> Result<Option<Vec<u8>>> {
        match unread(path, self.cursor, &self.mark)? {
            Unread::Missing => {
                self.rewind();
                Ok(None)
            }
            Unread::Tail(bytes) => Ok(Some(bytes)),
            Unread::Whole(bytes) => {
                self.rewind();
                Ok(Some(bytes))
            }
        }
    }

    fn rewind(&mut self) {
        self.cursor = 0;
        self.mark.clear();
    }

    fn remember(&mut self, consumed: &[u8]) {
        self.mark.extend_from_slice(consumed);
        let excess = self.mark.len().saturating_sub(MARK_BYTES);
        self.mark.drain(..excess);
    }

    fn parse(&mut self, path: &Path, bytes: &[u8]) -> Result<(Vec<Mail>, bool)> {
        let complete = bytes
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |end| end + 1);
        let mut mail = Vec::new();
        for line in bytes[..complete].split_inclusive(|byte| *byte == b'\n') {
            let width = line.len() as u64;
            let body = line.strip_suffix(b"\n").unwrap_or(line);
            let body = body.strip_suffix(b"\r").unwrap_or(body);
            match serde_json::from_slice::<Mail>(body) {
                Ok(item) => mail.push(item),
                Err(_) if !mail.is_empty() => return Ok((mail, false)),
                Err(error) => {
                    self.cursor += width;
                    return Err(Error::Config(format!(
                        "{} byte {}: {error}",
                        path.display(),
                        self.cursor
                    )));
                }
            }
            self.cursor += width;
        }
        Ok((mail, true))
    }

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
