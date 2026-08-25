use serde::{Deserialize, Serialize};
use serde_json::{Error as JsonError, to_vec};

use super::bound::{Bytes, Items, Text};
use super::id::{
    CandidateId, ContextEntryId, ContextEpochId, Digest, EvidenceId, InputId, OperationId,
    PlanRevisionId, ProviderResultId, ToolResultId, WorkId,
};
use super::work::{WorkStage, WorkStatus};

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_IMAGE_BYTES: usize = 10 * 1_048_576;
const MAX_IMAGES: usize = 16;
const MAX_HEAD_BYTES: usize = 262_144;
pub(crate) const ENTRY_LIMIT: usize = 65_536;

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
    WorkPolicy(InputId),
    Provider(ProviderResultId),
    Receipt(ProviderResultId),
    Tool(ToolResultId),
    Plan(PlanRevisionId),
    Work(WorkId),
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
    pub(crate) model: Text<1_024>,
    pub(crate) system: Text<MAX_HEAD_BYTES>,
    pub(crate) digest: Digest,
}

impl RequestHead {
    pub(crate) fn calculated_digest(&self) -> Result<Digest, JsonError> {
        #[derive(Serialize)]
        struct Body<'a> {
            model: &'a Text<1_024>,
            system: &'a Text<MAX_HEAD_BYTES>,
        }
        let body = Body {
            model: &self.model,
            system: &self.system,
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
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub(crate) enum AbortReason {
    User,
    Budget,
    Shutdown,
}
