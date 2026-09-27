use crate::provider::Provider;
use crate::runtime::Loop;
use crate::verify::Outcome;
use crate::workflow::Role;

pub(crate) const CHECK: &str = "review";

const SYSTEM: &str = "You review one code change against its objective. Read the changed \
files with the tools; they are read-only. Reply with one line of JSON: {\"pass\": true|false, \
\"findings\": \"<what must change, or empty>\"}. Pass only when the change does what the \
objective asks and you can point to no defect, missing case, or leftover.";

pub(crate) async fn outcome<P: Provider>(
    loop_: &mut Loop<P>,
    objective: &str,
    reply: &str,
    evidence: &str,
    paths: &[String],
    usage: &mut crate::provider::Usage,
) -> std::result::Result<Outcome, String> {
    match loop_
        .aside(
            Role::Review,
            SYSTEM,
            &user(objective, reply, evidence, paths),
        )
        .await
    {
        Ok((text, review_usage)) => {
            usage.add(&review_usage);
            let verdict = parse(&text);
            Ok(Outcome {
                command: CHECK.to_string(),
                passed: verdict.pass,
                output: verdict.findings,
            })
        }
        Err(error) => Err(format!("review unavailable: {error}")),
    }
}

fn user(objective: &str, reply: &str, evidence: &str, paths: &[String]) -> String {
    let changed = if paths.is_empty() {
        "none".to_string()
    } else {
        paths
            .iter()
            .map(|path| format!("- {path}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Objective:\n{objective}\n\nHarness evidence:\n{evidence}\n\nChanged paths:\n{changed}\n\nAgent's latest response:\n{reply}"
    )
}

struct Verdict {
    pass: bool,
    findings: String,
}

fn parse(reply: &str) -> Verdict {
    let trimmed = reply.trim();
    let raw = || Verdict {
        pass: false,
        findings: trimmed.to_string(),
    };
    let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) else {
        return raw();
    };
    if end < start {
        return raw();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&trimmed[start..=end]) else {
        return raw();
    };
    let Some(pass) = value.get("pass").and_then(serde_json::Value::as_bool) else {
        return raw();
    };
    let findings = match (
        pass,
        value.get("findings").and_then(serde_json::Value::as_str),
    ) {
        (_, Some(findings)) => findings.to_string(),
        (true, None) => String::new(),
        (false, None) => trimmed.to_string(),
    };
    Verdict { pass, findings }
}

#[cfg(test)]
#[path = "review_tests.rs"]
mod tests;
