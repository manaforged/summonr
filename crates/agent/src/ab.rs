use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::command::table::{self, Row, Table};
use crate::cron::launch::Launch;
use crate::cron::store::Task;
use crate::process::{Io, Spec};
use crate::worktree::Tree;
use summonr_core::error::{Error, Result};

mod stats;

use stats::{Lift, Report, Run};

const USAGE: &str = "usage:\n  summonr ab --goal GOAL --sets A,B[,C] [--runs N] [-e|--endpoint E] [--model M]\n      [--max-secs N] [--max-tokens N] [--json]\n\nRuns GOAL N times under each skill set, in A B B A order. Each run starts from HEAD in its own git\nworktree, stops on the workspace check, and its worktree is removed after; nothing is merged.\n`none` is a set with no skills; when it is an arm, every other set is compared with it.\nThe raw events of each run are kept in the ab folder of the summonr home (SUMMONR_HOME).";

/// Runs per set when `--runs` is not given.
const RUNS: usize = 3;
/// The exit code a run uses for a harness error, which measures nothing.
const HARNESS_ERROR: i32 = 1;

#[derive(Default)]
struct Options {
    goal: Option<String>,
    sets: Vec<String>,
    runs: Option<usize>,
    endpoint: Option<String>,
    model: Option<String>,
    max_secs: Option<u64>,
    max_tokens: Option<u64>,
    json: bool,
}

type Flag = Row<Options>;

const ROWS: &[Flag] = &[
    Flag::switch(&["--json"], |options| {
        options.json = true;
        Ok(())
    }),
    Flag::value(&["--goal"], "", |options, value| {
        options.goal = Some(value);
        Ok(())
    }),
    Flag::value(&["--sets"], "", |options, value| {
        options.sets = value
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        Ok(())
    }),
    Flag::value(&["--runs"], "", |options, value| {
        options.runs = Some(number(&value)?);
        Ok(())
    }),
    Flag::value(&["--endpoint", "-e"], "", |options, value| {
        options.endpoint = Some(value);
        Ok(())
    }),
    Flag::value(&["--model"], "", |options, value| {
        options.model = Some(value);
        Ok(())
    }),
    Flag::value(&["--max-secs"], "", |options, value| {
        options.max_secs = Some(number(&value)?);
        Ok(())
    }),
    Flag::value(&["--max-tokens"], "", |options, value| {
        options.max_tokens = Some(number(&value)?);
        Ok(())
    }),
];

const FLAGS: Table<Options> = Table {
    rows: ROWS,
    missing: |flag, _| format!("{flag} needs a value"),
    unknown: |flag| format!("unknown option `{flag}`"),
    error: usage,
    dash_values: true,
    pairs: false,
};

/// `summonr ab`: one goal under several skill sets, repeated, with a paired report.
pub fn run(args: &[String], cwd: &Path) -> Result<i32> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    if words.is_empty() || table::asks_help(&words) {
        println!("{USAGE}");
        return Ok(0);
    }
    let mut options = Options::default();
    FLAGS.walk(&mut options, args.iter().cloned())?;
    let goal = options
        .goal
        .clone()
        .ok_or_else(|| usage("--goal is required".to_owned()))?;
    if options.sets.len() < 2 {
        return Err(usage(
            "--sets needs two or more sets, such as --sets lean,none".to_owned(),
        ));
    }
    for name in &options.sets {
        crate::skills::Set::named(name)?;
    }
    let check = crate::verify::check_path(cwd);
    if !check.is_file() {
        return Err(Error::Config(format!(
            "summonr ab stops each run on the workspace check; add one at {}",
            check.display()
        )));
    }
    let samples = samples_dir()?;
    let order = stats::order(&options.sets, options.runs.unwrap_or(RUNS));
    let mut runs = Vec::with_capacity(order.len());
    for (index, (round, set)) in order.iter().enumerate() {
        let run = trial(&options, &goal, cwd, &samples, (index, *round, set))?;
        eprintln!(
            "ab: run {}/{} {set}: {}",
            index + 1,
            order.len(),
            line(&run)
        );
        runs.push(run);
    }
    let report = Report::build(&options.sets, runs);
    if options.json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        print!("{}", text(&report));
        println!("samples: {}", samples.display());
    }
    Ok(0)
}

