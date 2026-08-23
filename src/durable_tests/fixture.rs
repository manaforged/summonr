use std::path::Path;

use crate::durable::bound::{Bytes, Items, Text};
use crate::durable::context::{ContextEpoch, RequestHead};
use crate::durable::fact::{Fact, FactData};
use crate::durable::id::{ContextEpochId, Digest, FactId, ScopedId, SessionId};
use crate::durable::journal::JournalWriter;
use crate::durable::workspace::{
    NativeRoot, PathComponent, PathKey, PathPlatform, WorkspaceIdentity, WorkspaceScope,
};

pub(super) fn session() -> SessionId {
    SessionId::from_u128(1)
}

pub(super) fn id<K>(value: u128) -> ScopedId<K> {
    ScopedId::new(session(), value)
}

pub(super) fn text<const N: usize>(value: &str) -> Text<N> {
    Text::new(value).expect("fixture text must fit its declared bound")
}

pub(super) fn bytes<const N: usize>(value: &[u8]) -> Bytes<N> {
    Bytes::new(value.to_vec()).expect("fixture bytes must fit their declared bound")
}

pub(super) fn digest(label: &str) -> Digest {
    Digest::hash(b"mage:test:v1", label.as_bytes())
}

pub(super) fn unix_path(components: &[&[u8]]) -> PathKey {
    PathKey::new(
        PathPlatform::Unix,
        components
            .iter()
            .map(|component| {
                PathComponent::unix(component.to_vec())
                    .expect("fixture path component must be valid")
            })
            .collect(),
    )
    .expect("fixture path must be valid")
}

pub(super) fn scope() -> WorkspaceScope {
    let workspace = WorkspaceIdentity::new(
        NativeRoot::Unix(bytes(b"/")),
        vec![
            PathComponent::unix(b"tmp".to_vec()).expect("fixture component must be valid"),
            PathComponent::unix(b"mage".to_vec()).expect("fixture component must be valid"),
        ],
        digest("workspace-object"),
    )
    .expect("fixture workspace identity must be valid");
    WorkspaceScope::new(
        id(10),
        workspace,
        vec![PathKey::root(PathPlatform::Unix)],
        Vec::new(),
        Vec::new(),
    )
    .expect("fixture workspace scope must be valid")
}

pub(super) fn head() -> RequestHead {
    let mut head = RequestHead {
        adapter: text("openai-compatible"),
        request_format: 1,
        endpoint: text("https://example.test/v1"),
        model: text("test-model"),
        resource_version: None,
        body_overlay: text("{}"),
        system: text("You are Mage."),
        tool_schemas_digest: digest("tools"),
        output_settings: text("{}"),
        context_settings: text("{}"),
        streaming: true,
        serializer: text("serde-json-1"),
        digest: Digest::default(),
    };
    head.digest = head
        .calculated_digest()
        .expect("fixture request head must encode");
    head
}

pub(super) fn epoch() -> ContextEpoch {
    ContextEpoch {
        id: id(11),
        head: head(),
        stage: None,
        base: Items::default(),
    }
}

pub(super) fn root_facts() -> Vec<Fact> {
    vec![
        Fact {
            id: fact_id(1),
            data: FactData::SessionCreated {
                session: session(),
                scope: scope(),
            },
        },
        Fact {
            id: fact_id(2),
            data: FactData::ContextEpochOpened { epoch: epoch() },
        },
    ]
}

pub(super) fn fact_id(value: u128) -> FactId {
    id(value)
}

pub(super) fn epoch_id() -> ContextEpochId {
    id(11)
}

pub(super) fn writer(path: &Path) -> JournalWriter {
    let mut writer = JournalWriter::create(path).expect("fixture journal must be created");
    writer
        .append(id(1), None, root_facts())
        .expect("fixture root must append");
    writer
}
