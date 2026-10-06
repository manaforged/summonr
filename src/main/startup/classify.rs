use summonr::{Result, resolve_endpoint, resolve_session};

use super::commands::{session_path, workspace};
use crate::cli::Args;

async fn run_diff_review(
    args: &Args,
    asked: &std::collections::BTreeMap<String, summonr::classify::Question>,
    floor: f64,
) -> Result<i32> {
    let questions = if asked.is_empty() {
        summonr::classify::sets::diff()
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
    let changes = summonr::classify::patch::split(&patch);
    if changes.is_empty() {
        println!("no changes to review");
        return Ok(0);
    }
    let endpoint = classify_endpoint(args)?;
    let budget = summonr::classify::budget(&endpoint, &questions);
    let decided = summonr::classify::patch::review(&endpoint, &changes, &questions, budget).await?;
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
            summonr::classify::patch::table(&changes, &decided, floor)
        );
    }
    Ok(0)
}

async fn run_eval(
    args: &Args,
    questions: &std::collections::BTreeMap<String, summonr::classify::Question>,
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
    let examples = summonr::classify::eval::load(&text)?;
    let endpoint = classify_endpoint(args)?;
    let budget = summonr::classify::budget(&endpoint, questions);
    let decided = summonr::classify::eval::run(&endpoint, &examples, questions, budget).await?;
    let (tallies, misses) = summonr::classify::eval::score(&examples, &decided, floor)?;
    print!(
        "{}",
        summonr::classify::eval::report(&tallies, &misses, floor)
    );
    let clean = tallies.values().all(|tally| tally.gated_accuracy() >= 1.0);
    Ok(i32::from(!clean))
}

fn classify_endpoint(args: &Args) -> Result<summonr::Endpoint> {
    let name = match args.endpoint.as_deref() {
        Some(name) => name.to_string(),
        None => summonr::classify_endpoint().ok_or_else(|| {
            summonr::Error::Config(
                "no classify endpoint is set up; add an [endpoint.NAME] row with base_url and wire = \"decisions\" to ~/.summonr/config.toml, then `summonr login NAME`".to_string(),
            )
        })?,
    };
    resolve_endpoint(Some(&name), args.model.as_deref(), summonr::Need::Seat)
}

type Questions = std::collections::BTreeMap<String, summonr::classify::Question>;

pub(super) async fn run_classify(args: &Args) -> Result<i32> {
    let questions = classify_questions(args)?;
    let floor = args.gate.unwrap_or(0.7);
    if args.review_diff {
        return run_diff_review(args, &questions, floor).await;
    }
    if let Some(path) = args.eval.as_deref() {
        return run_eval(args, &questions, path, floor).await;
    }
    let state = classify_state(args)?;
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
    run_sweep(args, &questions, state).await
}

fn classify_questions(args: &Args) -> Result<Questions> {
    let mut questions: Questions = match args.questions.as_deref() {
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
        questions.extend(summonr::classify::sets::review());
    }
    if let Some(name) = args.set.as_deref() {
        let set = summonr::classify::sets::by_name(name).ok_or_else(|| {
            summonr::Error::Config(format!(
                "unknown question set `{name}`; try one of: {}",
                summonr::classify::sets::NAMES.join(", ")
            ))
        })?;
        questions.extend(set);
    }
    for (index, text) in args.noul.iter().enumerate() {
        questions.insert(
            format!("noul{}", index + 1),
            summonr::classify::Question::noul(text.clone()),
        );
    }
    Ok(questions)
}

fn classify_state(args: &Args) -> Result<String> {
    if args.session.is_some() || args.review {
        let path = match args.session.as_deref() {
            Some(id) => resolve_session(id)?,
            None => session_path(args)?,
        };
        return Ok(summonr::classify::digest::digest(&summonr::export_session(
            &path,
        )?));
    }
    if args.prompt.is_empty() {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)?;
        return Ok(input);
    }
    Ok(args.prompt.join(" "))
}

async fn run_sweep(args: &Args, questions: &Questions, state: String) -> Result<i32> {
    let endpoint = classify_endpoint(args)?;
    let budget = summonr::classify::budget(&endpoint, questions);
    let (decision, windows) = summonr::classify::sweep::sweep(
        &endpoint,
        &serde_json::Value::from(state),
        questions,
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
        print!("{}", summonr::classify::render(&decision));
    }
    Ok(0)
}
