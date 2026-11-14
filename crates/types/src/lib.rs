mod background;
/// The built-in tools table.
pub mod catalog;
mod child;
mod diff;
/// Endpoint wire protocols, regions, and access kinds.
pub mod endpoint;
mod event;
mod form;
/// Hook events, matchers, and payloads.
pub mod hooks;
/// MCP server config entries.
pub mod mcp;
mod message;
/// Built-in modules and their switches.
pub mod modules;
mod notify;
/// OAuth login settings.
pub mod oauth;
/// Plugin switches.
pub mod plugins;
/// Display preferences.
pub mod prefs;
mod reasoning;
mod receipt;
mod tool;
mod usage;
mod work;

pub use oauth::OAuth;

pub use child::{Access, ChildScope};
pub use event::{Abort, AbortCode, Event, Hold, Kept, StepState, progress_text};
pub use receipt::{Outcome, Receipt};
pub use work::{Todo, TodoStatus, WorkKind, WorkPhase, WorkProgress};

pub use background::Background;
pub use diff::DiffLayout;
pub use form::{Choice, Field, FieldKind, Form, FormAction, FormAnswer, TextFormat};
pub use notify::Notify;
pub use tool::{FunctionDefinition, Grant, Partial, ToolDefinition, ToolOutput, tool};

pub use message::{
    AssistantMessage, Checked, Continuation, Image, Message, Role, Source, StopReason, ToolCall,
    ToolFunction, ToolKind, UserInput, reasoning_field, vec_or_null,
};
pub use reasoning::{Effort, Off, Price};
pub use usage::Usage;
