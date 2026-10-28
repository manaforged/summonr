use crate::error::{Error, Result};
use crate::tools::ToolRegistry;

impl ToolRegistry {
    pub(in crate::tools) fn set_browser(&mut self, on: bool) -> Result<()> {
        self.browser = if on {
            Some(match self.browser.take() {
                Some(held) => held,
                None => crate::browser::Browser::discover()?,
            })
        } else {
            None
        };
        Ok(())
    }

    pub(in crate::tools) async fn browse(&self, arguments: &str) -> Result<String> {
        let session = self.session.lock().ok().and_then(|held| held.clone());
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| Error::Tool("the browser module is off".to_string()))?;
        let resolve = |path: &str| self.resolve_read(path, crate::tools::paths::Shield::Contained);
        browser.call(arguments, &resolve, session.as_deref()).await
    }
}
