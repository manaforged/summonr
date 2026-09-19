pub(super) const BAR: usize = 24;

pub(super) fn bars(source: &str) -> Option<Vec<String>> {
    xychart(source).or_else(|| series(source))
}

pub(super) fn xychart(source: &str) -> Option<Vec<String>> {
    let mut title = None;
    let mut labels = Vec::new();
    let mut values = Vec::new();
    let mut saw = false;
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
    {
        let token = line.split_whitespace().next().unwrap_or("");
        if token == "xychart" || token.starts_with("xychart-beta") {
            saw = true;
            continue;
        }
        if !saw {
            return None;
        }
        if let Some(rest) = line.strip_prefix("title") {
            title = Some(rest.trim().trim_matches('"').to_string());
            continue;
        }
        if line.starts_with("x-axis") {
            labels = csv_items(line);
            continue;
        }
        if line.starts_with("y-axis") {
            continue;
        }
        if line.starts_with("bar") || line.starts_with("line") {
            values = csv_items(line)
                .into_iter()
                .filter_map(|item| item.parse::<f64>().ok())
                .collect();
        }
    }
    if !saw || values.is_empty() {
        return None;
    }
    if labels.len() != values.len() {
        labels = (1..=values.len()).map(|index| index.to_string()).collect();
    }
    Some(paint_bars(
        title,
        &labels
            .into_iter()
            .zip(values)
            .collect::<Vec<(String, f64)>>(),
    ))
}

pub(super) fn series(source: &str) -> Option<Vec<String>> {
    let mut title = None;
    let mut rows = Vec::new();
    for line in source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("%%"))
    {
        let token = line.split_whitespace().next().unwrap_or("");
        if matches!(
            token,
            "graph"
                | "flowchart"
                | "stateDiagram"
                | "stateDiagram-v2"
                | "sequenceDiagram"
                | "pie"
                | "xychart"
                | "xychart-beta"
        ) {
            return None;
        }
        if rows.is_empty()
            && line
                .split_whitespace()
                .all(|word| word.parse::<f64>().is_err())
            && !line.contains(':')
            && !line.contains(',')
        {
            title = Some(line.trim_matches('"').to_string());
            continue;
        }
        rows.push(pair(line)?);
    }
    (!rows.is_empty()).then(|| paint_bars(title, &rows))
}

pub(super) fn pair(line: &str) -> Option<(String, f64)> {
    if let Some((label, value)) = line.rsplit_once(':') {
        let value: f64 = value.trim().parse().ok()?;
        let label = label.trim().trim_matches('"').to_string();
        return (!label.is_empty() && value >= 0.0).then_some((label, value));
    }
    if let Some((label, value)) = line.rsplit_once(',') {
        let value: f64 = value.trim().parse().ok()?;
        let label = label.trim().trim_matches('"').to_string();
        return (!label.is_empty() && value >= 0.0).then_some((label, value));
    }
    let value = line.split_whitespace().next_back()?;
    let label = line[..line.len() - value.len()].trim().trim_matches('"');
    let value: f64 = value.parse().ok()?;
    (!label.is_empty() && value >= 0.0).then_some((label.to_string(), value))
}

pub(super) fn csv_items(line: &str) -> Vec<String> {
    let Some(open) = line.find('[') else {
        return Vec::new();
    };
    let Some(close) = line.rfind(']') else {
        return Vec::new();
    };
    if close <= open {
        return Vec::new();
    }
    line[open + 1..close]
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

pub(super) fn paint_bars(title: Option<String>, rows: &[(String, f64)]) -> Vec<String> {
    let peak = rows.iter().map(|(_, value)| *value).fold(0.0, f64::max);
    let width = rows
        .iter()
        .map(|(label, _)| label.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = Vec::new();
    if let Some(title) = title {
        out.push(title);
    }
    for (label, value) in rows {
        let share = if peak > 0.0 { value / peak } else { 0.0 };
        let filled = usize::try_from((share.clamp(0.0, 1.0) * BAR as f64).round() as u64)
            .unwrap_or(BAR)
            .min(BAR);
        let pad = " ".repeat(width.saturating_sub(label.chars().count()));
        out.push(format!(
            "{label}{pad}  {}{}  {value}",
            "█".repeat(filled),
            "░".repeat(BAR.saturating_sub(filled)),
        ));
    }
    out
}
