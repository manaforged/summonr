use super::*;

pub(in crate::tui) fn export_session(
    session: &std::path::Path,
    id: &str,
    target: Option<String>,
    ui: &UnboundedSender<Msg>,
) {
    let out = match target {
        Some(path) => Ok(std::path::PathBuf::from(path)),
        None => {
            crate::prompt::summonr_home().map(|home| home.join("exports").join(format!("{id}.md")))
        }
    };
    let written = out.and_then(|out| {
        let text = crate::export_session(session)?;
        if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&out)
            .map_err(|error| {
                if error.kind() == ErrorKind::AlreadyExists {
                    crate::Error::Config(format!(
                        "{} already exists. Use /export PATH with a different filename.",
                        out.display()
                    ))
                } else {
                    error.into()
                }
            })?;
        file.write_all(text.as_bytes())?;
        Ok(out)
    });
    let message = match written {
        Ok(out) => Msg::System(format!("exported to {}", out.display())),
        Err(error) => Msg::Failed(error.to_string()),
    };
    drop(ui.send(message));
}

pub(in crate::tui) fn announce(
    ui: &UnboundedSender<Msg>,
    steering: &Arc<Mutex<VecDeque<UserInput>>>,
    followups: &VecDeque<UserInput>,
) {
    let steering = steering.lock().map_or(0, |queue| queue.len());
    drop(ui.send(Msg::Queued {
        steering,
        queued: followups.len(),
    }));
}
