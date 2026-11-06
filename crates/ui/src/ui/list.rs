use sigil::prelude::Signal;

pub(crate) fn shift(cursor: &Signal<usize>, delta: isize, len: usize) {
    cursor.set(
        cursor
            .peek()
            .min(len.saturating_sub(1))
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1)),
    );
}

pub(crate) fn cycle(at: usize, delta: isize, len: usize) -> usize {
    (at as isize + delta).rem_euclid(len.max(1) as isize) as usize
}
