use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

mod check;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Form {
    pub server: String,
    pub message: String,
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub kind: FieldKind,
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FieldKind {
    Text {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<TextFormat>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_length: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pattern: Option<String>,
    },
    Number {
        integer: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        minimum: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        maximum: Option<f64>,
    },
    Boolean,
    Choice {
        options: Vec<Choice>,
    },
    Choices {
        options: Vec<Choice>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min_items: Option<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_items: Option<usize>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextFormat {
    Email,
    Uri,
    Date,
    DateTime,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    pub title: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormAction {
    Accept,
    Decline,
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FormAnswer {
    pub action: FormAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Map<String, Value>>,
}

impl FormAnswer {
    #[must_use]
    pub const fn declined() -> Self {
        Self {
            action: FormAction::Decline,
            content: None,
        }
    }

    #[must_use]
    pub const fn cancelled() -> Self {
        Self {
            action: FormAction::Cancel,
            content: None,
        }
    }

    #[must_use]
    pub fn wire(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({"action": "cancel"}))
    }
}

impl Form {
    pub(crate) fn parse(server: &str, params: &Value) -> Option<Self> {
        if params
            .get("mode")
            .and_then(Value::as_str)
            .is_some_and(|mode| mode != "form")
        {
            return None;
        }
        let schema = params.get("requestedSchema")?;
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let fields = schema
            .get("properties")?
            .as_object()?
            .iter()
            .map(|(name, property)| Field::parse(name, property, required.contains(&name.as_str())))
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            server: server.to_string(),
            message: params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            fields,
        })
    }

    pub fn check(&self, content: &Map<String, Value>) -> Result<(), String> {
        for field in &self.fields {
            match content.get(&field.name) {
                Some(value) => field.check(value)?,
                None if field.required => return Err(format!("{} is required", field.title)),
                None => {}
            }
        }
        match content
            .keys()
            .find(|key| !self.fields.iter().any(|field| &field.name == *key))
        {
            Some(extra) => Err(format!("{extra} is not a field of this form")),
            None => Ok(()),
        }
    }
}

impl Field {
    fn parse(name: &str, property: &Value, required: bool) -> Option<Self> {
        let text = |key: &str| {
            property
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_string)
        };
        let count = |key: &str| {
            property
                .get(key)
                .and_then(Value::as_u64)
                .and_then(|count| usize::try_from(count).ok())
        };
        let kind = match property.get("type").and_then(Value::as_str)? {
            "string" => match choices(property) {
                Some(options) => FieldKind::Choice { options },
                None => FieldKind::Text {
                    format: property.get("format").and_then(|format| {
                        serde_json::from_value::<TextFormat>(format.clone()).ok()
                    }),
                    min_length: count("minLength"),
                    max_length: count("maxLength"),
                    pattern: text("pattern"),
                },
            },
            kind @ ("number" | "integer") => FieldKind::Number {
                integer: kind == "integer",
                minimum: property.get("minimum").and_then(Value::as_f64),
                maximum: property.get("maximum").and_then(Value::as_f64),
            },
            "boolean" => FieldKind::Boolean,
            "array" => FieldKind::Choices {
                options: choices(property.get("items")?)?,
                min_items: count("minItems"),
                max_items: count("maxItems"),
            },
            _other => return None,
        };
        Some(Self {
            name: name.to_string(),
            title: text("title").unwrap_or_else(|| name.to_string()),
            description: text("description"),
            kind,
            required,
            default: property.get("default").cloned(),
        })
    }

    pub fn read(&self, typed: &str) -> Result<Option<Value>, String> {
        let typed = typed.trim();
        if typed.is_empty() {
            return match (&self.default, self.required) {
                (Some(default), _) => Ok(Some(default.clone())),
                (None, true) => Err(format!("{} is required", self.title)),
                (None, false) => Ok(None),
            };
        }
        let value = match &self.kind {
            FieldKind::Text { .. } => json!(typed),
            FieldKind::Number { integer: true, .. } => typed
                .parse::<i64>()
                .map(|number| json!(number))
                .map_err(|_invalid| format!("{} takes a whole number", self.title))?,
            FieldKind::Number { .. } => typed
                .parse::<f64>()
                .map(|number| json!(number))
                .map_err(|_invalid| format!("{} takes a number", self.title))?,
            FieldKind::Boolean => match typed.to_ascii_lowercase().as_str() {
                "y" | "yes" | "true" => json!(true),
                "n" | "no" | "false" => json!(false),
                _other => return Err(format!("{} takes yes or no", self.title)),
            },
            FieldKind::Choice { options } => json!(pick(options, typed)?),
            FieldKind::Choices { options, .. } => json!(
                typed
                    .split(',')
                    .map(|part| pick(options, part.trim()))
                    .collect::<Result<Vec<_>, _>>()?
            ),
        };
        self.check(&value)?;
        Ok(Some(value))
    }
}

fn choices(property: &Value) -> Option<Vec<Choice>> {
    if let Some(values) = property.get("enum").and_then(Value::as_array) {
        return values
            .iter()
            .map(|value| {
                value.as_str().map(|value| Choice {
                    value: value.to_string(),
                    title: value.to_string(),
                })
            })
            .collect();
    }
    let titled = property
        .get("oneOf")
        .or_else(|| property.get("anyOf"))?
        .as_array()?;
    titled
        .iter()
        .map(|option| {
            let value = option.get("const")?.as_str()?.to_string();
            let title = option
                .get("title")
                .and_then(Value::as_str)
                .map_or_else(|| value.clone(), str::to_string);
            Some(Choice { value, title })
        })
        .collect()
}

fn pick(options: &[Choice], typed: &str) -> Result<String, String> {
    let lower = typed.to_lowercase();
    options
        .iter()
        .find(|option| option.value == typed || option.title.to_lowercase() == lower)
        .or_else(|| {
            typed
                .parse::<usize>()
                .ok()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| options.get(index))
        })
        .map(|option| option.value.clone())
        .ok_or_else(|| format!("`{typed}` is not one of the choices"))
}

#[cfg(test)]
#[path = "form_tests.rs"]
mod tests;
