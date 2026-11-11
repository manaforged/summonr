pub(super) const NONE_U16: u16 = u16::MAX;
pub(super) const NONE_U32: u32 = u32::MAX;
pub(super) const NONE_U64: u64 = u64::MAX;

pub(super) const REASONING: u8 = 1;
pub(super) const VISION: u8 = 1 << 1;
pub(super) const DEPRECATED: u8 = 1 << 2;
pub(super) const METERED_SHIFT: u8 = 3;
pub(super) const TOOLS_SHIFT: u8 = 5;

pub(super) const TRI_MASK: u8 = 0b11;
pub(super) static TRI: [Option<bool>; 3] = [None, Some(false), Some(true)];
