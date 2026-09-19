use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt,
    NodeStyleExt, Role, classes, container, signal,
};

pub(in crate::tui::sigil_view) fn hit(
    name: String,
    on: bool,
    act: impl Fn() + 'static,
) -> NodeBuilder {
    container()
        .role(Role::Button)
        .label(name)
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(28.0)
                .padding_x(8.0)
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(on), "bg-gray-5")
        .pointer_down(move |event| {
            act();
            event.stop_propagation();
        })
}

pub(in crate::tui::sigil_view) fn hit_menu(
    name: String,
    on: bool,
    act: impl Fn() + 'static,
    menu: impl Fn() + 'static,
) -> NodeBuilder {
    container()
        .role(Role::Button)
        .label(name)
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(28.0)
                .padding_x(8.0)
                .gap(8.0)
                .align_items(AlignItems::Center),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(on), "bg-gray-5")
        .pointer_down(move |event| {
            if event.button == Some(MouseButton::Right) {
                menu();
            } else {
                act();
            }
            event.stop_propagation();
        })
}

pub(in crate::tui::sigil_view) fn spacer() -> NodeBuilder {
    container().layout(LayoutStyle::block().grow(1.0))
}

#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) fn hairline() -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fixed_size(1.0, 18.0))
        .class(classes!("bg-gray-6"))
}
