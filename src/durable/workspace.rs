use std::cmp::Ordering;
use std::mem::discriminant;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use super::bound::{BoundError, Bytes, Items, Text};
use super::id::{CoverageRuleId, Digest, ObservationId, WorkspaceScopeId};

const MAX_COMPONENT_BYTES: usize = 255;
const MAX_COMPONENTS: usize = 256;
const MAX_NATIVE_ROOT_BYTES: usize = 4_096;
const MAX_ROOTS: usize = 64;
const MAX_RESERVED_PATHS: usize = 128;
const MAX_ARTIFACT_ROOTS: usize = 64;
const MAX_OBSERVATION_ENTRIES: usize = 100_000;
const MAX_CHANGE_ENTRIES: usize = 100_000;
const MAX_RULES: usize = 128;
const MAX_REASON_BYTES: usize = 16_384;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum WorkspaceError {
    #[error(transparent)]
    Bound(#[from] BoundError),
    #[error("path component is empty, `.` or `..`")]
    Component,
    #[error("path component contains a platform separator or NUL")]
    Separator,
    #[error("path components do not match the path platform")]
    Platform,
    #[error("paths must be strictly sorted and unique")]
    Order,
    #[error("workspace scope exclusions overlap ambiguously")]
    Overlap,
    #[error("candidate root starts inside an excluded subtree")]
    CandidateExcluded,
    #[error("workspace observation digest does not match its canonical entries")]
    Digest,
    #[error("workspace observations do not share one scope and platform")]
    ObservationScope,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PathPlatform {
    Unix,
    Windows,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "units", content = "value", rename_all = "snake_case")]
pub(crate) enum PathComponent {
    Unix(Bytes<MAX_COMPONENT_BYTES>),
    Windows(Items<u16, MAX_COMPONENT_BYTES>),
}

impl<'de> Deserialize<'de> for PathComponent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(tag = "units", content = "value", rename_all = "snake_case")]
        enum Raw {
            Unix(Bytes<MAX_COMPONENT_BYTES>),
            Windows(Items<u16, MAX_COMPONENT_BYTES>),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Unix(value) => Self::unix(value.as_slice().to_vec()),
            Raw::Windows(value) => Self::windows(value.as_slice().to_vec()),
        }
        .map_err(serde::de::Error::custom)
    }
}

impl PathComponent {
    pub(crate) fn unix(value: impl Into<Vec<u8>>) -> Result<Self, WorkspaceError> {
        let value = Bytes::new(value)?;
        validate_unix(value.as_slice())?;
        Ok(Self::Unix(value))
    }

    pub(crate) fn windows(value: Vec<u16>) -> Result<Self, WorkspaceError> {
        let value = Items::new(value)?;
        validate_windows(value.as_slice())?;
        Ok(Self::Windows(value))
    }

    fn platform(&self) -> PathPlatform {
        match self {
            Self::Unix(_) => PathPlatform::Unix,
            Self::Windows(_) => PathPlatform::Windows,
        }
    }

