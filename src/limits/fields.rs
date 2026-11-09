use serde::Deserialize;

use crate::endpoint::CacheTtl;

macro_rules! table {
    ($($name:ident: $ty:ty $(=> $(#[$meta:meta])* $getter:ident($unit:ident))?),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct Limits {
            $(pub $name: $ty,)*
        }

        #[derive(Clone, Debug, Default, Deserialize)]
        pub(crate) struct Overrides {
            $(pub $name: Option<$ty>,)*
            #[serde(flatten)]
            pub other: std::collections::BTreeMap<String, toml::Value>,
        }

        impl Limits {
            pub(crate) fn overlay(mut self, with: &Overrides) -> Self {
                $(self.$name = with.$name.unwrap_or(self.$name);)*
                self
            }

            $($(
                $(#[$meta])*
                pub(crate) fn $getter(&self) -> std::time::Duration {
                    super::durations::$unit(self.$name)
                }
            )?)*
        }
    };
}

table! {
    cache_ttl: CacheTtl,
    compact_keep_tail: usize,
    compact_reserve_tokens: u32,
    compact_tail_tokens: u32,
    ledger_max: usize,
    judge_repairs: usize,
    prompt_sessions: usize,
    retry_limit: u32,
    retry_base_ms: u64 => retry_base(millis),
    retry_max_ms: u64 => retry_max(millis),
    retry_after_max_secs: u64 => retry_after_max(secs),
    stream_idle_secs: u64 => stream_idle(secs),
    stream_total_secs: u64 => stream_total(secs),
    connect_secs: u64 => connect(secs),
    compact_percent: usize,
    elide_percent: usize,
    compact_chars: usize,
    clear_percent: usize,
    clear_tokens: usize,
    clear_keep: usize,
    clear_at_least_percent: usize,
    summary_retry_rounds: usize,
    job_wait_secs: u64 => job_wait(secs),
    command_wait_secs: u64,
    command_kill_secs: u64,
    agent_park_secs: u64 => agent_park(secs),
    grant_wait_secs: u64 => grant_wait(secs),
    job_poll_start_secs: u64,
    job_poll_cap_secs: u64,
    mail_stale_secs: u64,
    mail_sweep_secs: u64,
    mail_per_round: usize,
    verify_timeout_secs: u64,
    probe_secs: u64 => probe(secs),
    auth_secs: u64 => auth(secs),
    session_lock_ms: u64 => session_lock(millis),
    update_secs: u64 => update(secs),
    model_probe_secs: u64 => model_probe(secs),
    web_search_secs: u64 => web_search(secs),
    search_secs: u64 => search(secs),
    search_index_secs: u64,
    models_catalog_secs: u64,
    models_listing_secs: u64,
    search_index_after_ms: u64,
    web_connect_secs: u64 => web_connect(secs),
    web_fetch_secs: u64 => web_fetch(secs),
    web_body_bytes: usize,
    web_redirect_max: usize,
    keychain_ms: u64 => #[cfg(any(target_os = "macos", all(test, unix)))] keychain(millis),
    keychain_poll_ms: u64 => #[cfg(any(target_os = "macos", all(test, unix)))] keychain_poll(millis),
    mcp_open_secs: u64 => mcp_open(secs),
    mcp_call_secs: u64 => mcp_call(secs),
    mcp_idle_secs: u64 => mcp_idle(secs),
    mcp_progress_stretch: u32,
    mcp_cancel_write_secs: u64 => mcp_cancel_write(secs),
    mcp_close_grace_ms: u64 => mcp_close_grace(millis),
    image_side_px: u32,
    image_bytes: usize,
    mcp_log_max_bytes: u64,
    mcp_frame_max_bytes: usize,
    lsp_frame_max_bytes: usize,
    agent_line_max_bytes: usize,
    mcp_stderr_tail_bytes: usize,
    hook_output_max_bytes: usize,
    gateway_stderr_tail_lines: usize,
    mcp_stderr_settle_secs: u64 => mcp_stderr_settle(secs),
    mcp_defer_bytes: usize,
    mcp_found_max: usize,
    lsp_start_secs: u64 => lsp_start(secs),
    lsp_idle_secs: u64 => lsp_idle(secs),
    servers_max: usize,
    closest_percent: usize,
    typo_min_chars: usize,
    diff_split_min_cols: u16,
    lsp_request_secs: u64 => lsp_request(secs),
    lsp_check_secs: u64 => lsp_check(secs),
    log_retain_days: u64 => log_retain(days),
    session_retain_days: u64 => session_retain(days),
    session_prune_secs: u64,
    job_retain_days: u64 => job_retain(days),
    speed_window_secs: u64 => speed_window(secs),
    reader_window_ms: u64 => reader_window(millis),
    stamp_settle_ms: u64 => stamp_settle(millis),
    frame_window_secs: u64 => frame_window(secs),
    animation_ms: u64 => animation(millis),
    wake_margin_ms: u64 => wake_margin(millis),
    spinner_ms: u64 => spinner(millis),
    splash_reveal_ms: u64,
    status_refresh_secs: u64 => status_refresh(secs),
    beat_secs: u64 => beat(secs),
    notice_secs: u64 => notice(secs),
    worker_stop_secs: u64 => #[cfg(feature = "gui")] worker_stop(secs),
    repeat_window_secs: u64 => repeat_window(secs),
    step_attempts: u16,
    repair_limit: usize,
    repeat_nudge: usize,
    repeat_cycle_max: usize,
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
    output_line_bytes: usize,
    search_line_bytes: usize,
    gateway_poll_secs: u64,
    gateway_pair_secs: u64,
    gateway_port: u16,
    gateway_body_bytes: usize,
    gateway_fire_secs: u64,
    gateway_head_secs: u64,
    gateway_read_secs: u64,
    gateway_stop_secs: u64,
    job_child_rows: usize,
    job_child_width: usize,
    classify_task_chars: usize,
    classify_closing_chars: usize,
    classify_command_chars: usize,
    classify_errors_max: usize,
    classify_commands_max: usize,
    classify_files_max: usize,
}
