use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeStyleExt, PathNode, Rgba8, classes, container,
    path,
};

pub(super) fn folder() -> PathNode {
    mark(16, 16)
        .move_to(2.0, 13.0)
        .line_to(2.0, 5.0)
        .line_to(6.0, 5.0)
        .line_to(8.0, 7.0)
        .line_to(14.0, 7.0)
        .line_to(14.0, 13.0)
        .close()
}

pub(super) fn clock() -> PathNode {
    ring(fine(16, 16), 8.0, 8.0, 6.2)
        .move_to(8.0, 4.5)
        .line_to(8.0, 8.0)
        .line_to(11.2, 10.0)
}

pub(super) fn branch() -> PathNode {
    ring(mark(16, 16), 5.0, 4.0, 2.0)
        .move_to(5.0, 6.0)
        .line_to(5.0, 12.0)
        .move_to(5.0, 9.0)
        .cubic_to(5.0, 9.0, 11.0, 9.0, 11.0, 12.0)
}

pub(super) fn diffs() -> PathNode {
    mark(16, 16)
        .move_to(3.0, 4.0)
        .line_to(10.0, 4.0)
        .line_to(10.0, 13.0)
        .line_to(3.0, 13.0)
        .close()
        .move_to(6.0, 3.0)
        .line_to(13.0, 3.0)
        .line_to(13.0, 12.0)
        .line_to(10.0, 12.0)
}

pub(super) fn terminal() -> PathNode {
    mark(16, 16)
        .move_to(3.0, 4.0)
        .line_to(8.0, 8.0)
        .line_to(3.0, 12.0)
        .move_to(8.0, 12.0)
        .line_to(13.0, 12.0)
}

pub(super) fn peers() -> PathNode {
    ring(ring(fine(16, 16), 5.2, 8.0, 3.1), 10.8, 8.0, 3.1)
}

pub(super) fn avatar() -> NodeBuilder {
    container()
        .layout(LayoutStyle::block().fixed_size(18.0, 18.0))
        .class(classes!("rounded-full bg-gray-8"))
}

fn mark(w: u32, h: u32) -> PathNode {
    path(w, h).stroke(ink(), 1.6).fit_size(14.0)
}

fn fine(w: u32, h: u32) -> PathNode {
    path(w, h).stroke(ink(), 1.2).fit_size(14.0)
}

fn ring(node: PathNode, cx: f32, cy: f32, r: f32) -> PathNode {
    let k = r * 0.552_284_8;
    node.move_to(cx + r, cy)
        .cubic_to(cx + r, cy + k, cx + k, cy + r, cx, cy + r)
        .cubic_to(cx - k, cy + r, cx - r, cy + k, cx - r, cy)
        .cubic_to(cx - r, cy - k, cx - k, cy - r, cx, cy - r)
        .cubic_to(cx + k, cy - r, cx + r, cy - k, cx + r, cy)
        .close()
}

const fn ink() -> Rgba8 {
    Rgba8 {
        r: 200,
        g: 200,
        b: 205,
        a: 255,
    }
}
