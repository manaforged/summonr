use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use thiserror::Error;

const ID_BYTES: usize = 16;
const DIGEST_BYTES: usize = 32;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum IdentityError {
    #[error("identity must contain exactly {expected} lowercase hexadecimal characters")]
    Length { expected: usize },
    #[error("identity contains a noncanonical hexadecimal character at byte {index}")]
    Alphabet { index: usize },
    #[error("scoped identity must be `<session>.<local>`")]
    Scope,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Identity([u8; ID_BYTES]);

impl Identity {
    pub(crate) fn from_u128(value: u128) -> Self {
        Self(value.to_be_bytes())
    }

    pub(crate) fn as_bytes(&self) -> &[u8; ID_BYTES] {
        &self.0
    }

    pub(crate) fn as_u128(self) -> u128 {
        u128::from_be_bytes(self.0)
    }

    fn parse(value: &str) -> Result<Self, IdentityError> {
        parse_hex::<ID_BYTES>(value).map(Self)
    }
}

impl fmt::Debug for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Identity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SessionId(Identity);

impl SessionId {
    pub(crate) fn from_u128(value: u128) -> Self {
        Self(Identity::from_u128(value))
    }

    pub(crate) fn as_bytes(&self) -> &[u8; ID_BYTES] {
        self.0.as_bytes()
    }

    pub(crate) fn path_component(&self) -> String {
        self.to_string()
    }
}

impl fmt::Debug for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl FromStr for SessionId {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Identity::parse(value).map(Self)
    }
}

impl Serialize for SessionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

pub(crate) struct ScopedId<K> {
    session: SessionId,
    local: Identity,
    marker: PhantomData<fn() -> K>,
}

impl<K> ScopedId<K> {
    pub(crate) fn new(session: SessionId, local: u128) -> Self {
        Self {
            session,
            local: Identity::from_u128(local),
            marker: PhantomData,
        }
    }

    pub(crate) fn session(self) -> SessionId {
        self.session
    }

    pub(crate) fn belongs(self, session: SessionId) -> bool {
        self.session == session
    }

    pub(crate) fn local(self) -> Identity {
        self.local
    }

    pub(crate) fn local_u128(self) -> u128 {
        self.local.as_u128()
    }

    pub(crate) fn path_component(self) -> String {
        self.local.to_string()
    }

    fn parse(value: &str) -> Result<Self, IdentityError> {
        let Some((session, local)) = value.split_once('.') else {
            return Err(IdentityError::Scope);
        };
        if local.contains('.') {
            return Err(IdentityError::Scope);
        }
        Ok(Self {
            session: session.parse()?,
            local: Identity::parse(local)?,
            marker: PhantomData,
        })
    }
}

impl<K> Clone for ScopedId<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for ScopedId<K> {}

impl<K> PartialEq for ScopedId<K> {
    fn eq(&self, other: &Self) -> bool {
        self.session == other.session && self.local == other.local
    }
}

impl<K> Eq for ScopedId<K> {}

impl<K> PartialOrd for ScopedId<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for ScopedId<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.session, self.local).cmp(&(other.session, other.local))
    }
}

impl<K> Hash for ScopedId<K> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.session.hash(state);
        self.local.hash(state);
    }
}

impl<K> fmt::Debug for ScopedId<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl<K> fmt::Display for ScopedId<K> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.session, self.local)
    }
}

impl<K> Serialize for ScopedId<K> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de, K> Deserialize<'de> for ScopedId<K> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Digest([u8; DIGEST_BYTES]);

impl Digest {
    pub(crate) fn from_bytes(value: [u8; DIGEST_BYTES]) -> Self {
        Self(value)
    }

    pub(crate) fn hash(domain: &[u8], body: &[u8]) -> Self {
        let mut hash = Sha256::new();
        hash.update(
            u64::try_from(domain.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        hash.update(domain);
        hash.update(u64::try_from(body.len()).unwrap_or(u64::MAX).to_be_bytes());
        hash.update(body);
        Self(hash.finalize().into())
    }

    pub(crate) fn as_bytes(&self) -> &[u8; DIGEST_BYTES] {
        &self.0
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(formatter, &self.0)
    }
}

impl FromStr for Digest {
    type Err = IdentityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        parse_hex::<DIGEST_BYTES>(value).map(Self)
    }
}

impl Serialize for Digest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

fn parse_hex<const N: usize>(value: &str) -> Result<[u8; N], IdentityError> {
    if value.len() != N * 2 {
        return Err(IdentityError::Length { expected: N * 2 });
    }
    let mut bytes = [0; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let offset = index * 2;
        let high =
            nibble(value.as_bytes()[offset]).ok_or(IdentityError::Alphabet { index: offset })?;
        let low = nibble(value.as_bytes()[offset + 1])
            .ok_or(IdentityError::Alphabet { index: offset + 1 })?;
        *byte = (high << 4) | low;
    }
    Ok(bytes)
}

fn nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

fn write_hex(formatter: &mut fmt::Formatter<'_>, bytes: &[u8]) -> fmt::Result {
    for byte in bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}

macro_rules! markers {
    ($($marker:ident => $name:ident),+ $(,)?) => {
        $(pub(crate) enum $marker {}
        pub(crate) type $name = ScopedId<$marker>;)+
    };
}

markers! {
    CommitMarker => CommitId,
    FactMarker => FactId,
    CommandMarker => CommandId,
    InputMarker => InputId,
    WorkMarker => WorkId,
    WorkItemMarker => WorkItemId,
    PlanMarker => PlanRevisionId,
    MaterializationMarker => MaterializationId,
    ForkMarker => ForkId,
    OperationMarker => OperationId,
    ProviderMarker => ProviderAttemptId,
    ProviderResultMarker => ProviderResultId,
    ToolBatchMarker => ToolBatchId,
    ToolCallMarker => ToolCallId,
    ToolAttemptMarker => ToolAttemptId,
    ToolResultMarker => ToolResultId,
    EpochMarker => ContextEpochId,
    ContextEntryMarker => ContextEntryId,
    ObservationMarker => ObservationId,
    WorkspaceScopeMarker => WorkspaceScopeId,
    CandidateMarker => CandidateId,
    EvidenceMarker => EvidenceId,
    CheckMarker => CheckId,
    VerificationPolicyMarker => VerificationPolicyId,
    CoverageRuleMarker => CoverageRuleId,
}
