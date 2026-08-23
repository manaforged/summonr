use std::array::from_fn;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

use crate::durable::id::{CommandId, Digest, Identity, OperationId, SessionId};

const HOT_LIMIT: usize = 128;
const FILTER_WORDS: usize = 256;
const FILTER_BITS: usize = FILTER_WORDS * u64::BITS as usize;
const FILTER_HASHES: usize = 4;

pub(super) trait Key: Copy + Eq + Hash {
    fn fingerprint(self) -> Digest;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct History<K: Eq + Hash, V> {
    hot: HashMap<K, V>,
    order: VecDeque<K>,
    filter: [u64; FILTER_WORDS],
}

impl<K: Eq + Hash, V> Default for History<K, V> {
    fn default() -> Self {
        Self {
            hot: HashMap::new(),
            order: VecDeque::new(),
            filter: [0; FILTER_WORDS],
        }
    }
}

impl<K: Key, V> History<K, V> {
    pub(super) fn get(&self, key: K) -> Option<&V> {
        self.hot.get(&key)
    }

    pub(super) fn might_contain(&self, key: K) -> bool {
        positions(key).into_iter().all(|position| {
            let word = position / u64::BITS as usize;
            let bit = position % u64::BITS as usize;
            self.filter[word] & (1_u64 << bit) != 0
        })
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        for position in positions(key) {
            let word = position / u64::BITS as usize;
            let bit = position % u64::BITS as usize;
            self.filter[word] |= 1_u64 << bit;
        }
        if self.hot.insert(key, value).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > HOT_LIMIT {
            if let Some(oldest) = self.order.pop_front() {
                self.hot.remove(&oldest);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.hot.len()
    }
}

impl Key for CommandId {
    fn fingerprint(self) -> Digest {
        scoped(b"mage:receipt-filter:v1", self.session(), self.local())
    }
}

impl Key for OperationId {
    fn fingerprint(self) -> Digest {
        scoped(b"mage:operation-filter:v1", self.session(), self.local())
    }
}

fn scoped(domain: &[u8], session: SessionId, local: Identity) -> Digest {
    let mut body = [0_u8; 32];
    body[..16].copy_from_slice(session.as_bytes());
    body[16..].copy_from_slice(local.as_bytes());
    Digest::hash(domain, &body)
}

fn positions<K: Key>(key: K) -> [usize; FILTER_HASHES] {
    let digest = key.fingerprint();
    from_fn(|index| {
        let offset = index * 2;
        (usize::from(digest.as_bytes()[offset]) << 8 | usize::from(digest.as_bytes()[offset + 1]))
            % FILTER_BITS
    })
}
