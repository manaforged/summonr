use serde::{Deserialize, Serialize};

/// The direction a split lays its parts out in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    Right,
    Down,
}

/// How the window's panes share its space: one pane, or parts laid out along an axis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Pane(usize),
    Split { axis: Axis, parts: Vec<Part> },
}

/// One part of a split: its shape and its share of the split, relative to its siblings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub shape: Shape,
    #[serde(alias = "size")]
    pub share: u32,
}

/// The share of a part that fills its split alone.
pub const WHOLE: u32 = 10_000;

impl Default for Shape {
    fn default() -> Self {
        Self::Pane(0)
    }
}

impl Shape {
    /// `count` panes side by side, numbered from 0.
    #[must_use]
    pub fn row(count: usize) -> Self {
        if count <= 1 {
            return Self::Pane(0);
        }
        Self::Split {
            axis: Axis::Right,
            parts: (0..count).map(|at| part(at, WHOLE)).collect(),
        }
    }

    /// The panes in reading order.
    #[must_use]
    pub fn panes(&self) -> Vec<usize> {
        match self {
            Self::Pane(at) => vec![*at],
            Self::Split { parts, .. } => parts.iter().flat_map(|part| part.shape.panes()).collect(),
        }
    }

    /// Splits pane `at` along `axis`, giving pane `new` the far half of its space.
    pub fn split(&mut self, at: usize, axis: Axis, new: usize) -> bool {
        match self {
            Self::Pane(id) if *id == at => {
                *self = Self::Split {
                    axis,
                    parts: vec![part(at, WHOLE / 2), part(new, WHOLE / 2)],
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { axis: own, parts } => {
                let here = parts.iter().position(|part| part.shape == Self::Pane(at));
                if let (true, Some(i)) = (*own == axis, here) {
                    let half = (parts[i].share / 2).max(1);
                    parts[i].share = half;
                    parts.insert(i + 1, part(new, half));
                    return true;
                }
                parts.iter_mut().any(|part| part.shape.split(at, axis, new))
            }
        }
    }

    /// Removes pane `at`, gives its space to a neighbour, and renumbers the panes after it.
    pub fn remove(&mut self, at: usize) {
        self.drop_pane(at);
        self.renumber(at);
    }

    fn drop_pane(&mut self, at: usize) -> bool {
        let Self::Split { parts, .. } = self else {
            return false;
        };
        let Some(i) = parts.iter().position(|part| part.shape == Self::Pane(at)) else {
            return parts.iter_mut().any(|part| part.shape.drop_pane(at));
        };
        let gone = parts.remove(i);
        let heir = i.checked_sub(1).unwrap_or(0);
        if let Some(heir) = parts.get_mut(heir) {
            heir.share = heir.share.saturating_add(gone.share);
        }
        if parts.len() == 1 {
            *self = parts.remove(0).shape;
        }
        true
    }

    fn renumber(&mut self, gone: usize) {
        match self {
            Self::Pane(id) if *id > gone => *id -= 1,
            Self::Pane(_) => {}
            Self::Split { parts, .. } => {
                for part in parts {
                    part.shape.renumber(gone);
                }
            }
        }
    }

    /// Swaps the places of panes `a` and `b`.
    pub fn swap(&mut self, a: usize, b: usize) {
        match self {
            Self::Pane(id) if *id == a => *id = b,
            Self::Pane(id) if *id == b => *id = a,
            Self::Pane(_) => {}
            Self::Split { parts, .. } => {
                for part in parts {
                    part.shape.swap(a, b);
                }
            }
        }
    }

    /// Sets the shares of the split at `path` (part indices) from its parts' sizes in pixels.
    pub fn set_sizes(&mut self, path: &[usize], sizes: &[f32]) {
        let Self::Split { parts, .. } = self else {
            return;
        };
        match path.split_first() {
            Some((first, rest)) => {
                if let Some(part) = parts.get_mut(*first) {
                    part.shape.set_sizes(rest, sizes);
                }
            }
            None => {
                let total: f32 = sizes.iter().sum();
                if total <= 0.0 {
                    return;
                }
                for (part, size) in parts.iter_mut().zip(sizes) {
                    part.share = share(*size / total);
                }
            }
        }
    }
}

const fn part(at: usize, share: u32) -> Part {
    Part {
        shape: Shape::Pane(at),
        share,
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "a fraction from 0 to 1 scaled to WHOLE fits a u32"
)]
fn share(fraction: f32) -> u32 {
    ((fraction.clamp(0.0, 1.0) * WHOLE as f32).round() as u32).max(1)
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
