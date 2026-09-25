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

pub(super) fn write_block_segments(
    segments: &[String],
    workspace: &Path,
    roots: &[PathBuf],
) -> Option<Block> {
    let mut outside_cwd = false;
    for segment in segments {
        let segment = if outside_cwd || !workspace.join(".git").exists() {
            segment.clone()
        } else {
            segment.replace("$(git rev-parse --show-toplevel)", ".")
        };
        let words: Vec<String> = unquoted(&segment, &[" ", "\t"])
            .into_iter()
            .map(|word| word.trim_matches(|c| c == '"' || c == '\'').to_string())
            .filter(|word| !word.is_empty())
            .collect();
        let Some(verb) = words.iter().find(|word| !word.contains('=')) else {
            continue;
        };
        let verb = verb.rsplit('/').next().unwrap_or(verb).to_string();
        let args = words
            .iter()
            .skip_while(|word| {
                word.contains('=') || **word != verb && !word.ends_with(&format!("/{verb}"))
            })
            .skip(1);
        let row = verb_row(&verb);
        let write = row.map_or(Write::None, |row| row.write);
        let creates = matches!(write, Write::Creates);
        let destructive = write.destroys(&words);
        for word in &words {
            if let Some((_, target)) = word.split_once('>') {
                let target = target.trim_start_matches(['>', '&', '|']);
                let target = if target.is_empty() { continue } else { target };
                if let Some(block) = block_target(target, workspace, outside_cwd, roots) {
                    return Some(block);
                }
            }
        }
        let mut redirect_next = false;
        let mut targets: Vec<String> = Vec::new();
        for word in args.clone() {
            if redirect_next {
                redirect_next = false;
                if let Some(block) = block_target(word, workspace, outside_cwd, roots) {
                    return Some(block);
                }
                continue;
            }
            if word == ">" || word == ">>" || word == "1>" || word == "2>" || word == "&>" {
                redirect_next = true;
                continue;
            }
            if word.starts_with('-') {
                continue;
            }
            if row.is_some_and(|row| row.cwd == Cwd::Tracked) {
                if is_outside(word, workspace, outside_cwd, roots) {
                    outside_cwd = true;
                }
                break;
            }
            if creates {
                if let Some(block) = block_target(word, workspace, outside_cwd, roots) {
                    return Some(block);
                }
                continue;
            }
            if destructive {
                if in_temp(word) {
                    return Some(Block::Outside(word.clone()));
                }
                targets.push(word.clone());
            }
        }
        let checked: Vec<&String> = if matches!(write, Write::Last) {
            targets.last().into_iter().collect()
        } else {
            targets.iter().collect()
        };
        for word in checked {
            if let Some(block) = block_target(word, workspace, outside_cwd, roots) {
                return Some(block);
            }
        }
    }
    None
}
