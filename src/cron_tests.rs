use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use jiff::Zoned;
use jiff::civil::DateTime;
use jiff::tz::TimeZone;

use super::install::crontab::{cron_line, crontab_without};
use super::install::{TickEnv, domains, plist, units};
use super::refuse;
use super::runner::{Due, due};
use super::schedule::parse;
use super::store::{Job, Notify, Outcome, Task};

const CHICAGO: &str = "America/Chicago";

fn bare(path: &str) -> TickEnv<'_> {
    TickEnv {
        path,
        home: None,
        summonr_home: None,
    }
}

fn at(text: &str) -> Zoned {
    text.parse::<DateTime>()
        .unwrap()
        .to_zoned(TimeZone::get(CHICAGO).unwrap())
        .unwrap()
}

fn next(expr: &str, after: Zoned) -> String {
    let next = parse(expr).unwrap().next_after(after).unwrap().to_string();
    next.strip_suffix("[America/Chicago]").unwrap().to_string()
}

fn job(schedule: &str, created: i64) -> Job {
    Job {
        name: "nightly".to_string(),
        schedule: schedule.to_string(),
        tz: Some(CHICAGO.to_string()),
        task: Task::Prompt("hi".to_string()),
        endpoint: None,
        model: None,
        dir: PathBuf::from("/work"),
        max_secs: None,
        max_tokens: None,
        worktree: false,
        chat: false,
        notify: Notify::default(),
        mail: None,
        paused: false,
        fails: 0,
        created,
        runs: Vec::new(),
    }
}

fn secs(text: &str) -> i64 {
    at(text).timestamp().as_second()
}

#[test]
fn a_cron_run_cannot_add_jobs() {
    assert!(refuse(Some(OsStr::new("nightly"))).is_err());
    refuse(None).unwrap();
}

#[test]
fn schedules_fire_at_the_next_local_slot() {
    for (expr, after, want) in [
        (
            "*/15 9-17 * * mon-fri",
            "2026-10-12T08:50",
            "2026-10-12T09:00:00-05:00",
        ),
        (
            "*/15 9-17 * * mon-fri",
            "2026-10-16T17:45",
            "2026-10-19T09:00:00-05:00",
        ),
        (
            "0 12 * * 7",
            "2026-10-17T13:00",
            "2026-10-18T12:00:00-05:00",
        ),
        (
            "0 0 1 jan *",
            "2026-10-17T13:00",
            "2027-01-01T00:00:00-06:00",
        ),
        ("@daily", "2026-10-17T13:00", "2026-10-18T00:00:00-05:00"),
        (
            "0 0 13 * fri",
            "2026-10-10T12:00",
            "2026-10-13T00:00:00-05:00",
        ),
        (
            "0 0 13 * fri",
            "2026-10-13T12:00",
            "2026-10-16T00:00:00-05:00",
        ),
        (
            "30 2 * * *",
            "2026-03-07T03:00",
            "2026-03-09T02:30:00-05:00",
        ),
        (
            "30 1 * * *",
            "2026-11-01T00:00",
            "2026-11-01T01:30:00-05:00",
        ),
        ("every 1d", "2026-10-11T15:00", "2026-10-12T00:00:00-05:00"),
        ("every 30m", "2026-10-11T15:10", "2026-10-11T15:30:00-05:00"),
        ("every 1d", "2026-03-08T00:00", "2026-03-09T00:00:00-05:00"),
        ("every 1d", "2026-11-01T00:00", "2026-11-02T00:00:00-06:00"),
    ] {
        assert_eq!(next(expr, at(after)), want, "{expr} after {after}");
    }
}

#[test]
fn the_repeated_hour_fires_once_and_still_fires_for_a_job_made_inside_it() {
    let zoned = |text: &str| format!("{text}[America/Chicago]").parse::<Zoned>().unwrap();
    assert_eq!(
        next("30 1 * * *", zoned("2026-11-01T01:30:00-05:00")),
        "2026-11-02T01:30:00-06:00"
    );
    assert_eq!(
        next("30 1 * * *", zoned("2026-11-01T01:10:00-06:00")),
        "2026-11-01T01:30:00-06:00"
    );
}

#[test]
fn bad_schedules_are_refused() {
    for text in [
        "61 * * * *",
        "* * * *",
        "* * * * mon-xyz",
        "5-1 * * * *",
        "every 0m",
        "every 3w",
    ] {
        assert!(parse(text).is_err(), "{text}");
    }
}

#[test]
fn missed_slots_run_once_at_the_latest_and_old_slots_expire() {
    let hourly = job("0 * * * *", secs("2026-10-11T09:10"));
    assert_eq!(
        due(&hourly, None, secs("2026-10-11T12:05")).unwrap(),
        Due::Run(secs("2026-10-11T12:00"))
    );
    let ran = Some(secs("2026-10-11T12:00"));
    assert_eq!(
        due(&hourly, ran, secs("2026-10-11T12:59")).unwrap(),
        Due::No
    );
    let once = job("at 2026-09-01T12:00:00Z", secs("2026-08-30T12:00"));
    assert_eq!(
        due(&once, None, secs("2026-10-11T12:00")).unwrap(),
        Due::Expired(1_788_264_000)
    );
}

