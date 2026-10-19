use serde::Deserialize;

use crate::endpoint::CacheTtl;

macro_rules! table {
    ($($name:ident: $ty:ty),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct Limits {
            $(pub $name: $ty,)*
        }

        #[derive(Clone, Debug, Default, Deserialize)]
        pub(crate) struct Overrides {
            $(pub $name: Option<$ty>,)*
        }

        impl Limits {
            pub(crate) fn overlay(mut self, with: &Overrides) -> Self {
                $(self.$name = with.$name.unwrap_or(self.$name);)*
                self
            }
        }
    };
}

table! {
    cache_ttl: CacheTtl,
    retry_limit: u32,
    retry_base_ms: u64,
    retry_max_ms: u64,
    retry_after_max_secs: u64,
    stream_idle_secs: u64,
    stream_total_secs: u64,
    connect_secs: u64,
    compact_percent: usize,
    elide_percent: usize,
    compact_chars: usize,
    summary_retry_rounds: usize,
    job_wait_secs: u64,
    command_wait_secs: u64,
    command_kill_secs: u64,
    agent_park_secs: u64,
    grant_wait_secs: u64,
    job_poll_start_secs: u64,
    job_poll_cap_secs: u64,
    mail_stale_secs: u64,
    mail_sweep_secs: u64,
    verify_timeout_secs: u64,
    probe_secs: u64,
    auth_secs: u64,
    model_probe_secs: u64,
    web_search_secs: u64,
    search_secs: u64,
    search_index_secs: u64,
    models_catalog_secs: u64,
    models_listing_secs: u64,
    search_index_after_ms: u64,
    web_connect_secs: u64,
    web_fetch_secs: u64,
    web_body_bytes: usize,
    web_redirect_max: usize,
    keychain_ms: u64,
    keychain_poll_ms: u64,
    mcp_open_secs: u64,
    mcp_call_secs: u64,
    mcp_idle_secs: u64,
    mcp_progress_stretch: u32,
    mcp_cancel_write_secs: u64,
    mcp_close_grace_ms: u64,
    image_side_px: u32,
    image_bytes: usize,
    mcp_log_max_bytes: u64,
    mcp_frame_max_bytes: usize,
    mcp_stderr_tail_bytes: usize,
    mcp_stderr_settle_secs: u64,
    mcp_defer_bytes: usize,
    mcp_found_max: usize,
    lsp_start_secs: u64,
    lsp_idle_secs: u64,
    servers_max: usize,
    receipt_items_max: usize,
    closest_percent: usize,
    typo_min_chars: usize,
    diff_split_min_cols: u16,
    lsp_request_secs: u64,
    lsp_check_secs: u64,
    log_retain_days: u64,
    session_retain_days: u64,
    session_prune_secs: u64,
    job_retain_days: u64,
    speed_window_secs: u64,
    reader_window_ms: u64,
    stamp_settle_ms: u64,
    frame_window_secs: u64,
    animation_ms: u64,
    wake_margin_ms: u64,
    spinner_ms: u64,
    splash_reveal_ms: u64,
    status_refresh_secs: u64,
    beat_secs: u64,
    notice_secs: u64,
    worker_stop_secs: u64,
    repeat_window_secs: u64,
    step_attempts: u16,
    repair_limit: usize,
    repeat_nudge: usize,
    edit_hunk_rows: usize,
    display_patch_lines: usize,
    read_line_limit: usize,
    paste_fold_lines: usize,
    paste_fold_chars: usize,
    cron_tick_secs: u64,
    cron_budget_secs: u64,
    cron_runs_kept: usize,
    cron_catchup_secs: u64,
    cron_fail_pause: u32,
    memory_bytes: usize,
    gateway_poll_secs: u64,
    gateway_pair_secs: u64,
    gateway_port: u16,
    gateway_body_bytes: usize,
    gateway_fire_secs: u64,
    gateway_head_secs: u64,
    gateway_read_secs: u64,
    gateway_stop_secs: u64,
}
