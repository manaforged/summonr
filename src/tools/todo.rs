use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

use super::args::parse_args;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    Active,
    Done,
}

impl TodoStatus {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "pending" => Some(Self::Pending),
            "active" | "in_progress" => Some(Self::Active),
            "done" | "completed" => Some(Self::Done),
            _ => None,
        }
    }

    pub(crate) fn acp(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "in_progress",
            Self::Done => "completed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Todo {
    pub text: String,
    pub status: TodoStatus,
}

impl Todo {
    pub(crate) fn mark(&self) -> &'static str {
        match self.status {
            TodoStatus::Done => "✓",
            TodoStatus::Active => "●",
            TodoStatus::Pending => "○",
        }
    }

    pub(crate) fn line(&self) -> String {
        format!("{} {}", self.mark(), self.text)
    }
}

pub(crate) fn todos_from_args(arguments: &str) -> Result<Vec<Todo>> {
    let value = parse_args("todo", arguments)?;
    let items = value
        .get("items")
        .or_else(|| value.get("todos"))
        .or_else(|| value.get("tasks"))
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .ok_or_else(|| Error::Tool("todo requires items".to_string()))?;
    let mut todos = Vec::new();
    for item in items {
        let text = item
            .get("text")
            .or_else(|| item.get("title"))
            .or_else(|| item.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| Error::Tool("todo item needs text".to_string()))?;
        let name = item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending");
        let status = TodoStatus::parse(name).ok_or_else(|| {
            Error::Tool(format!(
                "todo status `{name}` is not one of pending, active, done"
            ))
        })?;
        todos.push(Todo {
            text: text.to_string(),
            status,
        });
    }
    if todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::Active)
        .count()
        > 1
    {
        return Err(Error::Tool(
            "todo allows one active item; mark the others pending or done".to_string(),
        ));
    }
    Ok(todos)
}

pub(super) fn parse_todo(arguments: &str) -> Result<String> {
    let todos = todos_from_args(arguments)?;
    if todos.is_empty() {
        return Ok("no todos".to_string());
    }
    Ok(todos.iter().map(Todo::line).collect::<Vec<_>>().join("\n"))
}

#[cfg(test)]
#[path = "todo_tests.rs"]
mod tests;
