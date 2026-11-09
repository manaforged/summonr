use serde::de::DeserializeOwned;

pub fn parse<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    toml::from_str(text).map_err(|error| error.to_string())
}

/// Decodes a built-in table that the crate's build script compiled from TOML to JSON.
pub fn embedded<T: DeserializeOwned>(name: &str, json: &str) -> T {
    match serde_json::from_str(json) {
        Ok(table) => table,
        Err(error) => panic!("the built-in {name} table is invalid: {error}"),
    }
}

/// Loads the built-in table at `src/PATH` of the calling crate, compiled to JSON at build time.
#[macro_export]
macro_rules! embedded {
    ($name:expr, $path:literal) => {
        $crate::table::embedded(
            $name,
            include_str!(concat!(env!("OUT_DIR"), "/tables/", $path, ".json")),
        )
    };
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
