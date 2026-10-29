# Module `summonr`

| Item | Kind | Description |
| --- | --- | --- |
| [`AuthInfo`](#authinfo) | struct |  |
| [`Budget`](#budget) | struct |  |
| [`Client`](#client) | struct |  |
| [`Endpoint`](#endpoint) | struct |  |
| [`InstalledPlugin`](#installedplugin) | struct |  |
| [`Loop`](#loop) | struct |  |
| [`ServerStatus`](#serverstatus) | struct |  |
| [`Sources`](#sources) | struct |  |
| [`ToolResult`](#toolresult) | struct |  |
| [`Tree`](#tree) | struct |  |
| [`Turn`](#turn) | struct |  |
| [`Delta`](#delta) | enum |  |
| [`EndpointNear`](#endpointnear) | enum |  |
| [`EndpointState`](#endpointstate) | enum |  |
| [`Job`](#job) | enum |  |
| [`LoginFlow`](#loginflow) | enum |  |
| [`McpLaunch`](#mcplaunch) | enum |  |
| [`McpState`](#mcpstate) | enum |  |
| [`McpTransport`](#mcptransport) | enum |  |
| [`Need`](#need) | enum |  |
| [`NextAction`](#nextaction) | enum |  |
| [`Pick`](#pick) | enum |  |
| [`ProfileScope`](#profilescope) | enum |  |
| [`SlashCommand`](#slashcommand) | enum |  |
| [`add_endpoint`](#add_endpoint) | fn |  |
| [`add_worktree`](#add_worktree) | fn |  |
| [`adopt_keys`](#adopt_keys) | fn |  |
| [`attach`](#attach) | fn |  |
| [`auth_info`](#auth_info) | fn |  |
| [`check_commands`](#check_commands) | fn |  |
| [`check_config`](#check_config) | fn |  |
| [`check_key`](#check_key) | fn |  |
| [`check_plugin_servers`](#check_plugin_servers) | fn |  |
| [`classify_endpoint`](#classify_endpoint) | fn |  |
| [`closest_endpoint`](#closest_endpoint) | fn |  |
| [`command_specs`](#command_specs) | fn |  |
| [`default_effort`](#default_effort) | fn |  |
| [`endpoint_chosen_by`](#endpoint_chosen_by) | fn |  |
| [`endpoint_fallbacks`](#endpoint_fallbacks) | fn |  |
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
| [`login_endpoint`](#login_endpoint) | fn |  |
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
| [`session_info`](#session_info) | fn |  |
| [`set_default_endpoint`](#set_default_endpoint) | fn |  |
| [`short`](#short) | fn |  |
| [`stop_children`](#stop_children) | fn |  |
| [`sweep_worktrees`](#sweep_worktrees) | fn |  |
| [`system_prompt`](#system_prompt) | fn |  |
| [`tilde`](#tilde) | fn |  |
| [`uninstall_plugin`](#uninstall_plugin) | fn |  |
| [`vision_endpoint`](#vision_endpoint) | fn |  |
| [`worktree_at`](#worktree_at) | fn |  |
| [`worktree_status`](#worktree_status) | fn |  |
| [`McpBoard`](#mcpboard) | type |  |
| [`INDEX_ENTRY`](#index_entry) | const |  |
| [`AbortCode`](#abortcode) | use |  |
| [`Effort`](#effort) | use |  |
| [`Error`](#error) | use |  |
| [`Event`](#event) | use |  |
| [`McpChoice`](#mcpchoice) | use |  |
| [`McpField`](#mcpfield) | use |  |
| [`McpFieldKind`](#mcpfieldkind) | use |  |
| [`McpForm`](#mcpform) | use |  |
| [`McpFormAction`](#mcpformaction) | use |  |
| [`McpFormAnswer`](#mcpformanswer) | use |  |
| [`McpTextFormat`](#mcptextformat) | use |  |
| [`ProviderFailureKind`](#providerfailurekind) | use |  |
| [`Receipt`](#receipt) | use |  |
| [`Result`](#result) | use |  |
| [`StepState`](#stepstate) | use |  |
| [`Usage`](#usage) | use |  |
| [`UserInput`](#userinput) | use |  |
| [`WorkKind`](#workkind) | use |  |
| [`WorkProgress`](#workprogress) | use |  |
| [`summonr_home`](#summonr_home) | use |  |

## Structs

### `AuthInfo`

**Methods**

| Method | Description |
| --- | --- |
| <code>catalog_line(&amp;self, name: usize, state: usize) -&gt; String</code> |  |
| <code>catalog_rank(&amp;self) -&gt; u8</code> |  |
| <code>catalog_title(&amp;self) -&gt; &amp;'static str</code> |  |
| <code>dual(&amp;self) -&gt; bool</code> |  |
| <code>key_action(&amp;self) -&gt; &amp;'static str</code> |  |
| <code>keyless(&amp;self) -&gt; bool</code> |  |
| <code>matches(&amp;self, query: &amp;str) -&gt; bool</code> |  |
| <code>paste_key(&amp;self) -&gt; bool</code> |  |
| <code>plan(&amp;self) -&gt; bool</code> |  |
| <code>state(&amp;self) -&gt; String</code> |  |
| <code>status(&amp;self) -&gt; String</code> |  |
| <code>stored(&amp;self) -&gt; bool</code> |  |
| <code>title(&amp;self) -&gt; &amp;str</code> |  |
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
| <code>provider: String</code> |  |
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
| <code>async classify(&amp;self, model: &amp;str, state: &amp;Value, questions: &amp;BTreeMap&lt;String, <a href="summonr-classify.html#question">Question</a>&gt;) -&gt; Result&lt;<a href="summonr-classify.html#decision">Decision</a>&gt;</code> |  |
| <code>connect(endpoint: &amp;<a href="#endpoint">Endpoint</a>) -&gt; Result&lt;Self&gt;</code> |  |
| <code>new(api_key: impl Into&lt;String&gt;, base_url: impl Into&lt;String&gt;) -&gt; Result&lt;Self&gt;</code> |  |
| <code>on_stream(self, emit: impl Fn(&amp;<a href="#delta">Delta</a>) + Send + Sync + 'static) -&gt; Self</code> |  |
| <code>async context(&amp;self, endpoint: &amp;str, model: &amp;str) -&gt; Option&lt;u32&gt;</code> |  |
| <code>async discover(endpoint: &amp;<a href="#endpoint">Endpoint</a>) -&gt; Result&lt;Vec&lt;<a href="summonr-catalog.html#model">Model</a>&gt;&gt;</code> |  |
| <code>state(error: &amp;Error) -&gt; <a href="#endpointstate">EndpointState</a></code> |  |
| <code>async login(endpoint: &amp;<a href="#endpoint">Endpoint</a>, prompt: &amp;mut Prompt) -&gt; Result&lt;()&gt;</code> |  |

**Trait implementations:** <code>Clone</code>


### `Endpoint`

**Methods**

| Method | Description |
| --- | --- |
| <code>effort_for(&amp;self, explicit: Option&lt;Effort&gt;) -&gt; Option&lt;Effort&gt;</code> |  |
| <code>keyless(&amp;self) -&gt; bool</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>api_key: Option&lt;Secret&gt;</code> |  |
| <code>assistant_after_tool: bool</code> |  |
| <code>base_url: Option&lt;String&gt;</code> |  |
| <code>body: Option&lt;Value&gt;</code> |  |
| <code>cache_marks: bool</code> |  |
| <code>cache_ttl: CacheTtl</code> |  |
| <code>compact_tokens: Option&lt;u32&gt;</code> |  |
| <code>context_window: Option&lt;u32&gt;</code> |  |
| <code>effort: Option&lt;Effort&gt;</code> |  |
| <code>env: BTreeMap&lt;String, Secret&gt;</code> |  |
| <code>fallback: Vec&lt;String&gt;</code> |  |
| <code>headers: BTreeMap&lt;String, Secret&gt;</code> |  |
| <code>idle_timeout_secs: Option&lt;u64&gt;</code> |  |
| <code>images: Option&lt;bool&gt;</code> |  |
| <code>keep_reasoning: bool</code> |  |
| <code>model: String</code> |  |
| <code>models_url: Option&lt;String&gt;</code> |  |
| <code>name: String</code> |  |
| <code>oauth: Option&lt;OAuth&gt;</code> |  |
| <code>price: Option&lt;Price&gt;</code> |  |
| <code>prompt_cache_key: bool</code> |  |
| <code>session_headers: BTreeMap&lt;String, endpoint::SessionValue&gt;</code> |  |
| <code>source: Option&lt;<a href="summonr-secret.html#source">Source</a>&gt;</code> |  |
| <code>wire: Wire</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


### `InstalledPlugin`

**Fields**

| Field | Description |
| --- | --- |
| <code>dir: PathBuf</code> |  |
| <code>name: String</code> |  |
| <code>report: String</code> |  |


### `Loop`

**Methods**

| Method | Description |
| --- | --- |
| <code>pub fn <a href="#loop">Loop</a>&lt;<a href="#client">Client</a>&gt;::configure_workflow(&amp;mut self, workflow: &amp;<a href="summonr-workflow.html#workflow">Workflow</a>, connect: impl FnMut(&amp;<a href="summonr-workflow.html#modelselection">ModelSelection</a>) -&gt; Result&lt;(<a href="#client">Client</a>, <a href="#endpoint">Endpoint</a>)&gt;) -&gt; Result&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;<a href="#client">Client</a>&gt;::watch_driver(&amp;mut self, client: <a href="#client">Client</a>, endpoint: &amp;str, model: &amp;str)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::abandon_work(&amp;mut self, reason: &amp;str) -&gt; Result&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::goal_at(path: &amp;Path, workspace: &amp;Path, objective: &amp;str) -&gt; bool</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::job_at(path: &amp;Path, workspace: &amp;Path) -&gt; Option&lt;(WorkKind, String)&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::select_goal(&amp;mut self, objective: &amp;str, commands: &amp;[String]) -&gt; Result&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::select_steps(&amp;mut self, markdown: &amp;str, commands: &amp;[String]) -&gt; Result&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::steps_at(path: &amp;Path, workspace: &amp;Path, markdown: &amp;str) -&gt; bool</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::touched_at(path: &amp;Path) -&gt; Vec&lt;String&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::workspace(&amp;self) -&gt; &amp;Path</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::allow_path(&amp;mut self, path: &amp;str) -&gt; Result&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::budget(&amp;self) -&gt; <a href="#budget">Budget</a></code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::choose_effort(&amp;mut self, effort: Option&lt;Effort&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::context_window(&amp;self) -&gt; Option&lt;u32&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::effort(&amp;self) -&gt; Option&lt;Effort&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::fork_path(provider: P, model: impl Into&lt;String&gt;, workspace: impl AsRef&lt;Path&gt;, system_prompt: impl Into&lt;String&gt;, source: &amp;Path) -&gt; Result&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::mode(&amp;self) -&gt; WorkKind</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::model(&amp;self) -&gt; &amp;str</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::new(provider: P, model: impl Into&lt;String&gt;, workspace: impl AsRef&lt;Path&gt;, system_prompt: impl Into&lt;String&gt;) -&gt; Result&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::policy_inert(&amp;self) -&gt; bool</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::resume_path(provider: P, model: impl Into&lt;String&gt;, workspace: impl AsRef&lt;Path&gt;, system_prompt: impl Into&lt;String&gt;, path: &amp;Path) -&gt; Result&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_context_window(&amp;mut self, window: Option&lt;u32&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_effort(&amp;mut self, effort: Option&lt;Effort&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_env(&amp;mut self, env: impl IntoIterator&lt;Item = (String, String)&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_images(&amp;mut self, model_images: bool, vision: Option&lt;<a href="#endpoint">Endpoint</a>&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_modules(&amp;mut self, switches: &amp;Switches) -&gt; Result&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::verify_commands(&amp;self) -&gt; &amp;[String]</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_interactive(self) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_strict(self) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_unrestricted_commands(self) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::without_task_list(self) -&gt; Self</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::apply_endpoint(&amp;mut self, endpoint: &amp;<a href="#endpoint">Endpoint</a>)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::history_len(&amp;self) -&gt; usize</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::name(&amp;self) -&gt; Option&lt;String&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::next_action(&amp;self, job: &amp;<a href="#job">Job</a>) -&gt; Result&lt;<a href="#nextaction">NextAction</a>&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::session_id(&amp;self) -&gt; String</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::session_path(&amp;self) -&gt; &amp;Path</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_compact_tokens(&amp;mut self, tokens: Option&lt;u32&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_fallbacks(&amp;mut self, fallbacks: impl IntoIterator&lt;Item = (P, <a href="#endpoint">Endpoint</a>)&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_name(&amp;mut self, name: &amp;str) -&gt; Result&lt;()&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::set_price(&amp;mut self, price: Option&lt;Price&gt;)</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_budget(self, budget: <a href="#budget">Budget</a>) -&gt; Result&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::with_verify(self, commands: Vec&lt;String&gt;) -&gt; Result&lt;Self&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::work_progress(&amp;self) -&gt; Result&lt;WorkProgress&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::workflow(&amp;self) -&gt; Option&lt;String&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::compact(&amp;mut self) -&gt; Result&lt;bool&gt;</code> |  |
| <code>pub fn <a href="#loop">Loop</a>&lt;P&gt;::mcp_board(&amp;self) -&gt; <a href="#mcpboard">McpBoard</a></code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::shell(&amp;self, command: &amp;str) -&gt; Result&lt;(String, bool)&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::stop_work(&amp;mut self, reason: &amp;str, emit: impl FnMut(&amp;Event)) -&gt; Result&lt;i32&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::turn(&amp;mut self, prompt: &amp;str) -&gt; Result&lt;<a href="#turn">Turn</a>&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::turn_mail_with(&amp;mut self, emit: impl FnMut(&amp;Event)) -&gt; Result&lt;<a href="#turn">Turn</a>&gt;</code> |  |
| <code>pub async fn <a href="#loop">Loop</a>&lt;P&gt;::turn_with(&amp;mut self, input: impl Into&lt;UserInput&gt;, emit: impl FnMut(&amp;Event)) -&gt; Result&lt;<a href="#turn">Turn</a>&gt;</code> |  |


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
| <code>changes(&amp;self) -&gt; Result&lt;Vec&lt;String&gt;&gt;</code> |  |
| <code>unchanged(&amp;self) -&gt; Result&lt;bool&gt;</code> |  |

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
| <code>checks: Vec&lt;Outcome&gt;</code> |  |
| <code>events: Vec&lt;Event&gt;</code> |  |
| <code>rounds: usize</code> |  |
| <code>text: String</code> |  |
| <code>usage: Usage</code> |  |

**Trait implementations:** <code>Clone</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


## Enums

### `Delta`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Call(String)</code> |  |
| <code>Progress</code> |  |
| <code>Text(String)</code> |  |
| <code>Thinking(String)</code> |  |


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


### `Job`

**Methods**

| Method | Description |
| --- | --- |
| <code>const aborted(&amp;self) -&gt; &amp;'static str</code> |  |
| <code>check(&amp;self, index: usize) -&gt; Option&lt;String&gt;</code> |  |
| <code>const label(&amp;self) -&gt; &amp;'static str</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>Goal::gates: Vec&lt;String&gt;</code> |  |
| <code>Goal::objective: String</code> |  |
| <code>Steps::plan: String</code> |  |
| <code>Steps::steps: Vec&lt;<a href="summonr-steps.html#step">Step</a>&gt;</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Goal</code> |  |
| <code>Steps</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Debug</code>


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


### `McpLaunch`

**Fields**

| Field | Description |
| --- | --- |
| <code>Http::auth: Option&lt;Box&lt;OAuth&gt;&gt;</code> |  |
| <code>Http::headers: Vec&lt;(String, String)&gt;</code> |  |
| <code>Http::url: String</code> |  |
| <code>Stdio::args: Vec&lt;String&gt;</code> |  |
| <code>Stdio::command: String</code> |  |
| <code>Stdio::env: Vec&lt;(String, String)&gt;</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Http</code> |  |
| <code>Stdio</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


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
| <code>ModelAndKey</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `NextAction`

**Fields**

| Field | Description |
| --- | --- |
| <code>Run::prompt: String</code> |  |
| <code>Run::step: Option&lt;(usize, usize)&gt;</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Done(i32)</code> |  |
| <code>Run</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>StructuralPartialEq</code>


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
| <code>Skills</code> |  |
| <code>Status</code> |  |
| <code>Steps</code> |  |
| <code>Undo</code> |  |
| <code>Usage</code> |  |
| <code>View</code> |  |
| <code>Workflow</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Deserialize&lt;'de&gt;</code>


## Functions

### `add_endpoint`

<pre>pub async fn <a href="#add_endpoint">add_endpoint</a>(name: &amp;str, url: Option&lt;&amp;str&gt;, model: Option&lt;&amp;str&gt;, default: bool) -&gt; Result&lt;String&gt;</pre>


### `add_worktree`

<pre>pub fn <a href="#add_worktree">add_worktree</a>(cwd: &amp;Path) -&gt; Result&lt;<a href="#tree">Tree</a>&gt;</pre>


### `adopt_keys`

<pre>pub fn <a href="#adopt_keys">adopt_keys</a>() -&gt; Result&lt;Vec&lt;String&gt;&gt;</pre>


### `attach`

<pre>pub fn <a href="#attach">attach</a>(workspace: &amp;Path, line: &amp;str) -&gt; Result&lt;UserInput&gt;</pre>


### `auth_info`

<pre>pub fn <a href="#auth_info">auth_info</a>(name: &amp;str) -&gt; Result&lt;<a href="#authinfo">AuthInfo</a>&gt;</pre>


### `check_commands`

<pre>pub fn <a href="#check_commands">check_commands</a>(workspace: &amp;Path) -&gt; Result&lt;Option&lt;Vec&lt;String&gt;&gt;&gt;</pre>


### `check_config`

<pre>pub fn <a href="#check_config">check_config</a>() -&gt; Result&lt;()&gt;</pre>


### `check_key`

<pre>pub async fn <a href="#check_key">check_key</a>(name: &amp;str, key: &amp;str) -&gt; Result&lt;<a href="summonr-secret.html#source">Source</a>&gt;</pre>


### `check_plugin_servers`

<pre>pub async fn <a href="#check_plugin_servers">check_plugin_servers</a>(installed: &amp;<a href="#installedplugin">InstalledPlugin</a>, workspace: &amp;Path) -&gt; Result&lt;Vec&lt;String&gt;&gt;</pre>


### `classify_endpoint`

<pre>pub fn <a href="#classify_endpoint">classify_endpoint</a>() -&gt; Option&lt;String&gt;</pre>


### `closest_endpoint`

<pre>pub fn <a href="#closest_endpoint">closest_endpoint</a>&lt;'a&gt;(name: &amp;str, names: &amp;'a [String], near: <a href="#endpointnear">EndpointNear</a>) -&gt; Option&lt;&amp;'a str&gt;</pre>


### `command_specs`

<pre>pub fn <a href="#command_specs">command_specs</a>() -&gt; &amp;'static [command::CommandSpec]</pre>


### `default_effort`

<pre>pub fn <a href="#default_effort">default_effort</a>(endpoint: &amp;str, model: &amp;str) -&gt; Option&lt;Effort&gt;</pre>


### `endpoint_chosen_by`

<pre>pub fn <a href="#endpoint_chosen_by">endpoint_chosen_by</a>(name: Option&lt;&amp;str&gt;, model: Option&lt;&amp;str&gt;) -&gt; Option&lt;<a href="#pick">Pick</a>&gt;</pre>


### `endpoint_fallbacks`

<pre>pub fn <a href="#endpoint_fallbacks">endpoint_fallbacks</a>&lt;P&gt;(endpoint: &amp;<a href="#endpoint">Endpoint</a>, connect: impl FnMut(&amp;<a href="#endpoint">Endpoint</a>) -&gt; Result&lt;P&gt;) -&gt; (Vec&lt;(P, <a href="#endpoint">Endpoint</a>)&gt;, Vec&lt;String&gt;)</pre>


### `endpoint_names`

<pre>pub fn <a href="#endpoint_names">endpoint_names</a>() -&gt; Result&lt;Vec&lt;String&gt;&gt;</pre>


### `endpoint_names_ordered`

<pre>pub fn <a href="#endpoint_names_ordered">endpoint_names_ordered</a>() -&gt; Result&lt;Vec&lt;String&gt;&gt;</pre>


### `endpoint_set_up`

<pre>pub fn <a href="#endpoint_set_up">endpoint_set_up</a>() -&gt; Vec&lt;String&gt;</pre>


### `export_session`

<pre>pub fn <a href="#export_session">export_session</a>(path: &amp;Path) -&gt; Result&lt;String&gt;</pre>


### `finish_worktree`

<pre>pub fn <a href="#finish_worktree">finish_worktree</a>(tree: &amp;<a href="#tree">Tree</a>, objective: &amp;str, paths: &amp;[String]) -&gt; Result&lt;Vec&lt;String&gt;&gt;</pre>


### `goal_prompt`

<pre>pub fn <a href="#goal_prompt">goal_prompt</a>(objective: &amp;str, gates: &amp;[String], round: u32) -&gt; String</pre>


### `in_git`

<pre>pub fn <a href="#in_git">in_git</a>(path: &amp;Path) -&gt; bool</pre>


### `index_files`

<pre>pub fn <a href="#index_files">index_files</a>(root: &amp;Path) -&gt; i32</pre>


### `install_plugin`

<pre>pub fn <a href="#install_plugin">install_plugin</a>(src: &amp;Path) -&gt; Result&lt;<a href="#installedplugin">InstalledPlugin</a>&gt;</pre>


### `interrupted`

<pre>pub async fn <a href="#interrupted">interrupted</a>() -&gt; i32</pre>


### `is_endpoint`

<pre>pub fn <a href="#is_endpoint">is_endpoint</a>(token: &amp;str) -&gt; bool</pre>


### `last_for`

<pre>pub fn <a href="#last_for">last_for</a>(workspace: &amp;Path) -&gt; Result&lt;Option&lt;session::SessionInfo&gt;&gt;</pre>


### `list_plugins`

<pre>pub fn <a href="#list_plugins">list_plugins</a>() -&gt; Result&lt;String&gt;</pre>


### `list_sessions`

<pre>pub fn <a href="#list_sessions">list_sessions</a>() -&gt; Result&lt;Vec&lt;session::SessionInfo&gt;&gt;</pre>


### `list_worktrees`

<pre>pub fn <a href="#list_worktrees">list_worktrees</a>(cwd: &amp;Path) -&gt; Result&lt;Vec&lt;<a href="#tree">Tree</a>&gt;&gt;</pre>


### `live_jobs`

<pre>pub fn <a href="#live_jobs">live_jobs</a>(workspace: &amp;Path) -&gt; Vec&lt;String&gt;</pre>


### `lock_file`

<pre>pub fn <a href="#lock_file">lock_file</a>(path: &amp;Path, wait: bool) -&gt; Result&lt;Option&lt;File&gt;&gt;</pre>


### `login_endpoint`

<pre>pub fn <a href="#login_endpoint">login_endpoint</a>(name: &amp;str) -&gt; Result&lt;<a href="#endpoint">Endpoint</a>&gt;</pre>


### `logout`

<pre>pub fn <a href="#logout">logout</a>(name: &amp;str) -&gt; Result&lt;String&gt;</pre>


### `mcp_add`

<pre>pub fn <a href="#mcp_add">mcp_add</a>(name: &amp;str, launch: &amp;<a href="#mcplaunch">McpLaunch</a>) -&gt; Result&lt;String&gt;</pre>


### `mcp_call_label`

<pre>pub fn <a href="#mcp_call_label">mcp_call_label</a>(name: &amp;str, arguments: &amp;str) -&gt; String</pre>


### `mcp_probe`

<pre>pub async fn <a href="#mcp_probe">mcp_probe</a>(workspace: &amp;Path, name: &amp;str) -&gt; Result&lt;Option&lt;usize&gt;&gt;</pre>


### `mcp_remove`

<pre>pub fn <a href="#mcp_remove">mcp_remove</a>(name: &amp;str) -&gt; Result&lt;bool&gt;</pre>


### `mcp_status`

<pre>pub fn <a href="#mcp_status">mcp_status</a>(workspace: &amp;Path, board: Option&lt;&amp;<a href="#mcpboard">McpBoard</a>&gt;) -&gt; Result&lt;Vec&lt;<a href="#serverstatus">ServerStatus</a>&gt;&gt;</pre>


### `parse_command`

<pre>pub fn <a href="#parse_command">parse_command</a>(line: &amp;str) -&gt; Option&lt;<a href="#slashcommand">SlashCommand</a>&gt;</pre>


### `profile`

<pre>pub fn <a href="#profile">profile</a>(scope: <a href="#profilescope">ProfileScope</a>&lt;'_&gt;) -&gt; Result&lt;String&gt;</pre>


### `prune_sessions`

<pre>pub fn <a href="#prune_sessions">prune_sessions</a>(keep: Option&lt;&amp;str&gt;)</pre>


### `remove_endpoint`

<pre>pub fn <a href="#remove_endpoint">remove_endpoint</a>(name: &amp;str) -&gt; Result&lt;String&gt;</pre>


### `resolve_endpoint`

<pre>pub fn <a href="#resolve_endpoint">resolve_endpoint</a>(name: Option&lt;&amp;str&gt;, model: Option&lt;&amp;str&gt;, need: <a href="#need">Need</a>) -&gt; Result&lt;<a href="#endpoint">Endpoint</a>&gt;</pre>


### `resolve_session`

<pre>pub fn <a href="#resolve_session">resolve_session</a>(id: &amp;str) -&gt; Result&lt;PathBuf&gt;</pre>


### `revision`

<pre>pub fn <a href="#revision">revision</a>() -&gt; &amp;'static str</pre>


### `run_tool`

<pre>pub async fn <a href="#run_tool">run_tool</a>(workspace: impl AsRef&lt;Path&gt;, name: &amp;str, arguments: &amp;str) -&gt; Result&lt;<a href="#toolresult">ToolResult</a>&gt;</pre>


### `session_info`

<pre>pub fn <a href="#session_info">session_info</a>(path: PathBuf) -&gt; session::SessionInfo</pre>


### `set_default_endpoint`

<pre>pub fn <a href="#set_default_endpoint">set_default_endpoint</a>(name: &amp;str, model: Option&lt;&amp;str&gt;) -&gt; Result&lt;String&gt;</pre>


### `short`

<pre>pub fn <a href="#short">short</a>(id: &amp;str) -&gt; &amp;str</pre>


### `stop_children`

<pre>pub fn <a href="#stop_children">stop_children</a>()</pre>


### `sweep_worktrees`

<pre>pub fn <a href="#sweep_worktrees">sweep_worktrees</a>(cwd: &amp;Path) -&gt; Result&lt;Vec&lt;PathBuf&gt;&gt;</pre>


### `system_prompt`

<pre>pub fn <a href="#system_prompt">system_prompt</a>(workspace: &amp;Path, sources: <a href="#sources">Sources</a>) -&gt; String</pre>


### `tilde`

<pre>pub fn <a href="#tilde">tilde</a>(path: &amp;Path) -&gt; String</pre>


### `uninstall_plugin`

<pre>pub fn <a href="#uninstall_plugin">uninstall_plugin</a>(plugin_name: &amp;str) -&gt; Result&lt;bool&gt;</pre>


### `vision_endpoint`

<pre>pub fn <a href="#vision_endpoint">vision_endpoint</a>() -&gt; Result&lt;Option&lt;<a href="#endpoint">Endpoint</a>&gt;&gt;</pre>


### `worktree_at`

<pre>pub fn <a href="#worktree_at">worktree_at</a>(cwd: &amp;Path) -&gt; Option&lt;<a href="#tree">Tree</a>&gt;</pre>


### `worktree_status`

<pre>pub fn <a href="#worktree_status">worktree_status</a>(cwd: &amp;Path) -&gt; Result&lt;Vec&lt;worktree::Status&gt;&gt;</pre>


## Type aliases

### `McpBoard`

<pre>pub type <a href="#mcpboard">McpBoard</a> = Arc&lt;Mutex&lt;Weak&lt;mcp::Hub&gt;&gt;&gt;</pre>


## Constants

### `INDEX_ENTRY`

<pre>pub const <a href="#index_entry">INDEX_ENTRY</a>: &amp;str</pre>


## Re-exports

### `AbortCode`


### `Effort`


### `Error`


### `Event`


### `McpChoice`


### `McpField`


### `McpFieldKind`


### `McpForm`


### `McpFormAction`


### `McpFormAnswer`


### `McpTextFormat`


### `ProviderFailureKind`


### `Receipt`


### `Result`


### `StepState`


### `Usage`


### `UserInput`


### `WorkKind`


### `WorkProgress`


### `summonr_home`


---

Generated by truesight from `summonr` 0.1.1. See the [API overview](index.md).
