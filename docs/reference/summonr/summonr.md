# Module `summonr`

| Item | Kind | Description |
| --- | --- | --- |
| [`AuthInfo`](#authinfo) | struct |  |
| [`Budget`](#budget) | struct |  |
| [`Client`](#client) | struct |  |
| [`Endpoint`](#endpoint) | struct |  |
| [`Loop`](#loop) | struct |  |
| [`McpChoice`](#mcpchoice) | struct |  |
| [`McpField`](#mcpfield) | struct |  |
| [`McpForm`](#mcpform) | struct |  |
| [`McpFormAnswer`](#mcpformanswer) | struct |  |
| [`Receipt`](#receipt) | struct |  |
| [`ServerStatus`](#serverstatus) | struct |  |
| [`Sources`](#sources) | struct |  |
| [`ToolResult`](#toolresult) | struct |  |
| [`Tree`](#tree) | struct |  |
| [`Turn`](#turn) | struct |  |
| [`Usage`](#usage) | struct |  |
| [`UserInput`](#userinput) | struct |  |
| [`WorkProgress`](#workprogress) | struct |  |
| [`AbortCode`](#abortcode) | enum |  |
| [`Effort`](#effort) | enum |  |
| [`EndpointNear`](#endpointnear) | enum |  |
| [`EndpointState`](#endpointstate) | enum |  |
| [`Error`](#error) | enum |  |
| [`Event`](#event) | enum |  |
| [`LoginFlow`](#loginflow) | enum |  |
| [`McpFieldKind`](#mcpfieldkind) | enum |  |
| [`McpFormAction`](#mcpformaction) | enum |  |
| [`McpLaunch`](#mcplaunch) | enum |  |
| [`McpState`](#mcpstate) | enum |  |
| [`McpTextFormat`](#mcptextformat) | enum |  |
| [`McpTransport`](#mcptransport) | enum |  |
| [`Need`](#need) | enum |  |
| [`Network`](#network) | enum |  |
| [`Pick`](#pick) | enum |  |
| [`ProfileScope`](#profilescope) | enum |  |
| [`ProviderFailureKind`](#providerfailurekind) | enum |  |
| [`RunMode`](#runmode) | enum |  |
| [`SlashCommand`](#slashcommand) | enum |  |
| [`StepRun`](#steprun) | enum |  |
| [`WorkKind`](#workkind) | enum |  |
| [`add_endpoint`](#add_endpoint) | fn |  |
| [`add_worktree`](#add_worktree) | fn |  |
| [`adopt_keys`](#adopt_keys) | fn |  |
| [`attach`](#attach) | fn |  |
| [`auth_info`](#auth_info) | fn |  |
| [`check_commands`](#check_commands) | fn |  |
| [`check_config`](#check_config) | fn |  |
| [`check_key`](#check_key) | fn |  |
| [`classify_endpoint`](#classify_endpoint) | fn |  |
| [`closest_endpoint`](#closest_endpoint) | fn |  |
| [`command_specs`](#command_specs) | fn |  |
| [`default_effort`](#default_effort) | fn |  |
| [`endpoint_chosen_by`](#endpoint_chosen_by) | fn |  |
| [`endpoint_fallbacks`](#endpoint_fallbacks) | fn |  |
| [`endpoint_label`](#endpoint_label) | fn |  |
| [`endpoint_names`](#endpoint_names) | fn |  |
| [`endpoint_names_ordered`](#endpoint_names_ordered) | fn |  |
| [`endpoint_set_up`](#endpoint_set_up) | fn |  |
| [`export_session`](#export_session) | fn |  |
| [`finish_worktree`](#finish_worktree) | fn |  |
| [`goal_prompt`](#goal_prompt) | fn |  |
| [`in_git`](#in_git) | fn |  |
| [`index_files`](#index_files) | fn |  |
| [`install_plugin`](#install_plugin) | fn |  |
| [`interrupted`](#interrupted) | fn |  |
| [`is_endpoint`](#is_endpoint) | fn |  |
| [`last_for`](#last_for) | fn |  |
| [`list_plugins`](#list_plugins) | fn |  |
| [`list_sessions`](#list_sessions) | fn |  |
| [`list_worktrees`](#list_worktrees) | fn |  |
| [`live_jobs`](#live_jobs) | fn |  |
| [`lock_file`](#lock_file) | fn |  |
| [`login_seat`](#login_seat) | fn |  |
| [`logout`](#logout) | fn |  |
| [`mcp_add`](#mcp_add) | fn |  |
| [`mcp_call_label`](#mcp_call_label) | fn |  |
| [`mcp_probe`](#mcp_probe) | fn |  |
| [`mcp_remove`](#mcp_remove) | fn |  |
| [`mcp_status`](#mcp_status) | fn |  |
| [`parse_command`](#parse_command) | fn |  |
| [`profile`](#profile) | fn |  |
| [`prune_sessions`](#prune_sessions) | fn |  |
| [`remove_endpoint`](#remove_endpoint) | fn |  |
| [`resolve_endpoint`](#resolve_endpoint) | fn |  |
| [`resolve_session`](#resolve_session) | fn |  |
| [`revision`](#revision) | fn |  |
| [`run_tool`](#run_tool) | fn |  |
| [`sandbox_on`](#sandbox_on) | fn |  |
| [`set_default_endpoint`](#set_default_endpoint) | fn |  |
| [`short`](#short) | fn |  |
| [`stop_children`](#stop_children) | fn |  |
| [`summonr_home`](#summonr_home) | fn |  |
| [`sweep_worktrees`](#sweep_worktrees) | fn |  |
| [`system_prompt`](#system_prompt) | fn |  |
| [`tilde`](#tilde) | fn |  |
| [`uninstall_plugin`](#uninstall_plugin) | fn |  |
| [`vision_endpoint`](#vision_endpoint) | fn |  |
| [`worktree_status`](#worktree_status) | fn |  |
| [`McpBoard`](#mcpboard) | type |  |
| [`Result`](#result) | type |  |
| [`INDEX_ENTRY`](#index_entry) | const |  |

## Structs

### `AuthInfo`

**Methods**

| Method | Description |
| --- | --- |
| <code>catalog_line(&amp;self, name: usize, state: usize) -&gt; String</code> |  |
| <code>catalog_rank(&amp;self) -&gt; u8</code> |  |
| <code>catalog_title(&amp;self) -&gt; &amp;'static str</code> |  |
| <code>display_name(&amp;self) -&gt; String</code> |  |
| <code>dual(&amp;self) -&gt; bool</code> |  |
| <code>keyless(&amp;self) -&gt; bool</code> |  |
| <code>plan(&amp;self) -&gt; bool</code> |  |
| <code>state(&amp;self) -&gt; String</code> |  |
| <code>status(&amp;self) -&gt; String</code> |  |
| <code>stored(&amp;self) -&gt; bool</code> |  |
| <code>unmodeled(&amp;self) -&gt; bool</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>base_url: Option&lt;String&gt;</code> |  |
| <code>configured: bool</code> |  |
| <code>env: Option&lt;&amp;'static str&gt;</code> |  |
| <code>flow: <a href="#loginflow">LoginFlow</a></code> |  |
| <code>key: bool</code> |  |
| <code>login: <a href="#loginflow">LoginFlow</a></code> |  |
| <code>model: String</code> |  |
| <code>name: String</code> |  |
| <code>own: bool</code> |  |
| <code>plan: String</code> |  |
| <code>setup: String</code> |  |
| <code>source: Option&lt;<a href="summonr-secret.html#source">Source</a>&gt;</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `Budget`

**Methods**

| Method | Description |
| --- | --- |
| <code>none() -&gt; Self</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>max_secs: Option&lt;u64&gt;</code> |  |
| <code>max_tokens: Option&lt;u64&gt;</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `Client`

**Methods**

| Method | Description |
| --- | --- |
| <code>async classify(&amp;self, model: &amp;str, state: &amp;Value, questions: &amp;BTreeMap&lt;String, <a href="summonr-classify.html#question">Question</a>&gt;) -&gt; <a href="#result">Result</a>&lt;<a href="summonr-classify.html#decision">Decision</a>&gt;</code> |  |
| <code>connect(endpoint: &amp;<a href="#endpoint">Endpoint</a>) -&gt; <a href="#result">Result</a>&lt;Self&gt;</code> |  |
| <code>new(api_key: impl Into&lt;String&gt;, base_url: impl Into&lt;String&gt;) -&gt; <a href="#result">Result</a>&lt;Self&gt;</code> |  |
| <code>on_delta(self, emit: impl Fn(&amp;str) + Send + Sync + 'static) -&gt; Self</code> |  |
| <code>on_reasoning(self, emit: impl Fn(&amp;str) + Send + Sync + 'static) -&gt; Self</code> |  |
| <code>on_tool(self, emit: impl Fn(&amp;str) + Send + Sync + 'static) -&gt; Self</code> |  |
| <code>async context(&amp;self, endpoint: &amp;str, model: &amp;str) -&gt; Option&lt;u32&gt;</code> |  |
| <code>async discover(endpoint: &amp;<a href="#endpoint">Endpoint</a>) -&gt; <a href="#result">Result</a>&lt;Vec&lt;<a href="summonr-catalog.html#model">Model</a>&gt;&gt;</code> |  |
| <code>state(error: &amp;<a href="#error">Error</a>) -&gt; <a href="#endpointstate">EndpointState</a></code> |  |
| <code>async login(endpoint: &amp;<a href="#endpoint">Endpoint</a>, prompt: &amp;mut Prompt) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |

**Trait implementations:** <code>Clone</code>


### `Endpoint`

**Methods**

| Method | Description |
| --- | --- |
| <code>keyless(&amp;self) -&gt; bool</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>api_key: Option&lt;String&gt;</code> |  |
| <code>assistant_after_tool: bool</code> |  |
| <code>base_url: Option&lt;String&gt;</code> |  |
| <code>body: Option&lt;Value&gt;</code> |  |
| <code>cache_header: Option&lt;String&gt;</code> |  |
| <code>cache_ttl: endpoint::CacheTtl</code> |  |
| <code>compact_tokens: Option&lt;u32&gt;</code> |  |
| <code>context_window: Option&lt;u32&gt;</code> |  |
| <code>effort: Option&lt;<a href="#effort">Effort</a>&gt;</code> |  |
| <code>env: BTreeMap&lt;String, String&gt;</code> |  |
| <code>fallback: Vec&lt;String&gt;</code> |  |
| <code>headers: BTreeMap&lt;String, String&gt;</code> |  |
| <code>idle_timeout_secs: Option&lt;u64&gt;</code> |  |
| <code>images: Option&lt;bool&gt;</code> |  |
| <code>keep_reasoning: bool</code> |  |
| <code>model: String</code> |  |
| <code>models_url: Option&lt;String&gt;</code> |  |
| <code>name: String</code> |  |
| <code>oauth: Option&lt;config::OAuth&gt;</code> |  |
| <code>opencode: bool</code> |  |
| <code>price: Option&lt;provider::request::Price&gt;</code> |  |
| <code>prompt_cache_key: bool</code> |  |
| <code>source: Option&lt;<a href="summonr-secret.html#source">Source</a>&gt;</code> |  |
| <code>wire: endpoint::Wire</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `Loop`

**Methods**

| Method | Description |
| --- | --- |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::abandon_work(&amp;mut self, reason: &amp;str) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::goal_at(path: &amp;Path, workspace: &amp;Path, objective: &amp;str) -&gt; bool</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::job_at(path: &amp;Path, workspace: &amp;Path) -&gt; Option&lt;(<a href="#workkind">WorkKind</a>, String)&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::select_goal(&amp;mut self, objective: &amp;str, commands: &amp;[String]) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::select_steps(&amp;mut self, markdown: &amp;str, commands: &amp;[String]) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::steps_at(path: &amp;Path, workspace: &amp;Path, markdown: &amp;str) -&gt; bool</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::touched_at(path: &amp;Path) -&gt; Vec&lt;String&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::workspace(&amp;self) -&gt; &amp;Path</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::allow_path(&amp;mut self, path: &amp;str) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::budget(&amp;self) -&gt; &amp;<a href="#budget">Budget</a></code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::context_window(&amp;self) -&gt; Option&lt;u32&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::effort(&amp;self) -&gt; Option&lt;<a href="#effort">Effort</a>&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::fork_path(provider: P, model: impl Into&lt;String&gt;, workspace: impl AsRef&lt;Path&gt;, system_prompt: impl Into&lt;String&gt;, source: &amp;Path) -&gt; <a href="#result">Result</a>&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::mode(&amp;self) -&gt; <a href="#runmode">RunMode</a></code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::model(&amp;self) -&gt; &amp;str</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::new(provider: P, model: impl Into&lt;String&gt;, workspace: impl AsRef&lt;Path&gt;, system_prompt: impl Into&lt;String&gt;) -&gt; <a href="#result">Result</a>&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::policy_inert(&amp;self) -&gt; bool</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::resume_path(provider: P, model: impl Into&lt;String&gt;, workspace: impl AsRef&lt;Path&gt;, system_prompt: impl Into&lt;String&gt;, path: &amp;Path) -&gt; <a href="#result">Result</a>&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_context_probe(&amp;mut self, probe: JoinHandle&lt;Option&lt;u32&gt;&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_context_window(&amp;mut self, window: Option&lt;u32&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_effort(&amp;mut self, effort: Option&lt;<a href="#effort">Effort</a>&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_env(&amp;mut self, env: impl IntoIterator&lt;Item = (String, String)&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_images(&amp;mut self, model_images: bool, vision: Option&lt;<a href="#endpoint">Endpoint</a>&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_modules(&amp;mut self, switches: &amp;<a href="summonr-modules.html#switches">Switches</a>) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::verify_commands(&amp;self) -&gt; &amp;[String]</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_interactive(self) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_sandbox(self, network: <a href="#network">Network</a>) -&gt; <a href="#result">Result</a>&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_strict(self) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_unrestricted_commands(self) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::apply_workflow(&amp;mut self, workflow: &amp;<a href="summonr-workflow.html#workflow">Workflow</a>, resolve: impl FnMut(&amp;<a href="summonr-workflow.html#seat">Seat</a>) -&gt; <a href="#result">Result</a>&lt;(P, <a href="#endpoint">Endpoint</a>)&gt;) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::history_len(&amp;self) -&gt; usize</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::name(&amp;self) -&gt; Option&lt;String&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::next_step(&amp;self, steps: &amp;[<a href="summonr-steps.html#step">Step</a>], plan_path: &amp;str) -&gt; <a href="#result">Result</a>&lt;<a href="#steprun">StepRun</a>&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::session_id(&amp;self) -&gt; String</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::session_path(&amp;self) -&gt; &amp;Path</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_compact_tokens(&amp;mut self, tokens: Option&lt;u32&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_fallbacks(&amp;mut self, fallbacks: impl IntoIterator&lt;Item = (P, String)&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_name(&amp;mut self, name: &amp;str) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_price(&amp;mut self, price: Option&lt;provider::request::Price&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_budget(self, budget: <a href="#budget">Budget</a>) -&gt; <a href="#result">Result</a>&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_verify(self, commands: Vec&lt;String&gt;) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::work_progress(&amp;self) -&gt; <a href="#result">Result</a>&lt;<a href="#workprogress">WorkProgress</a>&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::workflow(&amp;self) -&gt; Option&lt;String&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::compact(&amp;mut self) -&gt; <a href="#result">Result</a>&lt;bool&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::mcp_board(&amp;self) -&gt; <a href="#mcpboard">McpBoard</a></code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::shell(&amp;self, command: &amp;str) -&gt; <a href="#result">Result</a>&lt;(String, bool)&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::stop_work(&amp;mut self, reason: &amp;str, emit: impl FnMut(&amp;<a href="#event">Event</a>)) -&gt; <a href="#result">Result</a>&lt;i32&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::turn(&amp;mut self, prompt: &amp;str) -&gt; <a href="#result">Result</a>&lt;<a href="#turn">Turn</a>&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::turn_mail_with(&amp;mut self, emit: impl FnMut(&amp;<a href="#event">Event</a>)) -&gt; <a href="#result">Result</a>&lt;<a href="#turn">Turn</a>&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::turn_with(&amp;mut self, input: impl Into&lt;<a href="#userinput">UserInput</a>&gt;, emit: impl FnMut(&amp;<a href="#event">Event</a>)) -&gt; <a href="#result">Result</a>&lt;<a href="#turn">Turn</a>&gt;</code> |  |


### `McpChoice`

**Fields**

| Field | Description |
| --- | --- |
| <code>description: Option&lt;String&gt;</code> |  |
| <code>title: String</code> |  |
| <code>value: String</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `McpField`

**Methods**

| Method | Description |
| --- | --- |
| <code>check(&amp;self, value: &amp;Value) -&gt; Result&lt;(), String&gt;</code> |  |
| <code>read(&amp;self, typed: &amp;str) -&gt; Result&lt;Option&lt;Value&gt;, String&gt;</code> |  |
| <code>shown(value: &amp;Value) -&gt; String</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>default: Option&lt;Value&gt;</code> |  |
| <code>description: Option&lt;String&gt;</code> |  |
| <code>kind: <a href="#mcpfieldkind">McpFieldKind</a></code> |  |
| <code>name: String</code> |  |
| <code>required: bool</code> |  |
| <code>title: String</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `McpForm`

**Methods**

| Method | Description |
| --- | --- |
| <code>check(&amp;self, content: &amp;Map&lt;String, Value&gt;) -&gt; Result&lt;(), String&gt;</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>fields: Vec&lt;<a href="#mcpfield">McpField</a>&gt;</code> |  |
| <code>message: String</code> |  |
| <code>server: Option&lt;String&gt;</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `McpFormAnswer`

**Methods**

| Method | Description |
| --- | --- |
| <code>const cancelled() -&gt; Self</code> |  |
| <code>const declined() -&gt; Self</code> |  |
| <code>wire(&amp;self) -&gt; Value</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>action: <a href="#mcpformaction">McpFormAction</a></code> |  |
| <code>content: Option&lt;Map&lt;String, Value&gt;&gt;</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `Receipt`

**Fields**

| Field | Description |
| --- | --- |
| <code>aborted: bool</code> |  |
| <code>changes: Vec&lt;String&gt;</code> |  |
| <code>checks: Vec&lt;verify::Outcome&gt;</code> |  |
| <code>commands: usize</code> |  |
| <code>failures: Vec&lt;String&gt;</code> |  |
| <code>reads: usize</code> |  |
| <code>searches: usize</code> |  |
| <code>snapshot: String</code> |  |
| <code>text: String</code> |  |
| <code>verified: bool</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `ServerStatus`

**Methods**

| Method | Description |
| --- | --- |
| <code>schema_size(&amp;self) -&gt; String</code> |  |
| <code>state_text(&amp;self, width: usize) -&gt; String</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>detail: Option&lt;String&gt;</code> |  |
| <code>kind: <a href="summonr-supervise.html#serverkind">ServerKind</a></code> |  |
| <code>launch: Option&lt;String&gt;</code> |  |
| <code>name: String</code> |  |
| <code>rss_kb: Option&lt;u64&gt;</code> |  |
| <code>schema_bytes: usize</code> |  |
| <code>state: <a href="#mcpstate">McpState</a></code> |  |
| <code>tools: Vec&lt;String&gt;</code> |  |
| <code>transport: Option&lt;<a href="#mcptransport">McpTransport</a>&gt;</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Debug</code>, <code>Serialize</code>


### `Sources`

**Fields**

| Field | Description |
| --- | --- |
| <code>images: bool</code> |  |
| <code>memory: bool</code> |  |
| <code>skills: bool</code> |  |
| <code>trust: bool</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Default</code>, <code>Copy</code>


### `ToolResult`

**Fields**

| Field | Description |
| --- | --- |
| <code>success: bool</code> |  |
| <code>text: String</code> |  |


### `Tree`

**Methods**

| Method | Description |
| --- | --- |
| <code>unchanged(&amp;self) -&gt; <a href="#result">Result</a>&lt;bool&gt;</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>base: String</code> |  |
| <code>branch: String</code> |  |
| <code>path: PathBuf</code> |  |
| <code>primary: PathBuf</code> |  |
| <code>snapshot: Option&lt;String&gt;</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `Turn`

**Fields**

| Field | Description |
| --- | --- |
| <code>aborted: bool</code> |  |
| <code>checks: Vec&lt;verify::Outcome&gt;</code> |  |
| <code>events: Vec&lt;<a href="#event">Event</a>&gt;</code> |  |
| <code>rounds: usize</code> |  |
| <code>text: String</code> |  |
| <code>usage: <a href="#usage">Usage</a></code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `Usage`

**Methods**

| Method | Description |
| --- | --- |
| <code>add(&amp;mut self, other: &amp;Self)</code> |  |
| <code>clamp(count: u64) -&gt; u32</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>cache_write_tokens: u32</code> |  |
| <code>cached_tokens: u32</code> |  |
| <code>completion_tokens: u32</code> |  |
| <code>prompt_tokens: u32</code> |  |
| <code>reasoning_tokens: u32</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `UserInput`

**Fields**

| Field | Description |
| --- | --- |
| <code>images: Vec&lt;provider::message::Image&gt;</code> |  |
| <code>source: Option&lt;provider::message::Source&gt;</code> |  |
| <code>text: String</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>From&lt;&amp;str&gt;</code>, <code>From&lt;String&gt;</code>, <code>Default</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `WorkProgress`

**Methods**

| Method | Description |
| --- | --- |
| <code>exit_code(&amp;self) -&gt; Option&lt;i32&gt;</code> |  |
| <code>terminal(&amp;self) -&gt; bool</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>attempts: u16</code> |  |
| <code>checks_failed: bool</code> |  |
| <code>current: Option&lt;usize&gt;</code> |  |
| <code>kind: <a href="#workkind">WorkKind</a></code> |  |
| <code>ledger: Vec&lt;String&gt;</code> |  |
| <code>objective: String</code> |  |
| <code>phase: durable::work::progress::WorkPhase</code> |  |
| <code>reason: Option&lt;String&gt;</code> |  |
| <code>rounds: u32</code> |  |
| <code>todos: Vec&lt;tools::todo::Todo&gt;</code> |  |
| <code>total: usize</code> |  |
| <code>touched: Vec&lt;String&gt;</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Display</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


## Enums

### `AbortCode`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Cancelled</code> |  |
| <code>Error</code> |  |
| <code>Interrupted</code> |  |
| <code>TimeBudget</code> |  |
| <code>TokenBudget</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `Effort`

**Methods**

| Method | Description |
| --- | --- |
| <code>as_str(self) -&gt; &amp;'static str</code> |  |
| <code>next(self) -&gt; Self</code> |  |
| <code>parse(text: &amp;str) -&gt; Option&lt;Self&gt;</code> |  |
| <code>rejects(text: &amp;str) -&gt; String</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>High</code> |  |
| <code>Low</code> |  |
| <code>Max</code> |  |
| <code>Medium</code> |  |
| <code>Minimal</code> |  |
| <code>None</code> |  |
| <code>XHigh</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>Ord</code>, <code>PartialEq</code>, <code>PartialOrd</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `EndpointNear`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Relative</code> |  |
| <code>Typo</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `EndpointState`

**Methods**

| Method | Description |
| --- | --- |
| <code>const failed(self) -&gt; bool</code> |  |
| <code>const setup(self) -&gt; bool</code> |  |
| <code>const word(self) -&gt; &amp;'static str</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Checking</code> |  |
| <code>Error</code> |  |
| <code>KeyRejected</code> |  |
| <code>Loading</code> |  |
| <code>NeedsKey</code> |  |
| <code>NeedsModel</code> |  |
| <code>NoKey</code> |  |
| <code>Offline</code> |  |
| <code>RateLimited</code> |  |
| <code>Ready</code> |  |
| <code>SignIn</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Display</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `Error`

**Methods**

| Method | Description |
| --- | --- |
| <code>provider(kind: <a href="#providerfailurekind">ProviderFailureKind</a>, message: impl Into&lt;String&gt;) -&gt; Self</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Config(String)</code> |  |
| <code>Denied(String)</code> |  |
| <code>Io(Error)</code> |  |
| <code>Json(Error)</code> |  |
| <code>Provider(error::ProviderFailure)</code> |  |
| <code>Setup(String)</code> |  |
| <code>Tool(String)</code> |  |

**Trait implementations:** <code>From&lt;Error&gt;</code>, <code>Error</code>, <code>Debug</code>, <code>Display</code>


### `Event`

**Fields**

| Field | Description |
| --- | --- |
| <code>Aborted::code: <a href="#abortcode">AbortCode</a></code> |  |
| <code>Aborted::reason: String</code> |  |
| <code>Approval::detail: Option&lt;String&gt;</code> |  |
| <code>Approval::dialog: u64</code> |  |
| <code>Approval::persist: bool</code> |  |
| <code>Assistant::text: String</code> |  |
| <code>Child::event: Box&lt;<a href="#event">Event</a>&gt;</code> |  |
| <code>Child::id: String</code> |  |
| <code>Delta::text: String</code> |  |
| <code>Elicit::dialog: u64</code> |  |
| <code>Elicit::form: Option&lt;<a href="#mcpform">McpForm</a>&gt;</code> |  |
| <code>Mail::from: String</code> |  |
| <code>Mail::text: String</code> |  |
| <code>Merged::base: String</code> |  |
| <code>Merged::branch: String</code> |  |
| <code>Merged::files: Vec&lt;String&gt;</code> |  |
| <code>Note::text: String</code> |  |
| <code>Progress::id: String</code> |  |
| <code>Progress::message: Option&lt;String&gt;</code> |  |
| <code>Progress::progress: f64</code> |  |
| <code>Progress::total: Option&lt;f64&gt;</code> |  |
| <code>Reasoning::text: String</code> |  |
| <code>Receipt::receipt: <a href="#receipt">Receipt</a></code> |  |
| <code>Session::id: String</code> |  |
| <code>Spawned::access: <a href="summonr-child.html#access">Access</a></code> |  |
| <code>Spawned::agent: String</code> |  |
| <code>Spawned::assignment: String</code> |  |
| <code>Spawned::background: bool</code> |  |
| <code>Spawned::call: String</code> |  |
| <code>Spawned::workspace: String</code> |  |
| <code>ToolCall::arguments: String</code> |  |
| <code>ToolCall::id: String</code> |  |
| <code>ToolCall::name: String</code> |  |
| <code>ToolResult::id: String</code> |  |
| <code>ToolResult::millis: Option&lt;u64&gt;</code> |  |
| <code>ToolResult::mutated: bool</code> |  |
| <code>ToolResult::output: String</code> |  |
| <code>ToolResult::patch: Option&lt;String&gt;</code> |  |
| <code>ToolResult::success: bool</code> |  |
| <code>Unreported::tokens: u64</code> |  |
| <code>Usage::cached_tokens: u32</code> |  |
| <code>Usage::completion_tokens: u32</code> |  |
| <code>Usage::cost: Option&lt;f64&gt;</code> |  |
| <code>Usage::millis: Option&lt;u64&gt;</code> |  |
| <code>Usage::prefix_hash: String</code> |  |
| <code>Usage::prompt_tokens: u32</code> |  |
| <code>Usage::reasoning_tokens: u32</code> |  |
| <code>Verify::command: String</code> |  |
| <code>Verify::passed: bool</code> |  |
| <code>Work::progress: <a href="#workprogress">WorkProgress</a></code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Aborted</code> |  |
| <code>Approval</code> |  |
| <code>Assistant</code> |  |
| <code>Child</code> |  |
| <code>Delta</code> |  |
| <code>Elicit</code> |  |
| <code>Mail</code> |  |
| <code>Merged</code> |  |
| <code>Note</code> |  |
| <code>Progress</code> |  |
| <code>Reasoning</code> |  |
| <code>Receipt</code> |  |
| <code>Session</code> |  |
| <code>Spawned</code> |  |
| <code>ToolCall</code> |  |
| <code>ToolResult</code> |  |
| <code>Unreported</code> |  |
| <code>Usage</code> |  |
| <code>Verify</code> |  |
| <code>Work</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `LoginFlow`

**Methods**

| Method | Description |
| --- | --- |
| <code>signs_in(self) -&gt; bool</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Browser</code> |  |
| <code>Device</code> |  |
| <code>Key</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `McpFieldKind`

**Fields**

| Field | Description |
| --- | --- |
| <code>Choice::custom: bool</code> |  |
| <code>Choice::options: Vec&lt;<a href="#mcpchoice">McpChoice</a>&gt;</code> |  |
| <code>Choices::custom: bool</code> |  |
| <code>Choices::max_items: Option&lt;usize&gt;</code> |  |
| <code>Choices::min_items: Option&lt;usize&gt;</code> |  |
| <code>Choices::options: Vec&lt;<a href="#mcpchoice">McpChoice</a>&gt;</code> |  |
| <code>Number::integer: bool</code> |  |
| <code>Number::maximum: Option&lt;f64&gt;</code> |  |
| <code>Number::minimum: Option&lt;f64&gt;</code> |  |
| <code>Text::format: Option&lt;<a href="#mcptextformat">McpTextFormat</a>&gt;</code> |  |
| <code>Text::max_length: Option&lt;usize&gt;</code> |  |
| <code>Text::min_length: Option&lt;usize&gt;</code> |  |
| <code>Text::pattern: Option&lt;String&gt;</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Boolean</code> |  |
| <code>Choice</code> |  |
| <code>Choices</code> |  |
| <code>Number</code> |  |
| <code>Text</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `McpFormAction`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Accept</code> |  |
| <code>Cancel</code> |  |
| <code>Decline</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `McpLaunch`

**Fields**

| Field | Description |
| --- | --- |
| <code>Command::args: Vec&lt;String&gt;</code> |  |
| <code>Command::command: String</code> |  |
| <code>Command::env: Vec&lt;(String, String)&gt;</code> |  |
| <code>Url::headers: Vec&lt;(String, String)&gt;</code> |  |
| <code>Url::url: String</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Command</code> |  |
| <code>Url</code> |  |

**Trait implementations:** <code>Clone</code>


### `McpState`

**Methods**

| Method | Description |
| --- | --- |
| <code>const label(self) -&gt; &amp;'static str</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Cached</code> |  |
| <code>Failed</code> |  |
| <code>Live</code> |  |
| <code>Stopped</code> |  |
| <code>Unlisted</code> |  |
| <code>Waiting</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>


### `McpTextFormat`

**Methods**

| Method | Description |
| --- | --- |
| <code>const label(self) -&gt; &amp;'static str</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Date</code> |  |
| <code>DateTime</code> |  |
| <code>Email</code> |  |
| <code>Uri</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `McpTransport`

**Methods**

| Method | Description |
| --- | --- |
| <code>const label(self) -&gt; &amp;'static str</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Http</code> |  |
| <code>Stdio</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>


### `Need`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Config</code> |  |
| <code>Key</code> |  |
| <code>Model</code> |  |
| <code>Seat</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `Network`

**Methods**

| Method | Description |
| --- | --- |
| <code>const allowed(allow: bool) -&gt; Self</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Allowed</code> |  |
| <code>Denied</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `Pick`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Default</code> |  |
| <code>Env</code> |  |
| <code>Flag</code> |  |
| <code>LastUsed</code> |  |
| <code>ModelFlag</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Display</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `ProfileScope`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Recent(Option&lt;usize&gt;)</code> |  |
| <code>Session(&amp;'a Path)</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Copy</code>


### `ProviderFailureKind`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Authentication</code> |  |
| <code>ContextOverflow</code> |  |
| <code>Empty</code> |  |
| <code>Http</code> |  |
| <code>Malformed</code> |  |
| <code>MalformedToolCall</code> |  |
| <code>RateLimited</code> |  |
| <code>Server</code> |  |
| <code>Transport</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `RunMode`

**Methods**

| Method | Description |
| --- | --- |
| <code>name(self) -&gt; &amp;'static str</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Auto</code> |  |
| <code>Goal</code> |  |
| <code>Work</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `SlashCommand`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Agents</code> |  |
| <code>Auto</code> |  |
| <code>Clear</code> |  |
| <code>Compact</code> |  |
| <code>Copy</code> |  |
| <code>Demo</code> |  |
| <code>Diff</code> |  |
| <code>Effort</code> |  |
| <code>Exit</code> |  |
| <code>Export</code> |  |
| <code>Fork</code> |  |
| <code>Goal</code> |  |
| <code>Help</code> |  |
| <code>History</code> |  |
| <code>Login</code> |  |
| <code>Mcp</code> |  |
| <code>Memory</code> |  |
| <code>Model</code> |  |
| <code>Name</code> |  |
| <code>New</code> |  |
| <code>Pane</code> |  |
| <code>Permissions</code> |  |
| <code>Resume</code> |  |
| <code>Retry</code> |  |
| <code>Search</code> |  |
| <code>Settings</code> |  |
| <code>Status</code> |  |
| <code>Steps</code> |  |
| <code>Undo</code> |  |
| <code>Usage</code> |  |
| <code>View</code> |  |
| <code>Workflow</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `StepRun`

**Fields**

| Field | Description |
| --- | --- |
| <code>Run::index: usize</code> |  |
| <code>Run::prompt: String</code> |  |
| <code>Run::total: usize</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Done(i32)</code> |  |
| <code>Run</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `WorkKind`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Goal</code> |  |
| <code>Single</code> |  |
| <code>Steps</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


## Functions

### `add_endpoint`

<pre>pub async fn <a href="#add_endpoint">add_endpoint</a>(name: &amp;str, url: Option&lt;&amp;str&gt;, model: Option&lt;&amp;str&gt;, default: bool) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `add_worktree`

<pre>pub fn <a href="#add_worktree">add_worktree</a>(cwd: &amp;Path) -&gt; <a href="#result">Result</a>&lt;<a href="#tree">Tree</a>&gt;</pre>


### `adopt_keys`

<pre>pub fn <a href="#adopt_keys">adopt_keys</a>() -&gt; <a href="#result">Result</a>&lt;Vec&lt;String&gt;&gt;</pre>


### `attach`

<pre>pub fn <a href="#attach">attach</a>(workspace: &amp;Path, line: &amp;str) -&gt; <a href="#result">Result</a>&lt;<a href="#userinput">UserInput</a>&gt;</pre>


### `auth_info`

<pre>pub fn <a href="#auth_info">auth_info</a>(name: &amp;str) -&gt; <a href="#result">Result</a>&lt;<a href="#authinfo">AuthInfo</a>&gt;</pre>


### `check_commands`

<pre>pub fn <a href="#check_commands">check_commands</a>(workspace: &amp;Path) -&gt; <a href="#result">Result</a>&lt;Option&lt;Vec&lt;String&gt;&gt;&gt;</pre>


### `check_config`

<pre>pub fn <a href="#check_config">check_config</a>() -&gt; <a href="#result">Result</a>&lt;()&gt;</pre>


### `check_key`

<pre>pub async fn <a href="#check_key">check_key</a>(name: &amp;str, key: &amp;str) -&gt; <a href="#result">Result</a>&lt;<a href="summonr-secret.html#source">Source</a>&gt;</pre>


### `classify_endpoint`

<pre>pub fn <a href="#classify_endpoint">classify_endpoint</a>() -&gt; Option&lt;String&gt;</pre>


### `closest_endpoint`

<pre>pub fn <a href="#closest_endpoint">closest_endpoint</a>&lt;'a&gt;(name: &amp;str, names: &amp;'a [String], near: <a href="#endpointnear">EndpointNear</a>) -&gt; Option&lt;&amp;'a str&gt;</pre>


### `command_specs`

<pre>pub const fn <a href="#command_specs">command_specs</a>() -&gt; &amp;'static [command::CommandSpec]</pre>


### `default_effort`

<pre>pub fn <a href="#default_effort">default_effort</a>(endpoint: &amp;str, model: &amp;str) -&gt; Option&lt;<a href="#effort">Effort</a>&gt;</pre>


### `endpoint_chosen_by`

<pre>pub fn <a href="#endpoint_chosen_by">endpoint_chosen_by</a>(name: Option&lt;&amp;str&gt;, model: Option&lt;&amp;str&gt;) -&gt; Option&lt;<a href="#pick">Pick</a>&gt;</pre>


### `endpoint_fallbacks`

<pre>pub fn <a href="#endpoint_fallbacks">endpoint_fallbacks</a>&lt;P&gt;(endpoint: &amp;<a href="#endpoint">Endpoint</a>, connect: impl FnMut(&amp;<a href="#endpoint">Endpoint</a>) -&gt; <a href="#result">Result</a>&lt;P&gt;) -&gt; (Vec&lt;(P, String)&gt;, Vec&lt;String&gt;)</pre>


### `endpoint_label`

<pre>pub fn <a href="#endpoint_label">endpoint_label</a>(name: &amp;str) -&gt; Option&lt;&amp;'static str&gt;</pre>


### `endpoint_names`

<pre>pub fn <a href="#endpoint_names">endpoint_names</a>() -&gt; <a href="#result">Result</a>&lt;Vec&lt;String&gt;&gt;</pre>


### `endpoint_names_ordered`

<pre>pub fn <a href="#endpoint_names_ordered">endpoint_names_ordered</a>() -&gt; <a href="#result">Result</a>&lt;Vec&lt;String&gt;&gt;</pre>


### `endpoint_set_up`

<pre>pub fn <a href="#endpoint_set_up">endpoint_set_up</a>() -&gt; Vec&lt;String&gt;</pre>


### `export_session`

<pre>pub fn <a href="#export_session">export_session</a>(path: &amp;Path) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `finish_worktree`

<pre>pub fn <a href="#finish_worktree">finish_worktree</a>(tree: &amp;<a href="#tree">Tree</a>, objective: &amp;str, paths: &amp;[String]) -&gt; <a href="#result">Result</a>&lt;Vec&lt;String&gt;&gt;</pre>


### `goal_prompt`

<pre>pub fn <a href="#goal_prompt">goal_prompt</a>(objective: &amp;str, gates: &amp;[String], round: u32) -&gt; String</pre>


### `in_git`

<pre>pub fn <a href="#in_git">in_git</a>(path: &amp;Path) -&gt; bool</pre>


### `index_files`

<pre>pub fn <a href="#index_files">index_files</a>(root: &amp;Path) -&gt; i32</pre>


### `install_plugin`

<pre>pub fn <a href="#install_plugin">install_plugin</a>(src: &amp;Path) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `interrupted`

<pre>pub async fn <a href="#interrupted">interrupted</a>() -&gt; i32</pre>


### `is_endpoint`

<pre>pub fn <a href="#is_endpoint">is_endpoint</a>(token: &amp;str) -&gt; bool</pre>


### `last_for`

<pre>pub fn <a href="#last_for">last_for</a>(workspace: &amp;Path) -&gt; <a href="#result">Result</a>&lt;Option&lt;session::SessionInfo&gt;&gt;</pre>


### `list_plugins`

<pre>pub fn <a href="#list_plugins">list_plugins</a>() -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `list_sessions`

<pre>pub fn <a href="#list_sessions">list_sessions</a>() -&gt; <a href="#result">Result</a>&lt;Vec&lt;session::SessionInfo&gt;&gt;</pre>


### `list_worktrees`

<pre>pub fn <a href="#list_worktrees">list_worktrees</a>(cwd: &amp;Path) -&gt; <a href="#result">Result</a>&lt;Vec&lt;<a href="#tree">Tree</a>&gt;&gt;</pre>


### `live_jobs`

<pre>pub fn <a href="#live_jobs">live_jobs</a>(workspace: &amp;Path) -&gt; Vec&lt;String&gt;</pre>


### `lock_file`

<pre>pub fn <a href="#lock_file">lock_file</a>(path: &amp;Path, wait: bool) -&gt; Result&lt;Option&lt;File&gt;&gt;</pre>


### `login_seat`

<pre>pub fn <a href="#login_seat">login_seat</a>(name: &amp;str) -&gt; <a href="#result">Result</a>&lt;<a href="#endpoint">Endpoint</a>&gt;</pre>


### `logout`

<pre>pub fn <a href="#logout">logout</a>(name: &amp;str) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `mcp_add`

<pre>pub fn <a href="#mcp_add">mcp_add</a>(name: &amp;str, launch: &amp;<a href="#mcplaunch">McpLaunch</a>) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `mcp_call_label`

<pre>pub fn <a href="#mcp_call_label">mcp_call_label</a>(name: &amp;str, arguments: &amp;str) -&gt; String</pre>


### `mcp_probe`

<pre>pub async fn <a href="#mcp_probe">mcp_probe</a>(workspace: &amp;Path, name: &amp;str) -&gt; <a href="#result">Result</a>&lt;usize&gt;</pre>


### `mcp_remove`

<pre>pub fn <a href="#mcp_remove">mcp_remove</a>(name: &amp;str) -&gt; <a href="#result">Result</a>&lt;bool&gt;</pre>


### `mcp_status`

<pre>pub fn <a href="#mcp_status">mcp_status</a>(workspace: &amp;Path, board: Option&lt;&amp;<a href="#mcpboard">McpBoard</a>&gt;) -&gt; <a href="#result">Result</a>&lt;Vec&lt;<a href="#serverstatus">ServerStatus</a>&gt;&gt;</pre>


### `parse_command`

<pre>pub fn <a href="#parse_command">parse_command</a>(line: &amp;str) -&gt; Option&lt;<a href="#slashcommand">SlashCommand</a>&gt;</pre>


### `profile`

<pre>pub fn <a href="#profile">profile</a>(scope: <a href="#profilescope">ProfileScope</a>&lt;'_&gt;) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `prune_sessions`

<pre>pub fn <a href="#prune_sessions">prune_sessions</a>(keep: Option&lt;&amp;str&gt;)</pre>


### `remove_endpoint`

<pre>pub fn <a href="#remove_endpoint">remove_endpoint</a>(name: &amp;str) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `resolve_endpoint`

<pre>pub fn <a href="#resolve_endpoint">resolve_endpoint</a>(name: Option&lt;&amp;str&gt;, model: Option&lt;&amp;str&gt;, need: <a href="#need">Need</a>) -&gt; <a href="#result">Result</a>&lt;<a href="#endpoint">Endpoint</a>&gt;</pre>


### `resolve_session`

<pre>pub fn <a href="#resolve_session">resolve_session</a>(id: &amp;str) -&gt; <a href="#result">Result</a>&lt;PathBuf&gt;</pre>


### `revision`

<pre>pub fn <a href="#revision">revision</a>() -&gt; &amp;'static str</pre>


### `run_tool`

<pre>pub async fn <a href="#run_tool">run_tool</a>(workspace: impl AsRef&lt;Path&gt;, name: &amp;str, arguments: &amp;str, sandbox: Option&lt;<a href="#network">Network</a>&gt;) -&gt; <a href="#result">Result</a>&lt;<a href="#toolresult">ToolResult</a>&gt;</pre>


### `sandbox_on`

<pre>pub fn <a href="#sandbox_on">sandbox_on</a>() -&gt; <a href="#result">Result</a>&lt;bool&gt;</pre>


### `set_default_endpoint`

<pre>pub fn <a href="#set_default_endpoint">set_default_endpoint</a>(name: &amp;str, model: Option&lt;&amp;str&gt;) -&gt; <a href="#result">Result</a>&lt;String&gt;</pre>


### `short`

<pre>pub fn <a href="#short">short</a>(id: &amp;str) -&gt; &amp;str</pre>


### `stop_children`

<pre>pub fn <a href="#stop_children">stop_children</a>()</pre>


### `summonr_home`

<pre>pub fn <a href="#summonr_home">summonr_home</a>() -&gt; <a href="#result">Result</a>&lt;PathBuf&gt;</pre>


### `sweep_worktrees`

<pre>pub fn <a href="#sweep_worktrees">sweep_worktrees</a>(cwd: &amp;Path) -&gt; <a href="#result">Result</a>&lt;Vec&lt;PathBuf&gt;&gt;</pre>


### `system_prompt`

<pre>pub fn <a href="#system_prompt">system_prompt</a>(workspace: &amp;Path, sources: <a href="#sources">Sources</a>) -&gt; String</pre>


### `tilde`

<pre>pub fn <a href="#tilde">tilde</a>(path: &amp;Path) -&gt; String</pre>


### `uninstall_plugin`

<pre>pub fn <a href="#uninstall_plugin">uninstall_plugin</a>(name: &amp;str) -&gt; <a href="#result">Result</a>&lt;bool&gt;</pre>


### `vision_endpoint`

<pre>pub fn <a href="#vision_endpoint">vision_endpoint</a>() -&gt; <a href="#result">Result</a>&lt;Option&lt;<a href="#endpoint">Endpoint</a>&gt;&gt;</pre>


### `worktree_status`

<pre>pub fn <a href="#worktree_status">worktree_status</a>(cwd: &amp;Path) -&gt; <a href="#result">Result</a>&lt;Vec&lt;worktree::Status&gt;&gt;</pre>


## Type aliases

### `McpBoard`

<pre>pub type <a href="#mcpboard">McpBoard</a> = Arc&lt;Mutex&lt;Weak&lt;mcp::Hub&gt;&gt;&gt;</pre>


### `Result`

<pre>pub type <a href="#result">Result</a>&lt;T&gt; = Result&lt;T, <a href="#error">Error</a>&gt;</pre>


## Constants

### `INDEX_ENTRY`

<pre>pub const <a href="#index_entry">INDEX_ENTRY</a>: &amp;str</pre>


---

Generated by truesight from `summonr` 0.1.0. See the [API overview](index.md).
