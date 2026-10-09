use std::time::Duration;

use super::{Limits, seconds as secs};

const DAY_SECS: u64 = 24 * 60 * 60;

fn millis(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn days(value: u64) -> Duration {
    Duration::from_secs(value.saturating_mul(DAY_SECS))
}

impl Limits {
    pub(crate) fn retry_base(&self) -> Duration {
        millis(self.retry_base_ms)
    }

    pub(crate) fn retry_max(&self) -> Duration {
        millis(self.retry_max_ms)
    }

    pub(crate) fn retry_after_max(&self) -> Duration {
        secs(self.retry_after_max_secs)
    }

    pub(crate) fn stream_idle(&self) -> Duration {
        secs(self.stream_idle_secs)
    }

    pub(crate) fn stream_total(&self) -> Duration {
        secs(self.stream_total_secs)
    }

    pub(crate) fn connect(&self) -> Duration {
        secs(self.connect_secs)
    }

    pub(crate) fn job_wait(&self) -> Duration {
        secs(self.job_wait_secs)
    }

    pub(crate) fn grant_wait(&self) -> Duration {
        secs(self.grant_wait_secs)
    }

    pub(crate) fn probe(&self) -> Duration {
        secs(self.probe_secs)
    }

    pub(crate) fn auth(&self) -> Duration {
        secs(self.auth_secs)
    }

    pub(crate) fn model_probe(&self) -> Duration {
        secs(self.model_probe_secs)
    }

    pub(crate) fn web_search(&self) -> Duration {
        secs(self.web_search_secs)
    }

    pub(crate) fn search(&self) -> Duration {
        secs(self.search_secs)
    }

    pub(crate) fn web_connect(&self) -> Duration {
        secs(self.web_connect_secs)
    }

    pub(crate) fn web_fetch(&self) -> Duration {
        secs(self.web_fetch_secs)
    }

    pub(crate) fn keychain(&self) -> Duration {
        millis(self.keychain_ms)
    }

    pub(crate) fn keychain_poll(&self) -> Duration {
        millis(self.keychain_poll_ms)
    }

    pub(crate) fn mcp_open(&self) -> Duration {
        secs(self.mcp_open_secs)
    }

    pub(crate) fn mcp_call(&self) -> Duration {
        secs(self.mcp_call_secs)
    }

    pub(crate) fn mcp_idle(&self) -> Duration {
        secs(self.mcp_idle_secs)
    }

    pub(crate) fn mcp_cancel_write(&self) -> Duration {
        secs(self.mcp_cancel_write_secs)
    }

    pub(crate) fn mcp_close_grace(&self) -> Duration {
        millis(self.mcp_close_grace_ms)
    }

    pub(crate) fn mcp_stderr_settle(&self) -> Duration {
        secs(self.mcp_stderr_settle_secs)
    }

    pub(crate) fn lsp_start(&self) -> Duration {
        secs(self.lsp_start_secs)
    }

    pub(crate) fn lsp_idle(&self) -> Duration {
        secs(self.lsp_idle_secs)
    }

    pub(crate) fn lsp_request(&self) -> Duration {
        secs(self.lsp_request_secs)
    }

    pub(crate) fn lsp_check(&self) -> Duration {
        secs(self.lsp_check_secs)
    }

    pub(crate) fn log_retain(&self) -> Duration {
        days(self.log_retain_days)
    }

    pub(crate) fn session_retain(&self) -> Duration {
        days(self.session_retain_days)
    }

    pub(crate) fn job_retain(&self) -> Duration {
        days(self.job_retain_days)
    }

    pub(crate) fn speed_window(&self) -> Duration {
        secs(self.speed_window_secs)
    }

    pub(crate) fn reader_window(&self) -> Duration {
        millis(self.reader_window_ms)
    }

    pub(crate) fn frame_window(&self) -> Duration {
        secs(self.frame_window_secs)
    }

    pub(crate) fn animation(&self) -> Duration {
        millis(self.animation_ms)
    }

    pub(crate) fn spinner(&self) -> Duration {
        millis(self.spinner_ms)
    }

    pub(crate) fn status_refresh(&self) -> Duration {
        secs(self.status_refresh_secs)
    }

    pub(crate) fn beat(&self) -> Duration {
        secs(self.beat_secs)
    }

    pub(crate) fn notice(&self) -> Duration {
        secs(self.notice_secs)
    }

    #[cfg(feature = "gui")]
    pub(crate) fn worker_stop(&self) -> Duration {
        secs(self.worker_stop_secs)
    }

    pub(crate) fn repeat_window(&self) -> Duration {
        secs(self.repeat_window_secs)
    }

    pub(crate) fn token_expiry_skew(&self) -> Duration {
        secs(self.token_expiry_skew_secs)
    }
}
