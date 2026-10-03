# Module `summonr_auth`

| Item | Kind | Description |
| --- | --- | --- |
| [`Auth`](#auth) | struct |  |
| [`Prompt`](#prompt) | struct |  |
| [`Spec`](#spec) | struct |  |
| [`Error`](#error) | enum |  |
| [`Grant`](#grant) | enum |  |
| [`Method`](#method) | enum |  |
| [`Scheme`](#scheme) | enum |  |
| [`TokenStyle`](#tokenstyle) | enum |  |
| [`Store`](#store) | trait |  |
| [`builder`](#builder) | fn |  |
| [`http`](#http) | fn |  |
| [`Answer`](#answer) | type |  |
| [`Result`](#result) | type |  |
| [`AGENT`](#agent) | const |  |

## Structs

### `Auth`

**Methods**

| Method | Description |
| --- | --- |
| <code>async apply(&amp;self, request: RequestBuilder, http: &amp;Session) -&gt; <a href="#result">Result</a>&lt;RequestBuilder&gt;</code> |  |
| <code>forget_live(&amp;self)</code> |  |
| <code>is_login(&amp;self) -&gt; bool</code> |  |
| <code>async login(&amp;self, http: &amp;Session, prompt: &amp;mut <a href="#prompt">Prompt</a>) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |
| <code>new(method: <a href="#method">Method</a>, scheme: <a href="#scheme">Scheme</a>, name: &amp;str, store: Arc&lt;dyn <a href="#store">Store</a>&gt;) -&gt; Self</code> |  |
| <code>ready(&amp;self) -&gt; <a href="#result">Result</a>&lt;bool&gt;</code> |  |
| <code>scheme(&amp;self) -&gt; <a href="#scheme">Scheme</a></code> |  |
| <code>stamped(&amp;self) -&gt; Vec&lt;(String, String)&gt;</code> |  |
| <code>async token(&amp;self, http: &amp;Session) -&gt; <a href="#result">Result</a>&lt;String&gt;</code> |  |

**Trait implementations:** <code>Clone</code>


### `Prompt`

**Methods**

| Method | Description |
| --- | --- |
| <code>new(say: impl FnMut(&amp;str) + Send + 'static, ask: impl FnMut(String) -&gt; <a href="#answer">Answer</a> + Send + 'static) -&gt; Self</code> |  |
| <code>notice(&amp;mut self, line: &amp;str)</code> |  |
| <code>telling(say: impl FnMut(&amp;str) + Send + 'static) -&gt; Self</code> |  |


### `Spec`

**Methods**

| Method | Description |
| --- | --- |
| <code>check(&amp;self) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |

**Fields**

| Field | Description |
| --- | --- |
| <code>authorize_params: BTreeMap&lt;String, String&gt;</code> |  |
| <code>authorize_url: Option&lt;String&gt;</code> |  |
| <code>claim_headers: BTreeMap&lt;String, String&gt;</code> |  |
| <code>client_id: String</code> |  |
| <code>client_secret: Option&lt;String&gt;</code> |  |
| <code>device_url: Option&lt;String&gt;</code> |  |
| <code>exchange_url: Option&lt;String&gt;</code> |  |
| <code>grant: <a href="#grant">Grant</a></code> |  |
| <code>pkce: bool</code> |  |
| <code>redirect_uri: Option&lt;String&gt;</code> |  |
| <code>refresh_token: Option&lt;String&gt;</code> |  |
| <code>scope: Option&lt;String&gt;</code> |  |
| <code>token_params: BTreeMap&lt;String, String&gt;</code> |  |
| <code>token_style: <a href="#tokenstyle">TokenStyle</a></code> |  |
| <code>token_url: String</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Default</code>, <code>Debug</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


## Enums

### `Error`

**Fields**

| Field | Description |
| --- | --- |
| <code>NeedsLogin::detail: String</code> |  |
| <code>NeedsLogin::name: String</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>Config(String)</code> |  |
| <code>Denied(String)</code> |  |
| <code>NeedsLogin</code> |  |
| <code>Transport(String)</code> |  |

**Trait implementations:** <code>Error</code>, <code>Debug</code>, <code>Display</code>


### `Grant`

**Methods**

| Method | Description |
| --- | --- |
| <code>browser(self) -&gt; bool</code> |  |
| <code>interactive(self) -&gt; bool</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>AuthorizationCode</code> |  |
| <code>DeviceCode</code> |  |
| <code>RefreshToken</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


### `Method`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Key(String)</code> |  |
| <code>Login(Box&lt;<a href="#spec">Spec</a>&gt;)</code> |  |
| <code>Peer(fn(&amp;str) -&gt; Option&lt;String&gt;, String)</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Debug</code>


### `Scheme`

**Methods**

| Method | Description |
| --- | --- |
| <code>apply(self, request: RequestBuilder, token: &amp;str) -&gt; RequestBuilder</code> |  |

**Variants**

| Variant | Description |
| --- | --- |
| <code>ApiKey</code> |  |
| <code>Bearer</code> |  |
| <code>GoogleKey</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>


### `TokenStyle`

**Variants**

| Variant | Description |
| --- | --- |
| <code>Form</code> |  |
| <code>Json</code> |  |

**Trait implementations:** <code>Clone</code>, <code>Eq</code>, <code>PartialEq</code>, <code>Default</code>, <code>Debug</code>, <code>Copy</code>, <code>StructuralPartialEq</code>, <code>Serialize</code>, <code>Deserialize&lt;'de&gt;</code>


## Traits

### `Store`

**Methods**

| Method | Description |
| --- | --- |
| <code>claim(&amp;self, _name: &amp;str) -&gt; <a href="#result">Result</a>&lt;Box&lt;dyn Send&gt;&gt;</code> |  |
| <code>forget(&amp;self, name: &amp;str) -&gt; <a href="#result">Result</a>&lt;bool&gt;</code> |  |
| <code>get(&amp;self, name: &amp;str) -&gt; <a href="#result">Result</a>&lt;Option&lt;String&gt;&gt;</code> |  |
| <code>set(&amp;self, name: &amp;str, secret: &amp;str) -&gt; <a href="#result">Result</a>&lt;()&gt;</code> |  |


## Functions

### `builder`

<pre>pub fn <a href="#builder">builder</a>(timeout: Duration, connect: Duration) -&gt; SessionBuilder</pre>


### `http`

<pre>pub fn <a href="#http">http</a>(timeout: Duration, connect: Duration) -&gt; <a href="#result">Result</a>&lt;Session&gt;</pre>


## Type aliases

### `Answer`

<pre>pub type <a href="#answer">Answer</a> = Pin&lt;Box&lt;(dyn Future&lt;Output = Option&lt;String&gt;&gt; + Send)&gt;&gt;</pre>


### `Result`

<pre>pub type <a href="#result">Result</a>&lt;T&gt; = Result&lt;T, <a href="#error">Error</a>&gt;</pre>


## Constants

### `AGENT`

<pre>pub const <a href="#agent">AGENT</a>: &amp;str</pre>


---

Generated by truesight from `summonr-auth` 0.1.0-rc.1. See the [API overview](index.md).
