use std::io::{self, Write};

use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{Attribute, Print, SetAttribute};
use sigil_tui::{CONTINUES, Grid, render};

pub(super) struct Selection {
    grid: Grid,
    cols: usize,
    start: (usize, usize),
    end: (usize, usize),
}

impl Selection {
    pub(super) fn new(grid: Grid, cols: u16, col: u16, row: u16) -> Self {
        let mut selection = Self {
            cols: usize::from(cols).min(grid.cols()),
            grid,
            start: (0, 0),
            end: (0, 0),
        };
        selection.move_to(col, row);
        selection.start = selection.end;
        selection
    }

    pub(super) fn move_to(&mut self, col: u16, row: u16) {
        let row = usize::from(row).min(self.grid.rows().saturating_sub(1));
        let mut col = usize::from(col).min(self.cols.saturating_sub(1));
        while col > 0
            && self
                .grid
                .get(col, row)
                .is_some_and(|cell| cell.ch == CONTINUES)
        {
            col -= 1;
        }
        self.end = (row, col);
    }

    pub(super) fn active(&self) -> bool {
        self.start != self.end && self.cols > 0 && self.grid.rows() > 0
    }

    pub(super) fn text(&self) -> Option<String> {
        if !self.active() {
            return None;
        }
        let (start, end) = (self.start.min(self.end), self.start.max(self.end));
        Some(
            (start.0..=end.0)
                .map(|row| {
                    let text: String = self
                        .grid
                        .row(row)
                        .iter()
                        .enumerate()
                        .filter(|(col, cell)| {
                            *col < self.cols
                                && (row, *col) >= start
                                && (row, *col) <= end
                                && cell.ch != CONTINUES
                        })
                        .map(|(_, cell)| cell.ch)
                        .collect();
                    text.trim_end().to_string()
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    pub(super) fn paint(&self, out: &mut impl Write) -> io::Result<()> {
        render(out, &Grid::new(0, 0), &self.grid)?;
        let (start, end) = (self.start.min(self.end), self.start.max(self.end));
        queue!(out, SetAttribute(Attribute::Reverse))?;
        for row in start.0..=end.0 {
            let first = if row == start.0 { start.1 } else { 0 };
            let last = if row == end.0 { end.1 + 1 } else { self.cols };
            let text: String = self.grid.row(row)[first..last]
                .iter()
                .filter(|cell| cell.ch != CONTINUES)
                .map(|cell| cell.ch)
                .collect();
            queue!(out, MoveTo(first as u16, row as u16), Print(text))?;
        }
        queue!(out, SetAttribute(Attribute::Reset))
    }
}

#[cfg(test)]
#[path = "selection_tests.rs"]
mod tests;
