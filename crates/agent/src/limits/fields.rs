use std::time::Duration;

use serde::Deserialize;

use crate::endpoint::CacheTtl;

macro_rules! table {
    ($($name:ident: $ty:ty $(=> $(#[$meta:meta])* $getter:ident($unit:path))?),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct Limits {
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
                pub fn $getter(&self) -> Duration {
                    $unit(self.$name)
                }
            )?)*
        }
    };
}

fn days(value: u64) -> Duration {
    Duration::from_secs(value.saturating_mul(24 * 60 * 60))
}

table! {
    cache_ttl: CacheTtl,
    compact_keep_tail: usize,
    compact_reserve_tokens: u32,
    compact_tail_tokens: u32,
    ledger_max: usize,
    judge_repairs: usize,
    stall_rounds: usize,
    judge_turns: bool,
    job_quiet_secs: u64 => job_quiet(Duration::from_secs),
    prompt_sessions: usize,
    queue_rows: usize,
    retry_limit: u32,
    retry_base_ms: u64 => retry_base(Duration::from_millis),
    retry_max_ms: u64 => retry_max(Duration::from_millis),
    retry_after_max_secs: u64 => retry_after_max(Duration::from_secs),
    stream_idle_secs: u64 => stream_idle(Duration::from_secs),
    stream_total_secs: u64 => stream_total(Duration::from_secs),
    connect_secs: u64 => connect(Duration::from_secs),
    compact_percent: usize,
    elide_percent: usize,
    compact_chars: usize,
    clear_percent: usize,
    clear_tokens: usize,
    clear_keep: usize,
    clear_at_least_percent: usize,
    summary_retry_rounds: usize,
    stop_hook_streak: usize,
    job_wait_secs: u64 => job_wait(Duration::from_secs),
    command_wait_secs: u64 => command_wait(Duration::from_secs),
    tool_yield_secs: u64 => tool_yield(Duration::from_secs),
    command_kill_secs: u64 => command_kill(Duration::from_secs),
    agent_park_secs: u64 => agent_park(Duration::from_secs),
    grant_wait_secs: u64 => grant_wait(Duration::from_secs),
    job_poll_start_secs: u64 => job_poll_start(Duration::from_secs),
    job_poll_cap_secs: u64 => job_poll_cap(Duration::from_secs),
    mail_poll_ms: u64 => mail_poll(Duration::from_millis),
    watch_quiet_ms: u64 => watch_quiet(Duration::from_millis),
    watch_settle_ms: u64 => watch_settle(Duration::from_millis),
    session_lock_ms: u64 => session_lock(Duration::from_millis),
    session_takeover_ms: u64 => session_takeover(Duration::from_millis),
    strip_rows: usize,
    mail_sweep_secs: u64 => mail_sweep(Duration::from_secs),
    presence_secs: u64 => presence(Duration::from_secs),
    presence_tool_ms: u64 => presence_tool(Duration::from_millis),
    mail_per_round: usize,
    verify_timeout_secs: u64 => verify_timeout(Duration::from_secs),
    probe_secs: u64 => probe(Duration::from_secs),
    auth_secs: u64 => auth(Duration::from_secs),
    gh_secs: u64 => gh(Duration::from_secs),
    gh_poll_ms: u64 => gh_poll(Duration::from_millis),
    update_secs: u64 => update(Duration::from_secs),
    model_probe_secs: u64 => model_probe(Duration::from_secs),
    web_search_secs: u64 => web_search(Duration::from_secs),
    search_secs: u64 => search(Duration::from_secs),
    search_index_secs: u64 => search_index(Duration::from_secs),
    models_catalog_secs: u64 => models_catalog(Duration::from_secs),
    models_listing_secs: u64 => models_listing(Duration::from_secs),
    search_index_after_ms: u64 => search_index_after(Duration::from_millis),
    web_connect_secs: u64 => web_connect(Duration::from_secs),
    web_fetch_secs: u64 => web_fetch(Duration::from_secs),
    web_body_bytes: usize,
    web_redirect_max: usize,
    keychain_ms: u64 => #[cfg(any(target_os = "macos", all(test, unix)))] keychain(Duration::from_millis),
    keychain_poll_ms: u64 => #[cfg(any(target_os = "macos", all(test, unix)))] keychain_poll(Duration::from_millis),
    mcp_open_secs: u64 => mcp_open(Duration::from_secs),
    mcp_call_secs: u64 => mcp_call(Duration::from_secs),
    mcp_idle_secs: u64 => mcp_idle(Duration::from_secs),
    mcp_progress_stretch: u32,
    mcp_cancel_write_secs: u64 => mcp_cancel_write(Duration::from_secs),
    mcp_close_grace_ms: u64 => mcp_close_grace(Duration::from_millis),
    image_side_px: u32,
    image_bytes: usize,
    mcp_log_max_bytes: u64,
    mcp_instructions_bytes: usize,
    mcp_frame_max_bytes: usize,
    lsp_frame_max_bytes: usize,
    agent_line_max_bytes: usize,
    mcp_stderr_tail_bytes: usize,
    hook_output_max_bytes: usize,
    panels_max: usize,
    panel_output_bytes: usize,
    panel_timeout_secs: u64 => panel_timeout(Duration::from_secs),
    panel_every_min_secs: u64 => panel_every_min(Duration::from_secs),
    panel_poll_secs: u64 => panel_poll(Duration::from_secs),
    gateway_stderr_tail_lines: usize,
    mcp_stderr_settle_secs: u64 => mcp_stderr_settle(Duration::from_secs),
    mcp_defer_bytes: usize,
    mcp_found_max: usize,
    lsp_start_secs: u64 => lsp_start(Duration::from_secs),
    lsp_idle_secs: u64 => lsp_idle(Duration::from_secs),
    servers_max: usize,
    closest_percent: usize,
    typo_min_chars: usize,
    diff_split_min_cols: u16,
    lsp_request_secs: u64 => lsp_request(Duration::from_secs),
    lsp_check_secs: u64 => lsp_check(Duration::from_secs),
    log_retain_days: u64 => log_retain(days),
    session_retain_days: u64 => session_retain(days),
    session_prune_secs: u64 => session_prune(Duration::from_secs),
    job_retain_days: u64 => job_retain(days),
    speed_window_secs: u64 => speed_window(Duration::from_secs),
    reader_window_ms: u64 => reader_window(Duration::from_millis),
    stamp_settle_ms: u64 => stamp_settle(Duration::from_millis),
    thread_keep_alive_ms: u64 => thread_keep_alive(Duration::from_millis),
    frame_window_secs: u64 => frame_window(Duration::from_secs),
    animation_ms: u64 => animation(Duration::from_millis),
    wake_margin_ms: u64 => wake_margin(Duration::from_millis),
    spinner_ms: u64 => spinner(Duration::from_millis),
    splash_reveal_ms: u64 => splash_reveal(Duration::from_millis),
    status_refresh_secs: u64 => status_refresh(Duration::from_secs),
    beat_secs: u64 => beat(Duration::from_secs),
    notice_secs: u64 => notice(Duration::from_secs),
    worker_stop_secs: u64 => worker_stop(Duration::from_secs),
    local_probe_ms: u64 => local_probe(Duration::from_millis),
    repeat_window_secs: u64 => repeat_window(Duration::from_secs),
    step_attempts: u16,
    repair_limit: usize,
    repeat_nudge: usize,
    repeat_cycle_max: usize,
    edit_hunk_rows: usize,
    display_patch_lines: usize,
    read_line_limit: usize,
    paste_fold_lines: usize,
    paste_fold_chars: usize,
    cron_tick_secs: u64 => cron_tick(Duration::from_secs),
    cron_budget_secs: u64 => cron_budget(Duration::from_secs),
    cron_runs_kept: usize,
    cron_catchup_secs: u64 => cron_catchup(Duration::from_secs),
    cron_fail_pause: u32,
    memory_bytes: usize,
    output_line_bytes: usize,
    search_line_bytes: usize,
    gateway_poll_secs: u64 => gateway_poll(Duration::from_secs),
    gateway_pair_secs: u64 => gateway_pair(Duration::from_secs),
    gateway_port: u16,
    port_base: u16,
    port_block: u16,
    gateway_body_bytes: usize,
    gateway_fire_secs: u64 => gateway_fire(Duration::from_secs),
    gateway_head_secs: u64 => gateway_head(Duration::from_secs),
    gateway_read_secs: u64 => gateway_read(Duration::from_secs),
    gateway_stop_secs: u64 => gateway_stop(Duration::from_secs),
    job_child_rows: usize,
    job_child_width: usize,
    classify_task_chars: usize,
    classify_closing_chars: usize,
    classify_command_chars: usize,
    classify_errors_max: usize,
    classify_commands_max: usize,
    classify_files_max: usize,
}
