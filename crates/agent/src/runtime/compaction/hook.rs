use serde_json::json;

use super::*;
use crate::hooks::{Compaction, HookEvent, Start};
use crate::runtime::hooks::context_note;

impl<P: Provider> Loop<P> {
    pub(super) async fn pre_compact(
        &mut self,
        trigger: Compaction,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<std::result::Result<String, String>> {
        let payload = json!({ "trigger": trigger.name(), "custom_instructions": null });
        let heard = self
            .hook(HookEvent::PreCompact, Some(trigger.name()), payload)
            .await;
        self.hook_notes(HookEvent::PreCompact, &heard.notes, events, emit)?;
        Ok(match heard.blocked {
            Some(reason) => Err(reason),
            None => Ok(heard.context.join("\n\n")),
        })
    }

    pub(super) async fn restarted(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<String>> {
        if self.session.depth() > 0 {
            return Ok(None);
        }
        let heard = self.session_start(Start::Compact).await;
        self.hook_notes(HookEvent::SessionStart, &heard.notes, events, emit)?;
        Ok(context_note(&heard.context))
    }

    pub(super) async fn compacted(
        &mut self,
        summary: &str,
        trigger: Compaction,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let payload = json!({ "summary": summary, "trigger": trigger.name() });
        let heard = self
            .hook(HookEvent::Compact, Some(trigger.name()), payload)
            .await;
        self.hook_notes(HookEvent::Compact, heard.note().iter(), events, emit)
    }
}
