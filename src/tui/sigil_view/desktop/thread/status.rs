use super::*;
use sigil::prelude::{Radius, progress, signal};

use crate::tui::sigil_view::speed::Speed;

pub(in crate::tui::sigil_view) const CONTEXT_USED: &str = "Context used";
const METER: f32 = 64.0;

#[expect(clippy::cast_precision_loss, reason = "a percentage fits in f32")]
pub(in crate::tui::sigil_view) fn meters(app: App) -> NodeBuilder {
    let (used, cached, spend, rate) = {
        let model = app.model.get();
        (
            model.context_percent(),
            model.cache_percent(),
            model.spend(),
            model.speed.live(std::time::Instant::now()),
        )
    };
    let mut facts = container().layout(
        LayoutStyle::flex_row()
            .gap(L)
            .min_size(0.0, 0.0)
            .align_items(AlignItems::Center),
    );
    if let Some(used) = used {
        facts = facts.child(
            container()
                .layout(
                    LayoutStyle::flex_row()
                        .gap(S)
                        .align_items(AlignItems::Center),
                )
                .child(quiet("context"))
                .child(
                    container()
                        .label(CONTEXT_USED)
                        .layout(LayoutStyle::block().width(METER).height(XS).shrink(0.0))
                        .rounded(Radius::Full)
                        .child(progress(signal(used as f32), 100.0).height(XS)),
                )
                .child(quiet(format!("{used}%"))),
        );
    }
    if cached > 0 {
        facts = facts.child(quiet(format!("cache {cached}%")));
    }
    if let Some(spend) = spend {
        facts = facts.child(quiet(crate::provider::dollars(spend)));
    }
    if let Some(rate) = rate {
        facts = facts.child(quiet(Speed::label(rate)));
    }
    let mut row = container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(XL + XS)
                .gap(L)
                .align_items(AlignItems::Center),
        )
        .child(facts)
        .child(spacer());
    if let Some(note) = app.notice.get() {
        let tone = match note.kind {
            crate::tui::sigil_view::app::NoticeKind::Copied => Token::Success11,
            crate::tui::sigil_view::app::NoticeKind::Failed => Token::Error11,
        };
        row = row.child(quiet(note.text).text_color(tone));
    }
    row
}
