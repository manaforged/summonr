use super::*;
use crate::verify::Metric;

const OUTPUT_TAIL: usize = 400;

impl<P: Provider> Loop<P> {
    pub(super) fn goal_metric(&self) -> Option<(Metric, crate::verify::Protected)> {
        self.metric.clone().filter(|_| self.is_goal())
    }

    async fn read_metric(&self, metric: &Metric) -> (String, Option<(String, f64)>) {
        let result = self
            .tools
            .run_check(&metric.command, metric.timeout_secs)
            .await;
        let ran = verify::outcome(&metric.command, &result);
        let reading = if ran.passed {
            metric.read(&ran.output)
        } else {
            None
        };
        (ran.output, reading)
    }

    pub(super) async fn measure_baseline(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let Some((metric, _)) = self.goal_metric() else {
            return Ok(());
        };
        if self.session.measured()? {
            return Ok(());
        }
        let (output, reading) = self.read_metric(&metric).await;
        let text = match &reading {
            Some((value, _)) => format!("metric baseline {value}"),
            None => format!(
                "no metric baseline: {}",
                crate::text::ellipsize_start(output.trim(), OUTPUT_TAIL)
            ),
        };
        let kept = reading.is_some();
        self.session
            .record_measurement(reading.map(|(value, _)| value), kept)?;
        self.push_event(events, emit, Event::Note { text })
    }

    pub(super) async fn measure_turn(
        &mut self,
        checks: &[Outcome],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<Outcome>> {
        let Some((metric, protected)) = self.goal_metric() else {
            return Ok(None);
        };
        let best = self.session.best_measurement()?;
        let (output, reading) = if let Some(path) = protected.changed(&self.workspace) {
            (
                format!("the turn changed the protected file `{path}`, so the metric did not run"),
                None,
            )
        } else if checks.iter().all(|check| check.passed) {
            self.read_metric(&metric).await
        } else {
            (
                "a check failed, so the metric did not run".to_string(),
                None,
            )
        };
        let kept = reading
            .as_ref()
            .is_some_and(|(_, value)| metric.beats(*value, best));
        let mut text = match (&reading, kept) {
            (Some((value, _)), true) => format!("metric {value}: kept, the new best"),
            (Some((value, _)), false) => format!(
                "metric {value}: did not beat the best {} by more than {}",
                best.map_or_else(|| "none".to_string(), |best| best.to_string()),
                metric.noise
            ),
            (None, _) => format!(
                "no metric value: {}",
                crate::text::ellipsize_start(output.trim(), OUTPUT_TAIL)
            ),
        };
        let best = if kept {
            reading.as_ref().map(|(_, value)| *value)
        } else {
            let (_, files) = self.rollback(None)?;
            text.push_str(&format!(
                "; summonr restored {} to the start of this turn",
                crate::text::plural(files.len(), "file")
            ));
            best
        };
        if let Some(target) = metric.target {
            text.push_str(&format!("; target {target}"));
        }
        self.session
            .record_measurement(reading.map(|(value, _)| value), kept)?;
        let passed = metric.reached(best);
        let outcome = Outcome {
            command: metric.command,
            passed,
            output: text,
        };
        self.push_event(
            events,
            emit,
            Event::Verify {
                command: outcome.command.clone(),
                passed: outcome.passed,
            },
        )?;
        Ok(Some(outcome))
    }
}

#[cfg(test)]
#[path = "metric_tests.rs"]
mod tests;
