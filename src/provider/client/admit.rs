use std::borrow::Cow;

use super::*;
use crate::provider::wire::ImageBudget;

impl Client {
    pub(super) fn admit<'m>(&self, messages: &'m [Message]) -> Cow<'m, [Message]> {
        admit(
            messages,
            self.images,
            &crate::provider::wire::image_budget(self.wire),
        )
    }
}

fn admit<'m>(messages: &'m [Message], seen: bool, budget: &ImageBudget) -> Cow<'m, [Message]> {
    let Some(verdicts) = verdicts(messages, seen, budget) else {
        return Cow::Borrowed(messages);
    };
    let mut list = messages.to_vec();
    for (message, reasons) in list.iter_mut().zip(verdicts) {
        if reasons.iter().all(Option::is_none) {
            continue;
        }
        let mut kept = Vec::with_capacity(message.images.len());
        for (image, reason) in std::mem::take(&mut message.images).into_iter().zip(reasons) {
            let Some(reason) = reason else {
                kept.push(image);
                continue;
            };
            if !message.content.is_empty() {
                message.content.push_str("\n\n");
            }
            message
                .content
                .push_str(&format!("[image omitted: {reason}]"));
        }
        message.images = kept;
    }
    Cow::Owned(list)
}

fn verdicts(
    messages: &[Message],
    seen: bool,
    budget: &ImageBudget,
) -> Option<Vec<Vec<Option<String>>>> {
    let mut verdicts: Vec<Vec<Option<String>>> = messages
        .iter()
        .map(|message| vec![None; message.images.len()])
        .collect();
    let mut any = false;
    let mut count = 0;
    let mut bytes = 0;
    for (message, reasons) in messages.iter().zip(verdicts.iter_mut()).rev() {
        for (image, reason) in message.images.iter().zip(reasons.iter_mut()).rev() {
            let size = image.data.len();
            // complexity: bounded by the five media types a wire names
            let typed = budget.types.contains(&image.media_type.as_str());
            *reason = if !seen {
                Some("the active model does not accept image input".to_string())
            } else if !typed {
                Some(format!(
                    "this endpoint does not accept {}",
                    image.media_type
                ))
            } else if count >= budget.count || bytes + size > budget.bytes {
                Some("older than the images this request can carry".to_string())
            } else {
                count += 1;
                bytes += size;
                None
            };
            any |= reason.is_some();
        }
    }
    any.then_some(verdicts)
}
