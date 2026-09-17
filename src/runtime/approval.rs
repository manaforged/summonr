use std::process::Command;

use super::*;
use crate::child::bounded;

const SYSTEM: &str = "You review one shell command a coding agent wants to run on the user's machine. \
The user wants the agent out of their way, but must not lose data or damage the machine. \
ALLOW when the command fits what the user asked for and its effects stay inside the project or are easy to undo. \
BLOCK when it destroys work that existed before the session and the user did not ask for that, \
deletes or overwrites data outside the project, downloads and runs code, sends secrets or private data off the machine, \
force-pushes or rewrites shared history, disables a safety check, \
or changes system, cloud, or production state the user did not name. \
A boundary the user stated in their messages always wins. \
Reply with exactly one line: ALLOW, or BLOCK [Rule]: reason. \
Rules: Destroy, Outside, Exfiltrate, Remote code, History, Safety, Infrastructure, Boundary.";
const USER_MESSAGES: usize = 20;
const TOOL_CALLS: usize = 30;
const CLIP: usize = 2000;
const CONSECUTIVE: usize = 3;
const TOTAL: usize = 20;

pub(super) enum Verdict {
    Allow,
    Block(String),
    Ask(String),
}

impl<P: Provider> Loop<P> {
    pub(super) async fn review(&mut self, command: &str, reason: &str) -> Verdict {
        let prompt = self.evidence(command, reason);
        let reply = match self.judge(SYSTEM, &prompt).await {
            Ok((reply, _)) => reply,
            Err(error) => format!("BLOCK [Unavailable]: the reviewer did not answer: {error}"),
        };
        let line = reply
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("BLOCK [Unclear]: the reviewer gave no verdict")
            .to_string();
        if line.to_ascii_uppercase().starts_with("ALLOW") {
            self.blocks.0 = 0;
            return Verdict::Allow;
        }
        self.blocks.0 += 1;
        self.blocks.1 += 1;
        let why = line
            .trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .trim()
            .trim_end_matches('.')
            .to_string();
        if self.blocks.0 >= CONSECUTIVE || self.blocks.1 >= TOTAL {
            self.blocks.0 = 0;
            if self.blocks.1 >= TOTAL {
                self.blocks.1 = 0;
            }
            return Verdict::Ask(why);
        }
        Verdict::Block(why)
    }

    fn evidence(&self, command: &str, reason: &str) -> String {
        let users: Vec<String> = self
            .history
            .iter()
            .filter(|message| message.role == Role::User)
            .map(|message| bounded(crate::session::user_text(&message.content), CLIP))
            .collect();
        let calls: Vec<String> = self
            .history
            .iter()
            .flat_map(|message| &message.tool_calls)
            .map(|call| format!("{} {}", call.function.name, bounded(&call.function.arguments, CLIP)))
            .collect();
        let status = Command::new("git")
            .arg("-C")
            .arg(&self.workspace)
            .args(["status", "--porcelain"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .take(40)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_else(|| "not a git repository".to_string());
        format!(
            "Workspace: {}\n\nUser messages, oldest first:\n{}\n\nEarlier tool calls (results withheld):\n{}\n\n\
             Uncommitted work (git status --porcelain):\n{}\n\n\
             A static check flagged this command because {reason}.\n\nCommand:\n{command}",
            self.workspace.display(),
            tail(&users, USER_MESSAGES),
            tail(&calls, TOOL_CALLS),
            if status.is_empty() { "clean" } else { &status },
        )
    }
}

fn tail(items: &[String], limit: usize) -> String {
    if items.is_empty() {
        return "none".to_string();
    }
    items[items.len().saturating_sub(limit)..]
        .iter()
        .map(|item| format!("- {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}
