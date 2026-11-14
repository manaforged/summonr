use super::*;

pub(crate) fn board_text(board: &[(String, String, String)], posts: &[Mail]) -> String {
    if board.is_empty() && posts.is_empty() {
        return "the board is empty".to_string();
    }
    let claims = board
        .iter()
        .map(|(key, value, from)| format!("{key} = {value} (from {from})"));
    let posts = posts
        .iter()
        .map(|post| format!("@{}: {}", post.sender(), post.text));
    claims.chain(posts).collect::<Vec<_>>().join("\n")
}

pub(crate) fn board_entry(board: &[(String, String, String)], key: &str) -> String {
    board
        .iter()
        .find(|(posted, _, _)| posted == key)
        .map_or_else(
            || format!("no post for {key}"),
            |(key, value, from)| format!("{key} = {value} (from {from})"),
        )
}

/// Posts `mail` to the board at `root`: it joins the board's history and reaches every other live agent on that board.
pub(super) fn say_in(dir: &Path, root: &Path, mail: Mail) -> Result<(usize, String)> {
    let root = root.display().to_string();
    let to: Vec<String> = live(dir)
        .into_iter()
        .filter(|agent| agent.id != mail.from && agent.board_root() == root)
        .map(|agent| agent.id)
        .collect();
    append_feed(dir, Path::new(&root), &mail)?;
    let count = append(dir, &to, &mail)?;
    Ok((count, mail.id))
}

impl Mailbox {
    pub(crate) fn board(&self) -> Result<Vec<(String, String, String)>> {
        read_board(&self.dir, &self.board)
    }

    pub(crate) fn post(&self, key: &str, value: &str) -> Result<()> {
        post_board(&self.dir, &self.board, &self.id, key, value)
    }

    /// Posts `text` to every agent on this board; it wakes only those it mentions.
    pub(crate) fn say(&self, text: &str) -> Result<(usize, String)> {
        let mail = Mail {
            board: true,
            ..Mail::new(&self.id, &self.name, &self.kind, text, None)
        };
        say_in(&self.dir, &self.board, mail)
    }

    pub(crate) fn posts(&self) -> Result<Vec<Mail>> {
        read_feed(&self.dir, &self.board)
    }

    pub(crate) fn board_root(&self) -> &Path {
        &self.board
    }

    /// Moves this session onto the board at `root`, as a child joins its lead's board.
    pub(crate) fn share_board(&mut self, root: &Path, model: &str, cwd: &Path) {
        root.clone_into(&mut self.board);
        self.write_record(model, cwd, self.status);
    }

    /// Whether `mail` starts a turn here: direct mail always, a board post only when it mentions this session.
    pub(crate) fn wakes(&self, mail: &Mail) -> bool {
        !mail.board
            || mentions(&mail.text)
                .any(|word| word == "all" || word == self.name || word == self.id)
    }
}
