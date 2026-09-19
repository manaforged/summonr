use summonr::{Result, resolve_endpoint, resolve_session};

use super::commands::{session_path, workspace};
use crate::cli::Args;

async fn run_diff_review(
    args: &Args,
    asked: &std::collections::BTreeMap<String, summonr::decisions::Question>,
    floor: f64,
) -> Result<i32> {
    let questions = if asked.is_empty() {
        summonr::decisions::sets::diff()
    } else {
        asked.clone()
    };
    let patch = if std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        summonr::diff::patch(&workspace()?, None)?
    } else {
        let mut piped = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut piped)?;
        piped
    };
    let changes = summonr::decisions::patch::split(&patch);
    if changes.is_empty() {
        println!("no changes to review");
        return Ok(0);
    }
    let endpoint = decisions_endpoint(args)?;
    let budget = summonr::decisions::budget(&endpoint, &questions);
    let decided =
        summonr::decisions::patch::review(&endpoint, &changes, &questions, budget).await?;
    if args.json {
        let rows: Vec<_> = changes
            .iter()
            .zip(&decided)
            .map(|(change, decision)| {
                serde_json::json!({"path": change.path, "answers": decision.answers})
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        print!(
            "{}",
            summonr::decisions::patch::table(&changes, &decided, floor)
        );
    }
    Ok(0)
}

async fn run_eval(
    args: &Args,
    questions: &std::collections::BTreeMap<String, summonr::decisions::Question>,
    path: &str,
    floor: f64,
) -> Result<i32> {
    if questions.is_empty() {
        return Err(summonr::Error::Config(
            "an evaluation needs questions: --set, --questions, or --noul".to_string(),
        ));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| summonr::Error::Config(format!("cannot read {path}: {error}")))?;
    let examples = summonr::decisions::eval::load(&text)?;
    let endpoint = decisions_endpoint(args)?;
    let budget = summonr::decisions::budget(&endpoint, questions);
    let decided = summonr::decisions::eval::run(&endpoint, &examples, questions, budget).await?;
    let (tallies, misses) = summonr::decisions::eval::score(&examples, &decided, floor)?;
    print!(
        "{}",
        summonr::decisions::eval::report(&tallies, &misses, floor)
    );
    let clean = tallies.values().all(|tally| tally.gated_accuracy() >= 1.0);
    Ok(i32::from(!clean))
}

fn decisions_endpoint(args: &Args) -> Result<summonr::Endpoint> {
    let name = match args.endpoint.as_deref() {
        Some(name) => name.to_string(),
        None => summonr::decisions_endpoint().ok_or_else(|| {
            summonr::Error::Config(format!(
                "no decisions endpoint has a key — set one of {}, or `summonr login NAME`",
                summonr::decisions_keys().join(", ")
            ))
        })?,
    };
    resolve_endpoint(Some(&name), args.model.as_deref())
}

pub(super) async fn run_decide(args: &Args) -> Result<i32> {
    let mut questions: std::collections::BTreeMap<String, summonr::decisions::Question> =
        match args.questions.as_deref() {
            Some(source) => {
                let text = match source.strip_prefix('@') {
                    Some(path) => std::fs::read_to_string(path).map_err(|error| {
                        summonr::Error::Config(format!("cannot read {path}: {error}"))
                    })?,
                    None => source.to_string(),
                };
                serde_json::from_str(&text)?
            }
            None => std::collections::BTreeMap::new(),
        };
    if args.review {
        questions.extend(summonr::decisions::sets::review());
    }
    if let Some(name) = args.set.as_deref() {
        let set = summonr::decisions::sets::by_name(name).ok_or_else(|| {
            summonr::Error::Config(format!(
                "unknown question set `{name}` — try one of: {}",
                summonr::decisions::sets::NAMES.join(", ")
            ))
        })?;
        questions.extend(set);
    }
    for (index, text) in args.noul.iter().enumerate() {
        questions.insert(
            format!("noul{}", index + 1),
            summonr::decisions::Question::noul(text.clone()),
        );
    }
    let floor = args.gate.unwrap_or(0.7);
    if args.review_diff {
        return run_diff_review(args, &questions, floor).await;
    }
    if let Some(path) = args.eval.as_deref() {
        return run_eval(args, &questions, path, floor).await;
    }
    let state = if args.session.is_some() || args.review {
        let path = match args.session.as_deref() {
            Some(id) => resolve_session(id)?,
            None => session_path(args)?,
        };
        summonr::decisions::digest::digest(&summonr::export_session(&path)?)
    } else if args.prompt.is_empty() {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
        input
    } else {
        args.prompt.join(" ")
    };
    if state.trim().is_empty() {
        return Err(summonr::Error::Config(
            "give it some state, as arguments, on stdin, or with --session".to_string(),
        ));
    }
    if questions.is_empty() {
        return Err(summonr::Error::Config(
            "ask something: --review, --noul, or --questions".to_string(),
        ));
    }
    let endpoint = decisions_endpoint(args)?;
    let budget = summonr::decisions::budget(&endpoint, &questions);
    let (decision, windows) = summonr::decisions::sweep::sweep(
        &endpoint,
        &serde_json::Value::from(state),
        &questions,
        budget,
    )
    .await?;
    if windows > 1 && !args.json {
        eprintln!("summonr: read the state in {windows} windows and merged the answers");
    }
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "model": decision.model,
                "answers": decision.answers,
                "usage": {
                    "input_tokens": decision.usage.input_tokens,
                    "output_tokens": decision.usage.output_tokens,
                    "cost": decision.usage.cost,
                },
            }))?
        );
    } else {
        print!("{}", summonr::decisions::render(&decision));
    }
    Ok(0)
}
