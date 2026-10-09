use super::Model;

impl Model {
    fn fill(&self) -> (u32, u32, u64) {
        let window = self.snapshot.context_window.unwrap_or(0);
        let used = self
            .usage
            .last()
            .map(|round| round.prompt.saturating_add(round.completion))
            .unwrap_or(0);
        let percent = if window == 0 {
            0
        } else {
            (u64::from(used.min(window)) * 100).div_ceil(u64::from(window))
        };
        (used, window, percent)
    }

    pub(in crate::tui::sigil_view) fn context_percent(&self) -> Option<u64> {
        let (_, window, percent) = self.fill();
        (window > 0).then_some(percent)
    }

    pub(in crate::tui::sigil_view) fn cache_percent(&self) -> u64 {
        self.cache_hit()
    }

    pub(super) fn context_facts(&self) -> Vec<(String, String)> {
        let (used, window, ctx) = self.fill();
        let mut rows = vec![("used".to_string(), format!("{ctx}%"))];
        if window > 0 {
            rows.push((
                "window".to_string(),
                format!(
                    "{} / {}",
                    crate::provider::compact(u64::from(used)),
                    crate::provider::compact(u64::from(window))
                ),
            ));
        }
        let out = self.completion_tokens.saturating_sub(self.reasoning_tokens);
        if self.reasoning_tokens > 0 {
            rows.push((
                "think".to_string(),
                crate::provider::compact(self.reasoning_tokens),
            ));
        }
        if out > 0 {
            rows.push(("out".to_string(), crate::provider::compact(out)));
        }
        rows
    }

    pub(super) fn cache_facts(&self) -> Vec<(String, String)> {
        let mut rows = vec![("hit".to_string(), format!("{}%", self.cache_hit()))];
        if let Some(last) = self.last_hit() {
            rows.push(("last".to_string(), format!("{last}%")));
        }
        if self.cached_tokens > 0 {
            rows.push((
                "cached".to_string(),
                crate::provider::compact(self.cached_tokens),
            ));
        }
        if self.prompt_tokens > 0 {
            rows.push((
                "in".to_string(),
                crate::provider::compact(self.prompt_tokens),
            ));
            let fresh = self.prompt_tokens.saturating_sub(self.cached_tokens);
            rows.push(("fresh".to_string(), crate::provider::compact(fresh)));
        }
        rows
    }

    fn cache_hit(&self) -> u64 {
        if self.prompt_tokens == 0 {
            return 0;
        }
        self.cached_tokens.min(self.prompt_tokens) * 100 / self.prompt_tokens.max(1)
    }

    fn last_hit(&self) -> Option<u64> {
        let round = self.usage.last()?;
        if round.prompt == 0 {
            return None;
        }
        Some(u64::from(round.cached.min(round.prompt)) * 100 / u64::from(round.prompt.max(1)))
    }
}
