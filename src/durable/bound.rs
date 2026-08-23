use std::fmt;
use std::marker::PhantomData;
use std::slice::Iter;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::de::{Error as _, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum BoundError {
    #[error("payload has {actual} bytes; limit is {limit}")]
    Bytes { actual: usize, limit: usize },
    #[error("collection has {actual} items; limit is {limit}")]
    Items { actual: usize, limit: usize },
    #[error("payload is not valid base64: {0}")]
    Base64(String),
}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Text<const N: usize>(String);

impl<const N: usize> Text<N> {
    pub(crate) fn new(value: impl Into<String>) -> Result<Self, BoundError> {
        let value = value.into();
        if value.len() > N {
            return Err(BoundError::Bytes {
                actual: value.len(),
                limit: N,
            });
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl<const N: usize> fmt::Debug for Text<N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Text").field(&self.0).finish()
    }
}

impl<const N: usize> Serialize for Text<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de, const N: usize> Deserialize<'de> for Text<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Bytes<const N: usize>(Vec<u8>);

impl<const N: usize> Bytes<N> {
    pub(crate) fn new(value: impl Into<Vec<u8>>) -> Result<Self, BoundError> {
        let value = value.into();
        if value.len() > N {
            return Err(BoundError::Bytes {
                actual: value.len(),
                limit: N,
            });
        }
        Ok(Self(value))
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl<const N: usize> fmt::Debug for Bytes<N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Bytes")
            .field("length", &self.0.len())
            .finish()
    }
}

impl<const N: usize> Serialize for Bytes<N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de, const N: usize> Deserialize<'de> for Bytes<N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        let value = STANDARD
            .decode(encoded)
            .map_err(|error| D::Error::custom(BoundError::Base64(error.to_string())))?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Items<T, const N: usize>(Vec<T>);

impl<T, const N: usize> Default for Items<T, N> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T, const N: usize> Items<T, N> {
    pub(crate) fn new(value: Vec<T>) -> Result<Self, BoundError> {
        if value.len() > N {
            return Err(BoundError::Items {
                actual: value.len(),
                limit: N,
            });
        }
        Ok(Self(value))
    }

    pub(crate) fn as_slice(&self) -> &[T] {
        &self.0
    }

    pub(crate) fn iter(&self) -> Iter<'_, T> {
        self.0.iter()
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for Items<T, N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(&self.0).finish()
    }
}

impl<T: Serialize, const N: usize> Serialize for Items<T, N> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>, const N: usize> Deserialize<'de> for Items<T, N> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ItemsVisitor<T, const N: usize>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>, const N: usize> Visitor<'de> for ItemsVisitor<T, N> {
            type Value = Items<T, N>;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "at most {N} items")
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut value = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(N));
                while let Some(item) = sequence.next_element()? {
                    if value.len() == N {
                        return Err(A::Error::custom(BoundError::Items {
                            actual: N.saturating_add(1),
                            limit: N,
                        }));
                    }
                    value.push(item);
                }
                Ok(Items(value))
            }
        }

        deserializer.deserialize_seq(ItemsVisitor::<T, N>(PhantomData))
    }
}
