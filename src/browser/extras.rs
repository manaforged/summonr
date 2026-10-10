use std::path::Path;

use super::call::Extra;
use super::{fail, report};
use crate::error::Result;

const SHOT_PREFIX: &str = "shot-";
const SHOT_SUFFIX: &str = ".png";
const BLANK: &str = "about:blank";

pub(super) async fn run(
    page: &mut charm::Session,
    extra: Extra,
    shots: &Path,
    out_max: usize,
) -> Result<String> {
    match extra {
        Extra::Snapshot {
            interactive_only,
            max_chars,
        } => {
            let options = charm::SnapshotOptions {
                interactive_only,
                max_chars: Some(max_chars.unwrap_or(out_max).min(out_max)),
                ..charm::SnapshotOptions::default()
            };
            Ok(page.snapshot(options).await.map_err(fail)?.text())
        }
        Extra::Read { format, max_chars } => {
            let cap = max_chars.unwrap_or(out_max).min(out_max);
            page.text(format.charm(), cap).await.map_err(fail)
        }
        Extra::Screenshot { full_page } => {
            let options = charm::ScreenshotOptions {
                full_page,
                ..charm::ScreenshotOptions::default()
            };
            let bytes = page.screenshot(options).await.map_err(fail)?;
            std::fs::create_dir_all(shots)?;
            let path = crate::durable::store::keep(shots, SHOT_PREFIX, SHOT_SUFFIX, &bytes)?;
            Ok(format!("screenshot: {}", path.display()))
        }
        Extra::Tabs => Ok(report::tabs(&page.pages().await.map_err(fail)?)),
        Extra::Login { .. } | Extra::Close => Ok(String::new()),
    }
}

pub(super) async fn act(
    page: &mut charm::Session,
    action: charm::Action,
    report: charm::ActReport,
    out_max: usize,
) -> Result<String> {
    let outcome = page.act(action, report).await.map_err(fail)?;
    let escaped = !crate::web::reachable(&outcome.url_after).await;
    if escaped {
        page.goto(BLANK).await.map_err(fail)?;
    }
    Ok(report::outcome(&outcome, escaped, out_max))
}
