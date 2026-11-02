---
name: herdr
description: Use when the user asks to control Herdr terminals, agents, panes, tabs, spaces, layouts, or mods.
---

# Herdr

Use the installed Herdr CLIs through `run_command`. Summonr's terminal UI owns agent-state reporting; do not install a second status hook.

- For starting, reading, messaging, or waiting on terminals and agents, load `hc --skill` and use `hc`.
- For visible spaces, tabs, panes, focus, layouts, or mods, load `herdr-app --skill` and use `herdr-app`.
- If the required CLI is unavailable, report that limitation. Do not substitute the older `herdr` terminal multiplexer.

Use the installed command reference rather than a bundled copy. Inspect the target first and use its returned ID. A terminal address such as `$HC_TERMINAL_ID` is not a pane ID.

Read a terminal before sending input. Use `hc send` for an agent message and `hc wait` for readiness or completion. Preserve other terminals and running work. Starting another agent or paid run requires the user's authorization.

Use `herdr-app ls` and `herdr-app status` before changing the window. Create visible spaces, tabs, or splits only when requested. For layout or mod changes, read the installed app guide and generated API declarations first. Do not edit shipped mods or generated files directly.

Inspect the result with the corresponding list, read, or status command. Do not restart the user's app or core to test an integration.
