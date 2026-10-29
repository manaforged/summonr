use serde::de::DeserializeOwned;

pub fn parse<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    toml::from_str(text).map_err(|error| error.to_string())
}

pub fn embedded<T: DeserializeOwned>(name: &str, text: &str) -> T {
    match parse(text) {
        Ok(table) => table,
        Err(error) => panic!("the built-in {name} table is invalid: {error}"),
    }
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
