use sigil::prelude::{Signal, signal};

#[derive(Clone, Copy)]
pub(in crate::tui) struct List {
    pub(in crate::tui) cursor: Signal<usize>,
    pub(in crate::tui) scroll: Signal<f32>,
    pub(in crate::tui) open: Signal<Vec<String>>,
    pub(in crate::tui) query: Signal<String>,
    pub(in crate::tui) caret: Signal<usize>,
}

impl List {
    pub(in crate::tui) fn new() -> Self {
        Self {
            cursor: signal(0),
            scroll: signal(0.0),
            open: signal(Vec::new()),
            query: signal(String::new()),
            caret: signal(0),
        }
    }

    pub(in crate::tui) fn at(self, len: usize) -> usize {
        at(&self.cursor, len)
    }

    pub(in crate::tui) fn shift(self, delta: isize, len: usize) {
        shift(&self.cursor, delta, len);
    }

    pub(in crate::tui) fn cycle(self, delta: isize, len: usize) {
        self.cursor.set(cycle(self.at(len), delta, len));
    }

    pub(in crate::tui) fn select(self, index: usize) {
        self.cursor.set(index);
    }

    pub(in crate::tui) fn is_open(self, key: &str) -> bool {
        self.open.peek().iter().any(|open| open == key)
    }

    pub(in crate::tui) fn toggle(self, key: &str) {
        let mut open = self.open.peek();
        match open.iter().position(|held| held == key) {
            Some(index) => drop(open.remove(index)),
            None => open.push(key.to_string()),
        }
        self.open.set(open);
    }

    pub(in crate::tui) fn toggle_only(self, key: &str) {
        let other = !self.is_open(key);
        self.open.set(if other {
            vec![key.to_string()]
        } else {
            Vec::new()
        });
    }

    pub(in crate::tui) fn close(self) {
        self.open.set(Vec::new());
    }

    pub(in crate::tui) fn type_query(self, text: String) {
        self.caret.set(text.len());
        self.query.set(text);
    }
}

fn at(cursor: &Signal<usize>, len: usize) -> usize {
    cursor.peek().min(len.saturating_sub(1))
}

pub(in crate::tui) fn shift(cursor: &Signal<usize>, delta: isize, len: usize) {
    cursor.set(
        at(cursor, len)
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1)),
    );
}

pub(in crate::tui) fn cycle(at: usize, delta: isize, len: usize) -> usize {
    (at as isize + delta).rem_euclid(len.max(1) as isize) as usize
}
