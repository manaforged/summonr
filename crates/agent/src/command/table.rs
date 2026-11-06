use std::num::ParseIntError;
use std::str::FromStr;

use summonr_core::error::{Error, Result};

pub enum Arity<T> {
    Switch(fn(&mut T) -> Result<()>),
    Value(&'static str, fn(&mut T, String) -> Result<()>),
    Rest(fn(&mut T, Vec<String>)),
}

pub struct Scope<T> {
    pub verb: &'static str,
    pub active: fn(&T) -> bool,
}

pub struct Row<T> {
    pub names: &'static [&'static str],
    pub arity: Arity<T>,
    pub scope: Option<Scope<T>>,
    pub once: bool,
    pub usage: &'static str,
    pub help: Option<&'static str>,
}

impl<T> Row<T> {
    #[must_use]
    pub const fn switch(names: &'static [&'static str], set: fn(&mut T) -> Result<()>) -> Self {
        Self::new(names, Arity::Switch(set))
    }

    #[must_use]
    pub const fn value(
        names: &'static [&'static str],
        expected: &'static str,
        set: fn(&mut T, String) -> Result<()>,
    ) -> Self {
        Self::new(names, Arity::Value(expected, set))
    }

    #[must_use]
    pub const fn rest(names: &'static [&'static str], set: fn(&mut T, Vec<String>)) -> Self {
        Self::new(names, Arity::Rest(set))
    }

    const fn new(names: &'static [&'static str], arity: Arity<T>) -> Self {
        Self {
            names,
            arity,
            scope: None,
            once: false,
            usage: "",
            help: None,
        }
    }

    #[must_use]
    pub const fn help(mut self, usage: &'static str, help: &'static str) -> Self {
        self.usage = usage;
        self.help = Some(help);
        self
    }

    #[must_use]
    pub const fn scope(mut self, verb: &'static str, active: fn(&T) -> bool) -> Self {
        self.scope = Some(Scope { verb, active });
        self
    }

    #[must_use]
    pub const fn once(mut self) -> Self {
        self.once = true;
        self
    }

    fn line(&self, help: &str) -> String {
        let (short, long) = match self.names {
            [short, long] => (format!("{short}, "), *long),
            names => (String::new(), names.first().copied().unwrap_or_default()),
        };
        let long = if self.usage.is_empty() {
            long.to_string()
        } else {
            format!("{long} {}", self.usage)
        };
        format!("  {short:<4}{long:<17}{help}")
    }
}

pub struct Table<T: 'static> {
    pub rows: &'static [Row<T>],
    pub missing: fn(&str, &str) -> String,
    pub unknown: fn(&str) -> String,
    pub error: fn(String) -> Error,
    pub dash_values: bool,
    pub pairs: bool,
}

impl<T: 'static> Table<T> {
    pub fn find(&self, word: &str, target: &T) -> Option<&'static Row<T>> {
        self.rows.iter().find(|row| {
            // complexity: bounded by the one or two names of a flag row
            row.names.contains(&word)
                && row
                    .scope
                    .as_ref()
                    .is_none_or(|scope| (scope.active)(target))
        })
    }

    pub fn scoped(&self, target: &T) -> Option<&'static str> {
        self.rows
            .iter()
            .filter_map(|row| row.scope.as_ref())
            .find(|scope| (scope.active)(target))
            .map(|scope| scope.verb)
    }

    pub fn verbs(&self, word: &str) -> Vec<&'static str> {
        self.rows
            .iter()
            // complexity: bounded by the one or two names of a flag row
            .filter(|row| row.names.contains(&word))
            .filter_map(|row| row.scope.as_ref().map(|scope| scope.verb))
            .collect()
    }

    pub fn apply(
        &self,
        row: &Row<T>,
        flag: &str,
        target: &mut T,
        input: &mut impl Iterator<Item = String>,
    ) -> Result<()> {
        match row.arity {
            Arity::Switch(set) => set(target),
            Arity::Rest(set) => {
                set(target, input.collect());
                Ok(())
            }
            Arity::Value(expected, set) => {
                let named = row
                    .names
                    .iter()
                    .find(|name| name.starts_with("--"))
                    .copied()
                    .unwrap_or(flag);
                let missing = (self.missing)(named, expected);
                let value = input.next().ok_or_else(|| (self.error)(missing.clone()))?;
                if !self.dash_values && value.starts_with('-') {
                    return Err((self.error)(format!("{missing}, not option `{value}`")));
                }
                set(target, value)
            }
        }
    }

    pub fn walk(&self, target: &mut T, words: impl IntoIterator<Item = String>) -> Result<()> {
        let mut input = words.into_iter().peekable();
        let mut seen: Vec<&'static str> = Vec::new();
        while let Some(flag) = input.next() {
            let Some(row) = self.find(&flag, target) else {
                if self.pairs && input.peek().is_none() {
                    return Err((self.error)((self.missing)(&flag, "")));
                }
                return Err((self.error)((self.unknown)(&flag)));
            };
            // complexity: bounded by the rows of one table
            if row.once && seen.contains(&row.names[0]) {
                return Err((self.error)((self.unknown)(&flag)));
            }
            seen.push(row.names[0]);
            self.apply(row, &flag, target, &mut input)?;
        }
        Ok(())
    }

    #[must_use]
    pub fn help_lines(&self) -> String {
        self.rows
            .iter()
            .filter_map(|row| row.help.map(|help| row.line(help)))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[must_use]
pub fn asks_help(words: &[&str]) -> bool {
    words.iter().any(|word| matches!(*word, "--help" | "-h"))
}

pub fn number<T: FromStr<Err = ParseIntError>>(text: &str) -> std::result::Result<T, String> {
    text.parse()
        .map_err(|error| format!("expected a number, got `{text}`: {error}"))
}
