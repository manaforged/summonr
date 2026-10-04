use super::walk::Step;
use super::*;

pub(super) enum Block {
    Outside(String),
    Protected(String),
}

pub(super) fn locate(word: &str, workspace: &Path) -> PathBuf {
    expand_home(word).unwrap_or_else(|| {
        let path = Path::new(word);
        if path.has_root() {
            path.to_path_buf()
        } else {
            workspace.join(path)
        }
    })
}

pub(super) fn block_target(
    word: &str,
    workspace: &Path,
    outside_cwd: bool,
    roots: &[PathBuf],
) -> Option<Block> {
    if protected_write(&locate(word, workspace)) {
        return Some(Block::Protected(word.to_string()));
    }
    if outside_cwd || is_outside(word, workspace, outside_cwd, roots) {
        return Some(Block::Outside(word.to_string()));
    }
    None
}

pub(super) fn write_block(steps: &[Step], workspace: &Path, roots: &[PathBuf]) -> Option<Block> {
    steps
        .iter()
        .find_map(|step| step_block(step, workspace, roots))
}

fn step_block(step: &Step, workspace: &Path, roots: &[PathBuf]) -> Option<Block> {
    let check = |target: &str| block_target(target, workspace, step.outside, roots);
    if let Some(block) = step.redirects().into_iter().find_map(check) {
        return Some(block);
    }
    let row = verb_row(&step.verb()?)?;
    if row.cwd == Cwd::Tracked {
        return None;
    }
    let operands = step.operands();
    if matches!(row.write, Write::Creates) {
        return operands.into_iter().find_map(check);
    }
    if !row.write.destroys(&step.words) {
        return None;
    }
    if let Some(temp) = operands.iter().find(|word| in_temp(word)) {
        return Some(Block::Outside((*temp).to_string()));
    }
    let checked = if matches!(row.write, Write::Last) {
        &operands[operands.len().saturating_sub(1)..]
    } else {
        &operands[..]
    };
    checked.iter().copied().find_map(check)
}

fn write_directory(path: &str) -> PathBuf {
    let trimmed = path.trim_end_matches(['/', '\\']);
    let directory =
        trimmed.rsplit_once(['/', '\\']).map_or(
            trimmed,
            |(head, _)| {
                if head.is_empty() { "/" } else { head }
            },
        );
    expand_path(directory)
}

#[must_use]
pub(crate) fn write_denied(path: &str) -> String {
    let dir = write_directory(path);
    format!(
        "write to `{path}` is outside the writable roots — add {} to [sandbox] write in ~/.summonr/config.toml, or /permissions write {}",
        dir.display(),
        dir.display()
    )
}

#[must_use]
pub(crate) fn outside_hint(path: &str) -> String {
    write_denied(path)
}

pub(crate) fn grant_from_error(error: &Error) -> Option<Grant> {
    match error {
        Error::EditApproval { setting, .. } => Some(Grant::Edit(setting)),
        Error::Denied(message) | Error::Tool(message) => grant_from_message(message),
        _ => None,
    }
}

fn grant_from_message(message: &str) -> Option<Grant> {
    let rest = message.split_once("/permissions write ")?.1;
    let dir = rest.split([',', '\n']).next()?.trim();
    if dir.is_empty() || dir == "PATH" {
        return None;
    }
    write_grant(expand_path(dir))
}

pub(crate) fn outside_write(command: &str, workspace: &Path, roots: &[PathBuf]) -> Option<Grant> {
    match write_block(
        &walk::walk(command, workspace, workspace, roots),
        workspace,
        roots,
    ) {
        Some(Block::Outside(path)) => write_grant(write_directory(&path)),
        Some(Block::Protected(path)) => {
            write_grant(write_directory(&locate(&path, workspace).to_string_lossy()))
        }
        None => None,
    }
}

fn write_grant(path: PathBuf) -> Option<Grant> {
    if !(path.is_absolute() || path.has_root()) {
        return None;
    }
    Some(if protected_write(&path) {
        Grant::Guarded(path)
    } else {
        Grant::Write(path)
    })
}

#[derive(Clone, Copy)]
enum WriteRoot {
    CargoHome,
    NpmCache,
    GoPath,
}

const IMPLICIT_WRITES: &[(&str, WriteRoot)] = &[
    ("cargo", WriteRoot::CargoHome),
    ("rustup", WriteRoot::CargoHome),
    ("npm", WriteRoot::NpmCache),
    ("npx", WriteRoot::NpmCache),
    ("pnpm", WriteRoot::NpmCache),
    ("yarn", WriteRoot::NpmCache),
    ("bun", WriteRoot::NpmCache),
    ("go", WriteRoot::GoPath),
];

#[must_use]
pub(crate) fn implicit_write(command: &str) -> Option<PathBuf> {
    let verb = first_verb(command)?;
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)?;
    let (_, root) = IMPLICIT_WRITES.iter().find(|(name, _)| *name == verb)?;
    let dir = match root {
        WriteRoot::CargoHome => home.join(".cargo"),
        WriteRoot::NpmCache => home.join(".npm"),
        WriteRoot::GoPath => std::env::var_os("GOPATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("go")),
    };
    (!protected_write(&dir)).then_some(dir)
}
