use super::store::Task;

pub(crate) struct Launch<'a> {
    pub endpoint: Option<&'a str>,
    pub model: Option<&'a str>,
    pub session: Option<&'a str>,
    pub max_secs: Option<u64>,
    pub max_tokens: Option<u64>,
    pub worktree: bool,
    pub task: &'a Task,
}

impl Launch<'_> {
    pub(crate) fn argv(&self) -> Vec<String> {
        let mut args: Vec<String> = self.endpoint.iter().map(ToString::to_string).collect();
        let named = [
            ("--model", self.model.map(str::to_string)),
            ("--session", self.session.map(str::to_string)),
        ];
        let budget = [
            ("--max-secs", self.max_secs.map(|secs| secs.to_string())),
            (
                "--max-tokens",
                self.max_tokens.map(|tokens| tokens.to_string()),
            ),
        ];
        flags(&mut args, named);
        args.push("--json".to_string());
        flags(&mut args, budget);
        if self.worktree {
            args.push("--worktree".to_string());
        }
        let (task, text) = match self.task {
            Task::Prompt(prompt) => (["-p", "--"].as_slice(), prompt),
            Task::Goal(goal) => (["--goal"].as_slice(), goal),
        };
        args.extend(task.iter().map(ToString::to_string));
        args.push(text.clone());
        args
    }
}

fn flags(args: &mut Vec<String>, pairs: [(&str, Option<String>); 2]) {
    for (flag, value) in pairs {
        if let Some(value) = value {
            args.extend([flag.to_string(), value]);
        }
    }
}
