use super::*;

fn spend(model: &str, workspace: &str, hour: u64, input: u64, cached: u64) -> Spend {
    Spend {
        model: model.to_string(),
        hour,
        workspace: workspace.to_string(),
        input,
        cached,
        output: 10,
        turns: 2,
        millis: 3000,
        sessions: 1,
        ..Spend::default()
    }
}

#[test]
fn a_report_ranks_models_and_workspaces_and_places_each_hour() {
    let now = 40 * DAY;
    let rows = [
        spend("small", "/w/alpha", now - 2 * DAY + 5 * HOUR, 90, 45),
        spend("big", "/w/beta", now - 3 * HOUR, 990, 900),
        spend("big", "/w/alpha", now - 9 * DAY, 490, 400),
        spend("old", "/w/gamma", now - 31 * DAY, 5000, 0),
    ];
    let refs: Vec<&Spend> = rows.iter().collect();
    let report = Report::build(&refs, 2, Metric::Tokens, now);
    assert_eq!(report.models[0].0, "big");
    assert_eq!(report.models[0].1.input, 1480);
    assert_eq!(report.models.len(), 2);
    assert_eq!(report.places[0], ("beta".to_string(), 1000.0));
    assert_eq!(report.places[1], ("alpha".to_string(), 600.0));
    assert_eq!(report.total.sessions, 3);
    assert_eq!(report.columns.len(), 30);
    assert_eq!(report.columns.iter().sum::<f64>(), 1600.0);
    assert_eq!(report.hours[5], 100.0);
    assert_eq!(report.hours[21], 1000.0);
    assert!((report.cache_hit() - 1345.0 * 100.0 / 1570.0).abs() < 1e-9);
    assert!((turn_secs(&report.total) - 1.5).abs() < 1e-9);
}

#[test]
fn each_model_keeps_the_day_it_ran() {
    let now = 40 * DAY;
    let rows = [
        spend("alpha", "/w/one", now - 2 * DAY, 100, 0),
        spend("beta", "/w/two", now - 2 * DAY, 40, 0),
        spend("alpha", "/w/one", now - 5 * DAY, 10, 0),
    ];
    let refs: Vec<&Spend> = rows.iter().collect();
    let report = Report::build(&refs, 2, Metric::Tokens, now);
    assert_eq!(report.models[0].0, "alpha");
    assert_eq!(report.models[1].0, "beta");
    assert_eq!(report.series.len(), 2);
    assert_eq!(report.series[0][28], 110.0);
    assert_eq!(report.series[0][25], 20.0);
    assert_eq!(report.series[1][28], 50.0);
    assert_eq!(
        report.series[1]
            .iter()
            .filter(|value| **value > 0.0)
            .count(),
        1
    );
    assert_eq!(report.columns[28], 160.0);
    assert_eq!(report.columns[25], 20.0);
}
