use serde_json::Value;

use crate::error::{Error, Result};

use super::args::parse_args;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TodoStatus {
    Pending,
    Active,
    Done,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Todo {
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

    pub(crate) fn ledger_line(&self) -> String {
        let mark = match self.status {
            TodoStatus::Done => "[x]",
            TodoStatus::Active => "[/]",
            TodoStatus::Pending => "[ ]",
        };
        format!("{mark} {}", self.text)
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
        let status = match item
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("pending")
        {
            "done" | "completed" => TodoStatus::Done,
            "active" | "in_progress" => TodoStatus::Active,
            _ => TodoStatus::Pending,
        };
        todos.push(Todo {
            text: text.to_string(),
            status,
        });
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