#[test]
fn exit_codes_map_to_outcomes() {
    for (code, outcome) in [
        (Some(0), Outcome::Done),
        (Some(2), Outcome::CheckFailed),
        (Some(3), Outcome::Unfinished),
        (Some(1), Outcome::Error),
        (None, Outcome::Error),
    ] {
        assert_eq!(Outcome::from_code(code), outcome);
    }
    assert!(!Outcome::Done.failed() && Outcome::Unfinished.failed());
}

#[test]
fn jobs_round_trip_through_toml() {
    let mut nightly = job("0 3 * * *", 1);
    nightly.task = Task::Goal("make cargo test pass".to_string());
    nightly.runs.push(super::store::Run {
        slot: 2,
        started: 3,
        job: Some("j1".to_string()),
        outcome: Outcome::CheckFailed,
        origin: super::store::Origin::Webhook,
    });
    let text = toml::to_string(&nightly).unwrap();
    assert_eq!(toml::from_str::<Job>(&text).unwrap(), nightly);
}

#[test]
fn the_launch_agent_escapes_paths() {
    let text = plist(
        Path::new("/Apps/A&B/summonr"),
        60,
        Path::new("/h/.summonr/cron/tick.log"),
        TickEnv {
            path: "/usr/bin:/bin",
            home: Some("/h"),
            summonr_home: Some("/h/.summonr"),
        },
    );
    assert!(text.contains("<string>/Apps/A&amp;B/summonr</string>\n    <string>cron</string>\n    <string>tick</string>"));
    assert!(text.contains("<integer>60</integer>"));
    assert!(text.contains("<key>SUMMONR_HOME</key>"));
    assert!(
        text.contains("<key>HOME</key>\n    <string>/h</string>"),
        "{text}"
    );
    assert_eq!(
        domains(501),
        ["gui/501".to_string(), "user/501".to_string()]
    );
}

#[test]
fn the_crontab_line_is_quoted_escaped_and_replaceable() {
    let line = cron_line(Path::new("/opt/it's/summonr"), bare("/usr/bin"));
    assert_eq!(
        line,
        "* * * * * PATH='/usr/bin' '/opt/it'\\''s/summonr' cron tick # summonr-cron"
    );
    let table = format!("0 1 * * * backup\n{line}\n");
    assert_eq!(crontab_without(&table), "0 1 * * * backup\n");
    let line = cron_line(Path::new("/opt/100%/summonr"), bare("/usr/bin"));
    assert!(line.contains("'/opt/100\\%/summonr'"), "{line}");
    let env = TickEnv {
        home: Some("/Users/frankie"),
        ..bare("/usr/bin")
    };
    let line = cron_line(Path::new("/bin/summonr"), env);
    assert!(line.contains("HOME='/Users/frankie'"), "{line}");
}

#[test]
fn the_systemd_timer_fires_each_minute() {
    let (service, timer) = units(Path::new("/bin/summonr"), 60, bare("/usr/bin"));
    assert!(service.contains("ExecStart=\"/bin/summonr\" cron tick"));
    assert!(service.contains("Environment=\"PATH=/usr/bin\""));
    assert!(timer.contains("OnCalendar=*-*-* *:*:00"));
    assert!(service.contains("KillMode=process"), "{service}");
}

#[test]
fn status_without_the_tick_says_nothing_fires() {
    let next = at("2026-12-01T09:00:00");
    let off = super::view::status(None, 1, Some(next.clone()));
    assert!(
        off.contains("next: nothing fires until you run `summonr cron install`; would fire at 2026-12-01 09:00"),
        "{off}"
    );
    let on = super::view::status(Some(Path::new("/tick")), 1, Some(next));
    assert!(on.ends_with("next: 2026-12-01 09:00 CST"), "{on}");
}

#[test]
fn a_quiet_tick_leaves_the_job_book_alone() {
    super::store::update(|jobs| {
        jobs.retain(|job| job.name != "quiet-probe");
        let mut probe = job("0 3 * * *", 1);
        probe.name = "quiet-probe".to_string();
        jobs.push(probe);
        Ok(())
    })
    .unwrap();
    let book = super::store::dir().unwrap().join("jobs.toml");
    let before = std::fs::metadata(&book).unwrap().modified().unwrap();
    super::store::update(|_| Ok(())).unwrap();
    let after = std::fs::metadata(&book).unwrap().modified().unwrap();
    assert_eq!(
        before, after,
        "an update that changed nothing rewrote the job book"
    );
}
