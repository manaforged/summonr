pub(crate) struct Wall {
    body_max: usize,
    markers: &'static [&'static str],
    statuses: &'static [u16],
}

pub(crate) const WALL: Wall = Wall {
    body_max: 2048,
    markers: &[
        "enable javascript",
        "javascript is required",
        "javascript is disabled",
        "requires javascript",
        "turn on javascript",
        "just a moment",
        "checking your browser",
        "verify you are human",
        "attention required",
    ],
    statuses: &[403, 503],
};

impl Wall {
    pub(crate) fn status(&self, code: u16) -> bool {
        self.statuses.contains(&code)
    }

    pub(crate) fn body(&self, shown: &str, raw: &str) -> bool {
        if shown.chars().count() > self.body_max {
            return false;
        }
        let lower = raw.to_ascii_lowercase();
        // complexity: bounded by the few JavaScript-wall markers in the browser data table
        self.markers.iter().any(|marker| lower.contains(marker))
    }
}
