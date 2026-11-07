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

/// One part of a split: its shape and its size in pixels; the last part takes the rest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Part {
    pub shape: Shape,
    pub size: u32,
}

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
            parts: (0..count)
                .map(|at| Part {
                    shape: Self::Pane(at),
                    size: 0,
                })
                .collect(),
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

    /// Splits pane `at` along `axis`, giving pane `new` the far half of its `extent` pixels.
    pub fn split(&mut self, at: usize, axis: Axis, new: usize, extent: f32) -> bool {
        let half = half(extent);
        match self {
            Self::Pane(id) if *id == at => {
                *self = Self::Split {
                    axis,
                    parts: vec![part(at, half), part(new, half)],
                };
                true
            }
            Self::Pane(_) => false,
            Self::Split { axis: own, parts } => {
                let here = parts.iter().position(|part| part.shape == Self::Pane(at));
                if let (true, Some(i)) = (*own == axis, here) {
                    parts[i].size = half;
                    parts.insert(i + 1, part(new, half));
                    return true;
                }
                parts
                    .iter_mut()
                    .any(|part| part.shape.split(at, axis, new, extent))
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
            heir.size = heir.size.saturating_add(gone.size);
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

    /// Sets the part sizes of the split found by following the part indices in `path`.
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
                for (part, size) in parts.iter_mut().zip(sizes) {
                    part.size = pixels(*size);
                }
            }
        }
    }
}

const fn part(at: usize, size: u32) -> Part {
    Part {
        shape: Shape::Pane(at),
        size,
    }
}

fn half(extent: f32) -> u32 {
    pixels(extent / 2.0)
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a pane size is a small, non-negative pixel count"
)]
fn pixels(size: f32) -> u32 {
    size.max(0.0).round() as u32
}

#[cfg(test)]
#[path = "shape_tests.rs"]
mod tests;
