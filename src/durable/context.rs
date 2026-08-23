use serde::{Deserialize, Serialize};
use serde_json::{Error as JsonError, to_vec};

use super::bound::{Bytes, Items, Text};
use super::id::{
    CandidateId, ContextEntryId, ContextEpochId, Digest, DirectiveId, EvidenceId, InputId, MeterId,
    OperationId, PlanRevisionId, ProviderResultId, RetryId, ToolResultId, WorkId,
};
use super::work::{WorkStage, WorkStatus};

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_IMAGE_BYTES: usize = 10 * 1_048_576;
const MAX_IMAGES: usize = 16;
const MAX_HEAD_BYTES: usize = 262_144;
pub(crate) const ENTRY_LIMIT: usize = 65_536;
const MAX_OVERLAY_BYTES: usize = 65_536;
const MAX_DIAGNOSTIC_BYTES: usize = 65_536;
const MAX_INPUT_DISPOSITIONS: usize = 1_024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ImagePayload {
    pub(crate) media_type: Text<256>,
    pub(crate) data: Bytes<MAX_IMAGE_BYTES>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct UserPayload {
    pub(crate) text: Text<MAX_TEXT_BYTES>,
    pub(crate) images: Items<ImagePayload, MAX_IMAGES>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputOrigin {
    User,
    Peer,
    WorkPolicy,
    HarnessControl,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InputRoute {
    Start,
    Steer,
    Queue,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub(crate) enum RouteReason {
    User,
    Busy,
    OperationClosed,
    Policy,
    Other(Text<MAX_DIAGNOSTIC_BYTES>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case")]
pub(crate) enum DirectiveState {
    Active,
    Satisfied { evidence: Option<EvidenceId> },
    Superseded { by: DirectiveId },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Directive {
    pub(crate) id: DirectiveId,
    pub(crate) text: Text<MAX_TEXT_BYTES>,
    pub(crate) state: DirectiveState,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "source", content = "id", rename_all = "snake_case")]
pub(crate) enum ContextSource {
    Input(InputId),
    Provider(ProviderResultId),
    Tool(ToolResultId),
    Plan(PlanRevisionId),
    Work(WorkId),
    Directive(DirectiveId),
    Evidence(EvidenceId),
    Summary(ProviderResultId),
    HarnessControl,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ContextEntry {
    pub(crate) id: ContextEntryId,
    pub(crate) epoch: ContextEpochId,
    pub(crate) role: ContextRole,
    pub(crate) source: ContextSource,
    pub(crate) payload: Text<MAX_TEXT_BYTES>,
    pub(crate) digest: Digest,
}

impl ContextEntry {
    pub(crate) fn calculated_digest(&self) -> Digest {
        Digest::hash(b"mage:context-entry:v1", self.payload.as_str().as_bytes())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct RequestHead {
    pub(crate) adapter: Text<256>,
    pub(crate) request_format: u16,
    pub(crate) endpoint: Text<2_048>,
    pub(crate) model: Text<1_024>,
    pub(crate) resource_version: Option<Text<1_024>>,
    pub(crate) body_overlay: Text<MAX_OVERLAY_BYTES>,
    pub(crate) system: Text<MAX_HEAD_BYTES>,
    pub(crate) tool_schemas_digest: Digest,
    pub(crate) output_settings: Text<MAX_OVERLAY_BYTES>,
    pub(crate) context_settings: Text<MAX_OVERLAY_BYTES>,
    pub(crate) streaming: bool,
    pub(crate) serializer: Text<256>,
    pub(crate) digest: Digest,
}

impl RequestHead {
    pub(crate) fn calculated_digest(&self) -> Result<Digest, JsonError> {
        #[derive(Serialize)]
        struct Body<'a> {
            adapter: &'a Text<256>,
            request_format: u16,
            endpoint: &'a Text<2_048>,
            model: &'a Text<1_024>,
            resource_version: &'a Option<Text<1_024>>,
            body_overlay: &'a Text<MAX_OVERLAY_BYTES>,
            system: &'a Text<MAX_HEAD_BYTES>,
            tool_schemas_digest: Digest,
            output_settings: &'a Text<MAX_OVERLAY_BYTES>,
            context_settings: &'a Text<MAX_OVERLAY_BYTES>,
            streaming: bool,
            serializer: &'a Text<256>,
        }
        let body = Body {
            adapter: &self.adapter,
            request_format: self.request_format,
            endpoint: &self.endpoint,
            model: &self.model,
            resource_version: &self.resource_version,
            body_overlay: &self.body_overlay,
            system: &self.system,
            tool_schemas_digest: self.tool_schemas_digest,
            output_settings: &self.output_settings,
            context_settings: &self.context_settings,
            streaming: self.streaming,
            serializer: &self.serializer,
        };
        to_vec(&body).map(|bytes| Digest::hash(b"mage:request-head:v1", &bytes))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ContextEpoch {
    pub(crate) id: ContextEpochId,
    pub(crate) head: RequestHead,
    pub(crate) stage: Option<WorkStage>,
    pub(crate) base: Items<ContextEntryId, ENTRY_LIMIT>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationOutcome {
    Completed,
    Failed,
    Cancelled,
    Exhausted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct OperationClose {
    pub(crate) id: OperationId,
    pub(crate) outcome: OperationOutcome,
    pub(crate) provider: Option<ProviderResultId>,
    pub(crate) work: Option<WorkId>,
    pub(crate) work_status: Option<WorkStatus>,
    pub(crate) plan: Option<PlanRevisionId>,
    pub(crate) candidate: Option<CandidateId>,
    pub(crate) verdict: Option<EvidenceId>,
    pub(crate) usage_digest: Digest,
    pub(crate) inputs: Items<InputDisposition, MAX_INPUT_DISPOSITIONS>,
    pub(crate) diagnostic: Option<Text<MAX_DIAGNOSTIC_BYTES>>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "disposition", content = "input", rename_all = "snake_case")]
pub(crate) enum InputDisposition {
    Applied(InputId),
    Queued(InputId),
    Cancelled(InputId),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub(crate) enum AbortReason {
    User,
    Signal,
    Budget,
    Shutdown,
    Other(Text<MAX_DIAGNOSTIC_BYTES>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub(crate) enum RetryReason {
    RateLimit,
    Transport,
    OverflowRebase,
    Other(Text<MAX_DIAGNOSTIC_BYTES>),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RetryOutcome {
    Elapsed,
    Cancelled,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct RetryIntent {
    pub(crate) id: RetryId,
    pub(crate) operation: OperationId,
    pub(crate) reservation: MeterId,
    pub(crate) reason: RetryReason,
    pub(crate) not_before_unix_millis: u64,
    pub(crate) deadline_unix_millis: u64,
    pub(crate) next_request_digest: Digest,
}
