#[path = "mcp/content.rs"]
mod content;
#[path = "mcp/defer.rs"]
mod defer;
#[path = "mcp/diff.rs"]
mod diff;
#[path = "mcp/lifecycle.rs"]
mod lifecycle;
#[path = "mcp/manage.rs"]
mod manage;
#[path = "mcp/output.rs"]
mod output;
#[path = "mcp/plugin_install.rs"]
mod plugin_install;
#[path = "mcp/plugins.rs"]
mod plugins;
#[path = "mcp/policy.rs"]
mod policy;
#[path = "mcp/protocol.rs"]
mod protocol;
#[cfg(unix)]
#[path = "mcp/search_index.rs"]
mod search_index;
#[cfg(unix)]
#[path = "mcp/secrets.rs"]
mod secrets;
#[path = "mcp/session.rs"]
mod session;
mod support;
#[path = "mcp/transport.rs"]
mod transport;

macro_rules! proofs {
    ($($(#[$cfg:meta])* $module:ident::$name:ident),* $(,)?) => {
        &[$($(#[$cfg])* (stringify!($name), $module::$name as fn())),*]
    };
}

fn main() {
    if std::env::var_os("MCP_FIXTURE_SERVE").is_some() {
        support::fixture::main();
        return;
    }
    support::harness(proofs![
        content::a_server_prompt_becomes_the_user_message,
        content::a_prompt_without_its_required_argument_is_refused,
        content::a_server_resource_is_attached_to_the_prompt,
        content::a_changed_prompt_list_is_fetched_again,
        content::a_form_is_declined_without_a_user,
        defer::a_large_server_is_found_and_called_through_the_mcp_tool,
        defer::a_hook_sees_the_tool_behind_the_mcp_tool,
        defer::arguments_given_beside_the_tool_name_reach_the_server,
        lifecycle::a_configured_server_is_on_by_default_and_starts_only_when_called,
        lifecycle::an_idle_server_stops_and_starts_again_on_the_next_call,
        lifecycle::the_least_recently_used_idle_server_stops_at_the_server_cap,
        lifecycle::keep_alive_servers_count_toward_the_server_cap,
        lifecycle::tool_lists_are_cached_per_workspace,
        lifecycle::an_empty_cached_tool_list_is_fetched_again,
        lifecycle::an_unchanged_tool_list_is_not_rewritten,
        lifecycle::a_tool_list_that_never_ends_is_refused,
        lifecycle::a_server_that_never_answers_is_given_up_at_its_timeout,
        #[cfg(unix)]
        lifecycle::a_server_behind_a_wrapper_leaves_no_process_behind,
        #[cfg(unix)]
        lifecycle::ctrl_c_stops_the_servers,
        manage::servers_are_added_listed_described_and_removed_from_the_shell,
        #[cfg(unix)]
        plugins::a_claude_code_plugin_installs_and_every_part_loads,
        #[cfg(unix)]
        plugins::a_plugin_command_expands_into_the_prompt_and_its_agents_are_listed,
        #[cfg(unix)]
        plugins::an_uninstalled_or_disabled_plugin_does_not_load,
        #[cfg(unix)]
        plugins::a_claude_hook_blocks_only_on_exit_two_or_a_deny,
        plugins::two_pre_tool_hooks_both_run_in_order,
        #[cfg(unix)]
        plugins::a_chatty_hook_does_not_stall_the_call,
        #[cfg(unix)]
        plugins::plugin_settings_and_data_are_handled,
        plugins::a_legacy_bundle_installs_and_an_unreachable_server_fails_it,
        plugin_install::plugin_install_refuses_a_tree_that_adds_nothing,
        plugin_install::plugin_install_keeps_the_copy_when_json_does_not_parse,
        plugin_install::plugin_reinstall_keeps_plugin_data,
        plugin_install::plugin_unknown_off_warns_without_blocking_list_or_session_start,
        plugin_install::plugin_uninstall_removes_an_unknown_off_name,
        plugin_install::plugin_install_skips_a_server_turned_off_in_config,
        plugin_install::plugin_install_counts_only_language_servers_that_load,
        #[cfg(unix)]
        plugin_install::plugin_user_config_reaches_the_server,
        #[cfg(unix)]
        plugin_install::plugin_user_env_replaces_the_server_env,
        #[cfg(unix)]
        plugin_install::plugin_non_command_hook_is_skipped_and_does_not_run,
        #[cfg(unix)]
        plugins::a_claude_dialect_hook_from_config_blocks_and_sees_summonr_home,
        #[cfg(unix)]
        plugins::a_hook_matcher_limits_the_tools_it_runs_for,
        #[cfg(unix)]
        plugins::a_claude_hook_sees_a_name_search_as_glob_and_a_content_search_as_grep,
        output::large_output_is_cut_for_the_model_and_kept_whole_in_a_file,
        output::a_reply_over_the_frame_cap_is_dropped_and_the_session_goes_on,
        output::an_error_reply_is_cut_to_the_budget,
        output::output_that_cannot_be_saved_is_still_cut_and_returned,
        output::an_image_reaches_a_model_that_takes_images,
        output::an_image_is_withheld_from_a_model_without_images,
        output::an_image_whose_data_does_not_match_its_type_is_withheld,
        output::a_server_error_line_is_shown_without_secrets,
        output::a_json_run_starts_with_its_session_id,
        diff::turn_last_shows_the_latest_turn_of_a_resumed_session_without_generated_files,
        diff::a_turn_diff_names_its_session,
        session::a_plain_worktree_turn_lands_its_changes_without_running_a_check,
        session::a_failed_worktree_run_without_changes_says_so,
        session::a_goal_run_reports_the_judge_usage_once,
        session::a_goal_run_whose_output_lands_in_the_workspace_still_passes,
        session::a_print_run_says_its_check_was_not_run,
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        session::a_failing_grep_check_does_not_complete_a_goal,
        session::forking_a_named_session_keeps_the_original_transcript,
        session::a_budget_abort_runs_the_failed_check_before_choosing_its_exit_code,
        policy::strict_mode_denies_a_listed_tool_and_runs_an_allowed_one,
        policy::strict_mode_without_an_allow_entry_refuses_a_tool_that_changes_state,
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        policy::a_command_allow_list_does_not_block_mcp_tools_outside_strict_mode,
        policy::tools_outside_the_allowlist_are_not_advertised,
        policy::an_unapproved_project_server_is_named,
        policy::a_tuning_table_keeps_a_project_server_waiting,
        policy::a_zero_timeout_is_refused,
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        policy::a_relocated_summonr_home_stays_protected_from_read_and_search,
        #[cfg(unix)]
        secrets::secret_values_never_reach_the_model_but_commands_still_load_them,
        #[cfg(unix)]
        search_index::a_name_search_outside_the_workspace_answers_from_the_index_and_checks_the_disk,
        protocol::the_client_offers_the_current_spec_with_roots,
        protocol::a_server_on_an_unknown_version_is_refused,
        protocol::the_server_can_ask_for_the_workspace_roots,
        protocol::a_changed_tool_list_is_fetched_again,
        protocol::progress_reaches_the_event_stream,
        protocol::progress_keeps_a_slow_call_alive,
        protocol::server_log_messages_go_to_the_log_file,
        protocol::an_aborted_call_is_cancelled,
        transport::a_call_with_no_reply_is_cancelled_at_the_timeout,
        transport::a_timed_out_reply_does_not_break_the_next_call,
        transport::a_request_the_server_does_not_read_times_out,
        transport::a_remote_server_reconnects_after_its_session_expires,
        transport::a_remote_reply_on_a_stream_held_open_returns_at_once,
    ]);
}
