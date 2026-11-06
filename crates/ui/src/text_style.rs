use sigil::prelude::{FontFamily, Paint, Token};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TextStyle {
    size: f32,
    color: Token,
    mono: bool,
    italic: bool,
    bold: bool,
}

impl TextStyle {
    pub(crate) const fn xs(color: Token) -> Self {
        Self {
            size: 12.0,
            color,
            mono: false,
            italic: false,
            bold: false,
        }
    }

    pub(crate) const fn sm(color: Token) -> Self {
        Self {
            size: 14.0,
            ..Self::xs(color)
        }
    }

    pub(crate) const fn mono(self) -> Self {
        Self { mono: true, ..self }
    }

    pub(crate) const fn italic(self) -> Self {
        Self {
            italic: true,
            ..self
        }
    }

    pub(crate) const fn bold(self) -> Self {
        Self { bold: true, ..self }
    }

    pub(crate) fn on<P: Paint>(self, node: P) -> P {
        let node = node.font_size(self.size).text_color(self.color);
        let node = if self.mono {
            node.font_family(FontFamily::Mono)
        } else {
            node
        };
        let node = if self.bold {
            node.font_weight(700)
        } else {
            node
        };
        if self.italic {
            node.font_italic(true)
        } else {
            node
        }
    }
}
