use summonr_core::error::Error;
use summonr_core::text::squash;

pub(super) fn no_match(text: &str, old: &str) -> Error {
    let mut out = "old does not appear in the file".to_string();
    let needle = old
        .lines()
        .map(str::trim)
        .filter(|line| line.len() >= 4)
        .max_by_key(|line| line.len());
    if let Some(needle) = needle {
        let squashed = squash(needle);
        let floor = squashed.len().div_ceil(2);
        let mut needle_counts = [0_u32; 256];
        for byte in squashed.bytes() {
            needle_counts[usize::from(byte)] += 1;
        }
        let mut line_counts = [0_u32; 256];
        let mut scored: Vec<(f64, usize, &str)> = text
            .lines()
            .enumerate()
            .filter_map(|(index, line)| {
                if line.len() < floor {
                    return None;
                }
                line_counts.fill(0);
                for byte in line.bytes() {
                    line_counts[usize::from(byte)] += 1;
                }
                let mut shared = 0_usize;
                for byte in 0..256_usize {
                    if needle_counts[byte] > 0 {
                        shared += (needle_counts[byte].min(line_counts[byte])) as usize;
                    }
                }
                if shared < floor {
                    return None;
                }
                let candidate = squash(line);
                if candidate.is_empty() {
                    return None;
                }
                let score = lcs(&squashed, &candidate) as f64 / squashed.len() as f64;
                (score >= 0.5).then_some((score, index, line))
            })
            .collect();
        scored.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(left.1.cmp(&right.1))
        });
        let found: Vec<String> = scored
            .iter()
            .take(5)
            .map(|(_, index, line)| format!("  {}: {}", index + 1, line.trim()))
            .collect();
        if !found.is_empty() {
            out.push_str(&format!("; closest lines:\n{}", found.join("\n")));
        }
    }
    Error::Tool(out)
}

fn lcs(left: &str, right: &str) -> usize {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    let mut row = vec![0_usize; right.len() + 1];
    for &a in left {
        let mut diagonal = 0;
        for index in 1..=right.len() {
            let above = row[index];
            row[index] = if a == right[index - 1] {
                diagonal + 1
            } else {
                above.max(row[index - 1])
            };
            diagonal = above;
        }
    }
    row[right.len()]
}
