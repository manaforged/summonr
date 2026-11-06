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
                crate::display::ellipsize_start(output.trim(), OUTPUT_TAIL)
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
                crate::display::ellipsize_start(output.trim(), OUTPUT_TAIL)
            ),
        };
        let best = if kept {
            text.push_str(&self.tell_lanes(reading.as_ref().map(|(_, value)| *value)));
            reading.as_ref().map(|(_, value)| *value)
        } else {
            let (_, files) = self.rollback(None)?;
            text.push_str(&format!(
                "; summonr restored {} to the start of this turn",
                summonr_core::text::plural(files.len(), "file")
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

    fn tell_lanes(&self, value: Option<f64>) -> String {
        let Some((metric, _)) = self.goal_metric() else {
            return String::new();
        };
        if metric.lanes.is_empty() {
            return String::new();
        }
        let Some(value) = value else {
            return "; lanes were not told".to_string();
        };
        let patch = crate::diff::turn_patch(
            &self.workspace,
            &self.session_id(),
            crate::diff::TurnId::Last,
            None,
        )
        .unwrap_or_default();
        let finding = finding(value, &patch, &self.port_text());
        let Ok(dir) = crate::mail::dir() else {
            return "; lanes were not told".to_string();
        };
        let agents = crate::mail::live(&dir);
        let mut by_name = std::collections::HashMap::new();
        for agent in &agents {
            if !agent.name.is_empty() {
                by_name.insert(agent.name.as_str(), agent);
            }
            if !agent.id.is_empty() {
                by_name.insert(agent.id.as_str(), agent);
            }
        }
        let mut notes = Vec::new();
        for lane in &metric.lanes {
            let Some(agent) = by_name.get(lane.as_str()) else {
                notes.push(format!("{lane}: not on the roster"));
                continue;
            };
            if agent.id == self.session_id() {
                continue;
            }
            match crate::mail::Mailbox::wake(&agent.id, &finding) {
                Ok(()) => notes.push(format!("told {lane}")),
                Err(error) => notes.push(format!("{lane}: {error}")),
            }
        }
        if notes.is_empty() {
            String::new()
        } else {
            format!("; {}", notes.join("; "))
        }
    }

    fn port_text(&self) -> String {
        self.session
            .history()
            .iter()
            .rev()
            .find(|message| {
                message.role == crate::provider::Role::Assistant
                    && !message.content.trim().is_empty()
            })
            .map(|message| message.content.trim().to_string())
            .unwrap_or_default()
    }
}

fn finding(value: f64, patch: &str, port: &str) -> String {
    let mut text = format!("number: {value}\n");
    if !port.is_empty() {
        text.push_str(port);
        text.push('\n');
    }
    text.push_str(patch);
    text
}

#[cfg(test)]
#[path = "metric_tests.rs"]
mod tests;
