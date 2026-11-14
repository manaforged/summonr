use std::collections::BTreeMap;

use serde::de::DeserializeOwned;

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;

/// A row of a named table with an optional alias column.
pub trait Row: DeserializeOwned {
    /// The row's name.
    fn name(&self) -> &str;

    /// Names retired in favor of this row.
    fn retired(&self) -> &[String] {
        &[]
    }
}

/// Named rows with one index over names and aliases.
#[derive(Debug)]
pub struct Catalog<T: Row> {
    rows: Vec<T>,
    index: BTreeMap<String, usize>,
}

impl<T: Row> Catalog<T> {
    /// Builds the table from parsed rows, rejecting a name or alias listed twice.
    pub fn from_rows(label: &str, rows: Vec<T>) -> Result<Self, String> {
        let mut index = BTreeMap::new();
        for (at, row) in rows.iter().enumerate() {
            if index.insert(row.name().to_string(), at).is_some() {
                return Err(format!(
                    "`{}` is listed twice in the {label} table",
                    row.name()
                ));
            }
            for retired in row.retired() {
                if index.insert(retired.clone(), at).is_some() {
                    return Err(format!("`{retired}` is listed twice in the {label} table"));
                }
            }
        }
        Ok(Self { rows, index })
    }

    /// Finds the row with this name or retired alias.
    pub fn get(&self, name: &str) -> Option<&T> {
        self.index.get(name).map(|&at| &self.rows[at])
    }

    /// The rows in file order.
    pub fn rows(&self) -> &[T] {
        &self.rows
    }
}
