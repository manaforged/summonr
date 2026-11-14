#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Lid {
    Open,
    Half,
    Shut,
}

pub(super) struct Tongue {
    pub(super) rows: &'static [[i32; 2]],
    pub(super) amp: f32,
    pub(super) lag: f32,
    pub(super) stretch: f32,
    pub(super) phase: f32,
    pub(super) min: usize,
}

pub(super) struct Bubble {
    pub(super) at: u64,
    pub(super) life: u64,
    pub(super) from: [f32; 2],
    pub(super) to: [f32; 2],
    pub(super) radius: [f32; 2],
}

pub(super) struct Ember {
    pub(super) at: u64,
    pub(super) life: u64,
    pub(super) rise: f32,
    pub(super) drift: f32,
    pub(super) tongue: usize,
}

pub(super) struct Shape {
    pub(super) grid: &'static [&'static str],
    pub(super) eyes: [i32; 3],
    pub(super) mirror: i32,
    pub(super) head: usize,
    pub(super) foot: usize,
    pub(super) body: usize,
    pub(super) rings: [u32; 2],
    pub(super) tongues: &'static [Tongue],
    pub(super) bubbles: &'static [Bubble],
    pub(super) embers: &'static [Ember],
}

pub(super) const BANNER: Shape = Shape {
    grid: &[
        "......e.........",
        ".....ee.........",
        ".....eme........",
        ".....emme.......",
        "..e..emmme......",
        "..ee.emcmme.....",
        "..ee.emccme.....",
        "..eemmccccmmee..",
        ".eemmccccccmmee.",
        ".emmccccccccmme.",
        ".emmccccccccmme.",
        ".emmccccccccmme.",
        ".emmccccccccmme.",
        "..emmccccccmme..",
        "...emmccccmme...",
        ".....eeeeee.....",
    ],
    eyes: [5, 10, 3],
    mirror: 15,
    head: 5,
    foot: 1,
    body: 7,
    rings: [1, 3],
    tongues: &[
        Tongue {
            rows: &[[5, 10], [5, 10], [6, 10], [6, 9], [6, 8], [6, 7], [7, 7]],
            amp: 2.4,
            lag: 0.75,
            stretch: 0.28,
            phase: 0.0,
            min: 1,
        },
        Tongue {
            rows: &[[2, 3], [2, 3], [2, 2]],
            amp: 1.2,
            lag: 0.9,
            stretch: 0.45,
            phase: -1.6,
            min: 1,
        },
    ],
    bubbles: &[
        Bubble {
            at: 0,
            life: 14,
            from: [12.5, 7.5],
            to: [10.5, 4.0],
            radius: [1.4, 0.7],
        },
        Bubble {
            at: 12,
            life: 14,
            from: [11.5, 8.0],
            to: [9.5, 3.5],
            radius: [1.2, 0.6],
        },
        Bubble {
            at: 6,
            life: 12,
            from: [3.5, 7.5],
            to: [2.5, 2.5],
            radius: [1.1, 0.6],
        },
    ],
    embers: &[
        Ember {
            at: 4,
            life: 14,
            rise: 5.0,
            drift: 1.5,
            tongue: 0,
        },
        Ember {
            at: 16,
            life: 12,
            rise: 4.0,
            drift: -1.0,
            tongue: 0,
        },
        Ember {
            at: 10,
            life: 12,
            rise: 4.0,
            drift: 0.5,
            tongue: 0,
        },
        Ember {
            at: 21,
            life: 10,
            rise: 3.0,
            drift: 2.0,
            tongue: 0,
        },
        Ember {
            at: 1,
            life: 10,
            rise: 3.0,
            drift: -1.0,
            tongue: 1,
        },
        Ember {
            at: 13,
            life: 10,
            rise: 3.0,
            drift: -0.5,
            tongue: 1,
        },
    ],
};

pub(super) const COMPACT: Shape = Shape {
    grid: &[
        "...e.....",
        "...ee....",
        "...eme...",
        ".e.emme..",
        ".eemmmee.",
        "emcccccme",
        "emcccccme",
        "emcccccme",
        ".emcccme.",
        "..eeeee..",
    ],
    eyes: [2, 5, 2],
    mirror: 8,
    head: 3,
    foot: 1,
    body: 4,
    rings: [1, 2],
    tongues: &[
        Tongue {
            rows: &[[3, 6], [3, 6], [4, 5], [4, 4]],
            amp: 1.6,
            lag: 0.8,
            stretch: 0.3,
            phase: 0.0,
            min: 4,
        },
        Tongue {
            rows: &[[1, 1]],
            amp: 0.7,
            lag: 0.0,
            stretch: 1.0,
            phase: -1.6,
            min: 1,
        },
    ],
    bubbles: &[Bubble {
        at: 0,
        life: 12,
        from: [6.5, 4.5],
        to: [6.0, 1.0],
        radius: [0.9, 0.5],
    }],
    embers: &[
        Ember {
            at: 6,
            life: 12,
            rise: 3.0,
            drift: 1.0,
            tongue: 0,
        },
        Ember {
            at: 18,
            life: 10,
            rise: 2.0,
            drift: -1.0,
            tongue: 0,
        },
        Ember {
            at: 12,
            life: 8,
            rise: 2.0,
            drift: 0.0,
            tongue: 1,
        },
    ],
};
