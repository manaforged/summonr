use std::sync::Arc;

use super::{Meta, ProviderMeta, Registry};
use crate::provider::Price;

#[derive(Clone, Copy)]
pub(super) struct BundledModel {
    context: u32,
    release: u32,
    id_off: u32,
    display_off: u32,
    family_off: u32,
    id_len: u16,
    display_len: u16,
    family_len: u16,
    price: u16,
    flags: u8,
}

#[derive(Clone, Copy)]
pub(super) struct BundledProvider {
    id_off: u32,
    name_off: u32,
    api_off: u32,
    models_off: u16,
    models_len: u16,
    id_len: u16,
    name_len: u16,
    api_len: u16,
}

include!(concat!(env!("OUT_DIR"), "/models_dev.rs"));

fn str_at(off: u32, len: u16) -> &'static str {
    let start = off as usize;
    &STRINGS[start..start + usize::from(len)]
}

fn opt_str_at(off: u32, len: u16) -> Option<&'static str> {
    (len != u16::MAX).then(|| str_at(off, len))
}

fn unpack_tri(bits: u8) -> Option<bool> {
    [None, Some(false), Some(true), None][usize::from(bits & 3)]
}

impl BundledModel {
    fn id(self) -> &'static str {
        str_at(self.id_off, self.id_len)
    }

    fn view(self) -> MetaView<'static> {
        MetaView {
            display: opt_str_at(self.display_off, self.display_len),
            context: (self.context != u32::MAX).then_some(self.context),
            metered: unpack_tri(self.flags >> 3),
            release: (self.release != u32::MAX).then_some(u64::from(self.release)),
            reasoning: (self.flags & 1) != 0,
            vision: (self.flags & 2) != 0,
            family: opt_str_at(self.family_off, self.family_len),
            deprecated: (self.flags & 4) != 0,
            tools: unpack_tri(self.flags >> 5),
            price: (self.price != u16::MAX).then(|| {
                let [input, output, cached, write] = PRICES[usize::from(self.price)];
                Price {
                    input: f64::from_bits(input),
                    output: f64::from_bits(output),
                    cached: f64::from_bits(cached),
                    cache_write: (write != u64::MAX).then(|| f64::from_bits(write)),
                }
            }),
        }
    }
}

impl BundledProvider {
    fn id(self) -> &'static str {
        str_at(self.id_off, self.id_len)
    }

    fn name(self) -> &'static str {
        str_at(self.name_off, self.name_len)
    }

    fn api(self) -> &'static str {
        str_at(self.api_off, self.api_len)
    }

    fn slice(self) -> &'static [BundledModel] {
        let start = usize::from(self.models_off);
        &MODELS[start..start + usize::from(self.models_len)]
    }
}

#[derive(Clone, Copy)]
pub(super) struct MetaView<'a> {
    pub(super) display: Option<&'a str>,
    pub(super) context: Option<u32>,
    pub(super) metered: Option<bool>,
    pub(super) release: Option<u64>,
    pub(super) reasoning: bool,
    pub(super) vision: bool,
    pub(super) family: Option<&'a str>,
    pub(super) deprecated: bool,
    pub(super) tools: Option<bool>,
    pub(super) price: Option<Price>,
}

impl<'a> MetaView<'a> {
    fn from_cached(meta: &'a Meta) -> Self {
        Self {
            display: meta.display.as_deref(),
            context: meta.context,
            metered: meta.metered,
            release: meta.release,
            reasoning: meta.reasoning,
            vision: meta.vision,
            family: meta.family.as_deref(),
            deprecated: meta.deprecated,
            tools: meta.tools,
            price: meta.price,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ProviderRef<'a> {
    Cached(&'a ProviderMeta),
    Bundled(&'static BundledProvider),
}

impl<'a> ProviderRef<'a> {
    pub(super) fn id(self) -> &'a str {
        match self {
            Self::Cached(provider) => &provider.id,
            Self::Bundled(provider) => provider.id(),
        }
    }

    pub(super) fn name(self) -> &'a str {
        match self {
            Self::Cached(provider) => &provider.name,
            Self::Bundled(provider) => provider.name(),
        }
    }

    pub(super) fn api(self) -> &'a str {
        match self {
            Self::Cached(provider) => &provider.api,
            Self::Bundled(provider) => provider.api(),
        }
    }

    pub(super) fn get(self, model: &str) -> Option<MetaView<'a>> {
        match self {
            Self::Cached(provider) => provider.models.get(model).map(MetaView::from_cached),
            Self::Bundled(provider) => {
                let slice = provider.slice();
                let index = slice
                    .binary_search_by(|row| row.id().as_bytes().cmp(model.as_bytes()))
                    .ok()?;
                Some(slice[index].view())
            }
        }
    }

    pub(super) fn models(self) -> ModelsIter<'a> {
        match self {
            Self::Cached(provider) => ModelsIter::Cached(provider.models.iter()),
            Self::Bundled(provider) => ModelsIter::Bundled(provider.slice().iter()),
        }
    }
}

pub(super) enum ModelsIter<'a> {
    Cached(std::collections::btree_map::Iter<'a, String, Meta>),
    Bundled(std::slice::Iter<'static, BundledModel>),
}

impl<'a> Iterator for ModelsIter<'a> {
    type Item = (&'a str, MetaView<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Cached(iter) => iter
                .next()
                .map(|(id, meta)| (id.as_str(), MetaView::from_cached(meta))),
            Self::Bundled(iter) => iter.next().map(|row| (row.id(), row.view())),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::catalog) enum Source<'a> {
    Cached(&'a Registry),
    Bundled,
}

impl<'a> Source<'a> {
    pub(super) fn is_empty(self) -> bool {
        match self {
            Self::Cached(registry) => registry.providers.is_empty(),
            Self::Bundled => PROVIDERS.is_empty(),
        }
    }

    pub(super) fn providers(self) -> ProvidersIter<'a> {
        match self {
            Self::Cached(registry) => ProvidersIter::Cached(registry.providers.iter()),
            Self::Bundled => ProvidersIter::Bundled(PROVIDERS.iter()),
        }
    }
}

pub(super) enum ProvidersIter<'a> {
    Cached(std::slice::Iter<'a, ProviderMeta>),
    Bundled(std::slice::Iter<'static, BundledProvider>),
}

impl<'a> Iterator for ProvidersIter<'a> {
    type Item = ProviderRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Cached(iter) => iter.next().map(ProviderRef::Cached),
            Self::Bundled(iter) => iter.next().map(ProviderRef::Bundled),
        }
    }
}

pub(in crate::catalog) enum Current {
    Cached(Arc<Registry>),
    Bundled,
}

impl Current {
    pub(in crate::catalog) fn as_source(&self) -> Source<'_> {
        match self {
            Self::Cached(registry) => Source::Cached(registry),
            Self::Bundled => Source::Bundled,
        }
    }
}
