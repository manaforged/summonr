use serde_json::Value;

use summonr_core::error::{Error, Result};

use super::args::parse_args;

pub(crate) use summonr_types::{Todo, TodoStatus};

pub(crate) fn todos_from_args(arguments: &str) -> Result<Vec<Todo>> {
    todos(&parse_args("todo", arguments)?)
}

fn todos(value: &Value) -> Result<Vec<Todo>> {
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

pub(super) fn parse_todo(args: &Value) -> Result<String> {
    let todos = todos(args)?;
    if todos.is_empty() {
        return Ok("no todos".to_string());
    }
    Ok(todos.iter().map(Todo::line).collect::<Vec<_>>().join("\n"))
}

#[cfg(test)]
#[path = "todo_tests.rs"]
mod tests;