    fn frame(&self, output: &mut Vec<u8>) {
        match self {
            Self::Unix(value) => {
                push_len(output, value.len());
                output.extend_from_slice(value.as_slice());
            }
            Self::Windows(value) => {
                push_len(output, value.len() * 2);
                for unit in value.iter() {
                    output.extend_from_slice(&unit.to_be_bytes());
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct PathKey {
    platform: PathPlatform,
    components: Items<PathComponent, MAX_COMPONENTS>,
}

impl<'de> Deserialize<'de> for PathKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            platform: PathPlatform,
            components: Items<PathComponent, MAX_COMPONENTS>,
        }

        let raw = Raw::deserialize(deserializer)?;
        Self::new(raw.platform, raw.components.as_slice().to_vec())
            .map_err(serde::de::Error::custom)
    }
}

impl PathKey {
    pub(crate) fn new(
        platform: PathPlatform,
        components: Vec<PathComponent>,
    ) -> Result<Self, WorkspaceError> {
        if components
            .iter()
            .any(|component| component.platform() != platform)
        {
            return Err(WorkspaceError::Platform);
        }
        Ok(Self {
            platform,
            components: Items::new(components)?,
        })
    }

    pub(crate) fn root(platform: PathPlatform) -> Self {
        Self {
            platform,
            components: Items::default(),
        }
    }

    pub(crate) fn platform(&self) -> PathPlatform {
        self.platform
    }

    pub(crate) fn components(&self) -> &[PathComponent] {
        self.components.as_slice()
    }

    pub(crate) fn canonical(&self) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(&1_u16.to_be_bytes());
        output.push(match self.platform {
            PathPlatform::Unix => 0,
            PathPlatform::Windows => 1,
        });
        push_len(&mut output, self.components.len());
        for component in self.components.iter() {
            component.frame(&mut output);
        }
        output
    }

    pub(crate) fn prefixes(&self, other: &Self) -> bool {
        self.platform == other.platform
            && self.components.len() <= other.components.len()
            && self
                .components
                .iter()
                .zip(other.components.iter())
                .all(|(left, right)| left == right)
    }

    pub(crate) fn parent(&self) -> Option<Self> {
        let (_, parent) = self.components.as_slice().split_last()?;
        Self::new(self.platform, parent.to_vec()).ok()
    }

    pub(crate) fn len(&self) -> usize {
        self.components.len()
    }

    pub(crate) fn last(&self) -> Option<&PathComponent> {
        self.components.as_slice().last()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "platform", content = "root", rename_all = "snake_case")]
pub(crate) enum NativeRoot {
    Unix(Bytes<MAX_NATIVE_ROOT_BYTES>),
    Windows(Items<u16, MAX_NATIVE_ROOT_BYTES>),
}

impl NativeRoot {
    fn platform(&self) -> PathPlatform {
        match self {
            Self::Unix(_) => PathPlatform::Unix,
            Self::Windows(_) => PathPlatform::Windows,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceIdentity {
    root: NativeRoot,
    components: Items<PathComponent, MAX_COMPONENTS>,
    object: Digest,
}

impl WorkspaceIdentity {
    pub(crate) fn new(
        root: NativeRoot,
        components: Vec<PathComponent>,
        object: Digest,
    ) -> Result<Self, WorkspaceError> {
        let platform = root.platform();
        if match &root {
            NativeRoot::Unix(value) => value.as_slice().is_empty(),
            NativeRoot::Windows(value) => value.is_empty(),
        } {
            return Err(WorkspaceError::Component);
        }
        if components
            .iter()
            .any(|component| component.platform() != platform)
        {
            return Err(WorkspaceError::Platform);
        }
        Ok(Self {
            root,
            components: Items::new(components)?,
            object,
        })
    }

    pub(crate) fn platform(&self) -> PathPlatform {
        self.root.platform()
    }

    pub(crate) fn root(&self) -> &NativeRoot {
        &self.root
    }

    pub(crate) fn components(&self) -> &[PathComponent] {
        self.components.as_slice()
    }

    pub(crate) fn object(&self) -> Digest {
        self.object
    }

    fn validate(&self) -> Result<(), WorkspaceError> {
        Self::new(
            self.root.clone(),
            self.components.as_slice().to_vec(),
            self.object,
        )
        .map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReservedCoverage {
    Entry,
    Subtree,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ReservedPath {
    path: PathKey,
    coverage: ReservedCoverage,
}

impl ReservedPath {
    pub(crate) fn new(path: PathKey, coverage: ReservedCoverage) -> Self {
        Self { path, coverage }
    }

    pub(crate) fn path(&self) -> &PathKey {
        &self.path
    }

    pub(crate) fn coverage(&self) -> ReservedCoverage {
        self.coverage
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ArtifactRoot {
    pub(crate) path: PathKey,
    pub(crate) rule: CoverageRuleId,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceScope {
    pub(crate) id: WorkspaceScopeId,
    pub(crate) workspace: WorkspaceIdentity,
    pub(crate) candidate_roots: Items<PathKey, MAX_ROOTS>,
    pub(crate) reserved: Items<ReservedPath, MAX_RESERVED_PATHS>,
    pub(crate) artifacts: Items<ArtifactRoot, MAX_ARTIFACT_ROOTS>,
}

impl WorkspaceScope {
    pub(crate) fn new(
        id: WorkspaceScopeId,
        workspace: WorkspaceIdentity,
        candidate_roots: Vec<PathKey>,
        reserved: Vec<ReservedPath>,
        artifacts: Vec<ArtifactRoot>,
    ) -> Result<Self, WorkspaceError> {
        workspace.validate()?;
        ordered_paths(&candidate_roots)?;
        ordered_by(&reserved, |item| &item.path)?;
        ordered_by(&artifacts, |item| &item.path)?;
        let platform = workspace.platform();
        if candidate_roots
            .iter()
            .any(|path| path.platform() != platform)
            || reserved.iter().any(|item| item.path.platform() != platform)
            || artifacts
                .iter()
                .any(|item| item.path.platform() != platform)
        {
            return Err(WorkspaceError::Platform);
        }
        if candidate_roots.iter().enumerate().any(|(index, left)| {
            candidate_roots[index + 1..]
                .iter()
                .any(|right| left.prefixes(right) || right.prefixes(left))
        }) {
            return Err(WorkspaceError::Overlap);
        }
        if artifacts.iter().enumerate().any(|(index, left)| {
            artifacts[index + 1..]
                .iter()
                .any(|right| left.path.prefixes(&right.path) || right.path.prefixes(&left.path))
        }) {
            return Err(WorkspaceError::Overlap);
        }
        if reserved.iter().enumerate().any(|(index, left)| {
            reserved[index + 1..]
                .iter()
                .any(|right| overlaps(&left.path, left.coverage, &right.path))
        }) {
            return Err(WorkspaceError::Overlap);
        }
        for pair in reserved.iter().flat_map(|left| {
            artifacts
                .iter()
                .map(move |right| (&left.path, left.coverage, &right.path))
        }) {
            if overlaps(pair.0, pair.1, pair.2) {
                return Err(WorkspaceError::Overlap);
            }
        }
        for candidate in &candidate_roots {
            let reserved_hit = reserved.iter().any(|item| {
                item.coverage == ReservedCoverage::Subtree && item.path.prefixes(candidate)
            });
            let artifact_hit = artifacts.iter().any(|item| item.path.prefixes(candidate));
            if reserved_hit || artifact_hit {
                return Err(WorkspaceError::CandidateExcluded);
            }
        }
        Ok(Self {
            id,
            workspace,
            candidate_roots: Items::new(candidate_roots)?,
            reserved: Items::new(reserved)?,
            artifacts: Items::new(artifacts)?,
        })
    }

    pub(crate) fn id(&self) -> WorkspaceScopeId {
        self.id
    }

    pub(crate) fn workspace(&self) -> &WorkspaceIdentity {
        &self.workspace
    }

    pub(crate) fn candidate_roots(&self) -> &[PathKey] {
        self.candidate_roots.as_slice()
    }

    pub(crate) fn reserved(&self) -> &[ReservedPath] {
        self.reserved.as_slice()
    }

    pub(crate) fn artifacts(&self) -> &[ArtifactRoot] {
        self.artifacts.as_slice()
    }

    pub(crate) fn validate(&self) -> Result<(), WorkspaceError> {
        Self::new(
            self.id,
            self.workspace.clone(),
            self.candidate_roots.as_slice().to_vec(),
            self.reserved.as_slice().to_vec(),
            self.artifacts.as_slice().to_vec(),
        )
        .map(|_| ())
    }

    pub(crate) fn platform(&self) -> PathPlatform {
        self.workspace.platform()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ObservedKind {
    Directory,
    File { content: Digest, executable: bool },
    Symlink { target: Digest },
}

impl ObservedKind {
    fn frame(&self, output: &mut Vec<u8>) {
        match self {
            Self::Directory => output.push(0),
            Self::File {
                content,
                executable,
            } => {
                output.push(1);
                output.extend_from_slice(content.as_bytes());
                output.push(u8::from(*executable));
            }
            Self::Symlink { target } => {
                output.push(2);
                output.extend_from_slice(target.as_bytes());
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ObservedEntry {
    path: PathKey,
    kind: ObservedKind,
}

impl ObservedEntry {
    pub(crate) fn new(path: PathKey, kind: ObservedKind) -> Self {
        Self { path, kind }
    }

    pub(crate) fn path(&self) -> &PathKey {
        &self.path
    }

    pub(crate) fn kind(&self) -> &ObservedKind {
        &self.kind
    }

    pub(crate) fn canonical(&self) -> Vec<u8> {
        let path = self.path.canonical();
        let mut output = Vec::new();
        push_len(&mut output, path.len());
        output.extend_from_slice(&path);
        self.kind.frame(&mut output);
        output
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct WorkspaceObservation {
    pub(crate) schema: u16,
    pub(crate) id: ObservationId,
    pub(crate) scope: WorkspaceScopeId,
    pub(crate) platform: PathPlatform,
    pub(crate) entries: Items<ObservedEntry, MAX_OBSERVATION_ENTRIES>,
    pub(crate) digest: Digest,
}

impl WorkspaceObservation {
    pub(crate) fn new(
        id: ObservationId,
        scope: WorkspaceScopeId,
        platform: PathPlatform,
        entries: Vec<ObservedEntry>,
    ) -> Result<Self, WorkspaceError> {
        ordered_by(&entries, ObservedEntry::path)?;
        if entries
            .iter()
            .any(|entry| entry.path.platform() != platform)
        {
            return Err(WorkspaceError::Platform);
        }
        let entries = Items::new(entries)?;
        let digest = observation_digest(platform, scope, &entries);
        Ok(Self {
            schema: 1,
            id,
            scope,
            platform,
            entries,
            digest,
        })
    }

    pub(crate) fn id(&self) -> ObservationId {
        self.id
    }

    pub(crate) fn scope(&self) -> WorkspaceScopeId {
        self.scope
    }

    pub(crate) fn digest(&self) -> Digest {
        self.digest
    }

    pub(crate) fn valid_digest(&self) -> bool {
        self.schema == 1
            && self.digest == observation_digest(self.platform, self.scope, &self.entries)
    }

    pub(crate) fn validate(&self) -> Result<(), WorkspaceError> {
        ordered_by(self.entries.as_slice(), ObservedEntry::path)?;
        if self
            .entries
            .iter()
            .any(|entry| entry.path.platform() != self.platform)
        {
            return Err(WorkspaceError::Platform);
        }
        if !self.valid_digest() {
            return Err(WorkspaceError::Digest);
        }
        Ok(())
    }

    pub(crate) fn changes(&self, next: &Self) -> Result<ChangeManifest, WorkspaceError> {
        if self.scope != next.scope || self.platform != next.platform {
            return Err(WorkspaceError::ObservationScope);
        }

        let mut before = self.entries.iter().peekable();
        let mut after = next.entries.iter().peekable();
        let mut entries = Vec::new();
        loop {
            match (before.peek(), after.peek()) {
                (Some(left), Some(right)) => match left.path.cmp(&right.path) {
                    Ordering::Less => {
                        entries.push(ChangeEntry::new(
                            left.path.clone(),
                            ChangeKind::Removed,
                            Some(left.kind.clone()),
                            None,
                        ));
                        before.next();
                    }
                    Ordering::Greater => {
                        entries.push(ChangeEntry::new(
                            right.path.clone(),
                            ChangeKind::Added,
                            None,
                            Some(right.kind.clone()),
                        ));
                        after.next();
                    }
                    Ordering::Equal => {
                        if let Some(change) = classify(&left.kind, &right.kind) {
                            entries.push(ChangeEntry::new(
                                left.path.clone(),
                                change,
                                Some(left.kind.clone()),
                                Some(right.kind.clone()),
                            ));
                        }
                        before.next();
                        after.next();
                    }
                },
                (Some(left), None) => {
                    entries.push(ChangeEntry::new(
                        left.path.clone(),
                        ChangeKind::Removed,
                        Some(left.kind.clone()),
                        None,
                    ));
                    before.next();
                }
                (None, Some(right)) => {
                    entries.push(ChangeEntry::new(
                        right.path.clone(),
                        ChangeKind::Added,
                        None,
                        Some(right.kind.clone()),
                    ));
                    after.next();
                }
                (None, None) => break,
            }
        }
        ChangeManifest::new(entries)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChangeKind {
    Added,
    Modified,
    Removed,
    KindChanged,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ChangeEntry {
    path: PathKey,
    change: ChangeKind,
    before: Option<ObservedKind>,
    after: Option<ObservedKind>,
}

impl ChangeEntry {
    pub(crate) fn new(
        path: PathKey,
        change: ChangeKind,
        before: Option<ObservedKind>,
        after: Option<ObservedKind>,
    ) -> Self {
        Self {
            path,
            change,
            before,
            after,
        }
    }

    fn path(&self) -> &PathKey {
        &self.path
    }

    fn canonical(&self) -> Vec<u8> {
        let path = self.path.canonical();
        let mut output = Vec::new();
        push_len(&mut output, path.len());
        output.extend_from_slice(&path);
        output.push(match self.change {
            ChangeKind::Added => 0,
            ChangeKind::Modified => 1,
            ChangeKind::Removed => 2,
            ChangeKind::KindChanged => 3,
        });
        push_kind(&mut output, self.before.as_ref());
        push_kind(&mut output, self.after.as_ref());
        output
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ChangeManifest {
    entries: Items<ChangeEntry, MAX_CHANGE_ENTRIES>,
    digest: Digest,
}

impl ChangeManifest {
    pub(crate) fn new(entries: Vec<ChangeEntry>) -> Result<Self, WorkspaceError> {
        ordered_by(&entries, ChangeEntry::path)?;
        let entries = Items::new(entries)?;
        let digest = manifest_digest(&entries);
        Ok(Self { entries, digest })
    }

    pub(crate) fn validate(&self) -> Result<(), WorkspaceError> {
        ordered_by(self.entries.as_slice(), ChangeEntry::path)?;
        if self.digest != manifest_digest(&self.entries) {
            return Err(WorkspaceError::Digest);
        }
        Ok(())
    }

    pub(crate) fn paths(&self) -> impl Iterator<Item = &PathKey> {
        self.entries.iter().map(ChangeEntry::path)
    }

    pub(crate) fn digest(&self) -> Digest {
        self.digest
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum WorkspaceEffect {
    None,
    Known {
        changes: ChangeManifest,
        artifact_rules: Items<CoverageRuleId, MAX_RULES>,
    },
    Unknown {
        reason: Text<MAX_REASON_BYTES>,
    },
}

impl WorkspaceEffect {
    pub(crate) fn validate(&self) -> Result<(), WorkspaceError> {
        if let Self::Known {
            changes,
            artifact_rules,
        } = self
        {
            changes.validate()?;
            if artifact_rules
                .as_slice()
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            {
                return Err(WorkspaceError::Order);
            }
        }
        Ok(())
    }
}

fn classify(before: &ObservedKind, after: &ObservedKind) -> Option<ChangeKind> {
    if before == after {
        None
    } else if discriminant(before) == discriminant(after) {
        Some(ChangeKind::Modified)
    } else {
        Some(ChangeKind::KindChanged)
    }
}

pub(crate) fn validate_paths(paths: &[PathKey]) -> Result<(), WorkspaceError> {
    ordered_paths(paths)
}

fn observation_digest(
    platform: PathPlatform,
    scope: WorkspaceScopeId,
    entries: &Items<ObservedEntry, MAX_OBSERVATION_ENTRIES>,
) -> Digest {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_be_bytes());
    body.push(match platform {
        PathPlatform::Unix => 0,
        PathPlatform::Windows => 1,
    });
    body.extend_from_slice(scope.session().as_bytes());
    body.extend_from_slice(scope.local().as_bytes());
    push_len(&mut body, entries.len());
    for entry in entries.iter() {
        let frame = entry.canonical();
        push_len(&mut body, frame.len());
        body.extend_from_slice(&frame);
    }
    Digest::hash(b"mage:workspace-observation:v1", &body)
}

fn manifest_digest(entries: &Items<ChangeEntry, MAX_CHANGE_ENTRIES>) -> Digest {
    let mut body = Vec::new();
    body.extend_from_slice(&1_u16.to_be_bytes());
    push_len(&mut body, entries.len());
    for entry in entries.iter() {
        let frame = entry.canonical();
        push_len(&mut body, frame.len());
        body.extend_from_slice(&frame);
    }
    Digest::hash(b"mage:change-manifest:v1", &body)
}

fn push_kind(output: &mut Vec<u8>, kind: Option<&ObservedKind>) {
    match kind {
        Some(kind) => {
            output.push(1);
            kind.frame(output);
        }
        None => output.push(0),
    }
}

fn validate_unix(value: &[u8]) -> Result<(), WorkspaceError> {
    if value.is_empty() || value == b"." || value == b".." {
        return Err(WorkspaceError::Component);
    }
    if value.iter().any(|byte| matches!(byte, 0 | b'/')) {
        return Err(WorkspaceError::Separator);
    }
    Ok(())
}

fn validate_windows(value: &[u16]) -> Result<(), WorkspaceError> {
    if value.is_empty() || value == [u16::from(b'.')] || value == [u16::from(b'.'), u16::from(b'.')]
    {
        return Err(WorkspaceError::Component);
    }
    if value.iter().any(|unit| matches!(*unit, 0 | 0x2f | 0x5c)) {
        return Err(WorkspaceError::Separator);
    }
    Ok(())
}

fn ordered_paths(paths: &[PathKey]) -> Result<(), WorkspaceError> {
    ordered_by(paths, |path| path)
}

fn ordered_by<T, K: Ord + ?Sized>(
    values: &[T],
    key: impl Fn(&T) -> &K,
) -> Result<(), WorkspaceError> {
    if values.windows(2).any(|pair| key(&pair[0]) >= key(&pair[1])) {
        return Err(WorkspaceError::Order);
    }
    Ok(())
}

fn overlaps(left: &PathKey, coverage: ReservedCoverage, right: &PathKey) -> bool {
    match coverage {
        ReservedCoverage::Entry => left == right,
        ReservedCoverage::Subtree => left.prefixes(right) || right.prefixes(left),
    }
}

fn push_len(output: &mut Vec<u8>, value: usize) {
    output.extend_from_slice(&u32::try_from(value).unwrap_or(u32::MAX).to_be_bytes());
}