fn trial(
    options: &Options,
    goal: &str,
    cwd: &Path,
    samples: &Path,
    at: (usize, usize, &str),
) -> Result<Run> {
    let tree = crate::worktree::add(cwd)?;
    let run = run_in(&tree, options, goal, samples, at);
    let removed = crate::worktree::throw_away(&tree, "");
    let run = run?;
    removed?;
    Ok(run)
}

fn run_in(
    tree: &Tree,
    options: &Options,
    goal: &str,
    samples: &Path,
    at: (usize, usize, &str),
) -> Result<Run> {
    let (index, round, set) = at;
    if !crate::verify::check_path(&tree.path).is_file() {
        return Err(Error::Config(
            "the workspace check is not committed; summonr ab starts each run from HEAD".to_owned(),
        ));
    }
    let stem = samples.join(format!("{:03}-{set}", index + 1));
    let log = stem.with_extension("jsonl");
    let err = stem.with_extension("err");
    let task = Task::Goal(goal.to_owned());
    let argv = Launch {
        endpoint: options.endpoint.as_deref(),
        model: options.model.as_deref(),
        session: None,
        max_secs: options.max_secs,
        max_tokens: options.max_tokens,
        worktree: false,
        skills: Some(set),
        task: &task,
    }
    .argv();
    let started = Instant::now();
    let mut child = crate::process::spawn_blocking(Spec {
        cwd: Some(tree.path.clone()),
        stdin: Io::Null,
        stdout: Io::File(std::fs::File::create(&log)?),
        stderr: Io::File(std::fs::File::create(&err)?),
        ..Spec::new(std::env::current_exe()?, argv)
    })?;
    let code = child.wait()?.code();
    let secs = started.elapsed().as_secs_f64();
    if code == Some(HARNESS_ERROR) {
        return Err(Error::Config(format!(
            "run {} under `{set}` stopped on a harness error; see {}",
            index + 1,
            err.display()
        )));
    }
    Ok(Run::read(
        set,
        round,
        code,
        secs,
        &std::fs::read_to_string(&log)?,
    ))
}

fn samples_dir() -> Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis());
    let dir = summonr_core::dirs::summonr_home()?
        .join("ab")
        .join(stamp.to_string());
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn line(run: &Run) -> String {
    let verdict = if run.passed { "pass" } else { "fail" };
    format!(
        "{verdict} · {} tokens · {}",
        crate::provider::compact(run.tokens),
        secs(run.secs)
    )
}

fn text(report: &Report) -> String {
    let mut out = format!(
        "{:<16} {:>4} {:>6} {:>6} {:>8} {:>9} {:>7}  skills loaded\n",
        "set", "runs", "passed", "pass^k", "tokens", "cost", "time"
    );
    for arm in &report.sets {
        let skills = if arm.skills.is_empty() {
            "-".to_owned()
        } else {
            arm.skills
                .iter()
                .map(|(name, count)| format!("{name} {count}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let _ = writeln!(
            out,
            "{:<16} {:>4} {:>6} {:>6} {:>8} {:>9} {:>7}  {skills}",
            arm.name,
            arm.runs,
            format!("{}/{}", arm.passed, arm.runs),
            if arm.pass_all { "yes" } else { "no" },
            crate::provider::compact(arm.tokens.round() as u64),
            arm.cost
                .map_or_else(|| "-".to_owned(), crate::provider::dollars),
            secs(arm.secs),
        );
    }
    for pair in &report.pairs {
        let verdict = match pair.lift {
            Lift::Better => "passes more often",
            Lift::Worse => "passes less often",
            Lift::NoMeasuredLift => "no measured lift",
        };
        let _ = writeln!(
            out,
            "{} vs {}: pass {:+.0}% [{:+.0}%, {:+.0}%] · tokens {:+.0} [{:+.0}, {:+.0}] · {verdict}",
            pair.set,
            pair.against,
            pair.pass.diff * 100.0,
            pair.pass.low * 100.0,
            pair.pass.high * 100.0,
            pair.tokens.diff,
            pair.tokens.low,
            pair.tokens.high,
        );
    }
    out
}

fn secs(secs: f64) -> String {
    let whole = secs.round() as u64;
    if whole >= 60 {
        format!("{}m{:02}s", whole / 60, whole % 60)
    } else {
        format!("{whole}s")
    }
}

fn number<T: std::str::FromStr<Err = std::num::ParseIntError>>(text: &str) -> Result<T> {
    table::number(text).map_err(Error::Config)
}

fn usage(message: String) -> Error {
    Error::Setup(format!("{message}\n{USAGE}"))
}
