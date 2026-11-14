use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::mood::{Mood, Shot};
use super::rig::{Face, Pose};
use super::shape::{Frame, Hold, SWEEP, Sig};

/// Spring stiffness and damping for a pose change: about 0.6 s with a little overshoot.
const SPRING: (f32, f32) = (180.0, 14.0);
/// Spring stiffness and damping for the eyes: about 0.3 s with a slight overshoot.
const GLANCE: (f32, f32) = (420.0, 26.0);
/// How fast the glow, embers, ring and dot ease to their targets, per second.
const EASE: f32 = 7.0;
/// How long a blink keeps the eyes shut, and the shortest and longest gaps between blinks.
const BLINK: (Duration, f32, f32) = (Duration::from_millis(70), 2.4, 6.0);
/// How long the old and new eyes cross-fade, in seconds.
const SWAP: f32 = 0.16;
/// The shortest gap between two colour laps.
const COOLDOWN: Duration = Duration::from_secs(30);
/// Seconds per glow pulse and per status-dot ping.
const PULSE: f32 = 3.0;
const PING: f32 = 1.3;
/// How far the whole body bobs, on the 120 grid.
const BOB: f32 = -2.5;
/// Seconds per doze mote.
const MOTE: f32 = 3.0;

static CLOCK: OnceLock<Instant> = OnceLock::new();

/// Will's moving body: each pose value springs toward its target, and the flame flickers.
pub(super) struct Body {
    at: [f32; 8],
    speed: [f32; 8],
    soft: [f32; 4],
    sway: [f32; 3],
    bob: f32,
    clock: f32,
    face: Face,
    was: Face,
    swapped: Option<Instant>,
    mood: Mood,
    sig: Sig,
    shots: Vec<(Shot, Instant)>,
    swept: Option<Instant>,
    last: Option<Instant>,
    blink: Instant,
}

impl Body {
    /// A body at rest in `mood`.
    pub(super) fn new(mood: Mood) -> Self {
        let pose = mood.pose();
        Self {
            at: target(&pose, (0.0, 0.0)),
            speed: [0.0; 8],
            soft: soft(&pose),
            sway: [0.0; 3],
            bob: 0.0,
            clock: 0.0,
            face: pose.face,
            was: pose.face,
            swapped: None,
            mood,
            sig: pose.sig.unwrap_or(Sig::Warn),
            shots: Vec::new(),
            swept: None,
            last: None,
            blink: Instant::now() + Duration::from_secs_f32(BLINK.1),
        }
    }

    /// Moves the body on to now, toward `mood`, with `played` clips and the eyes on `look`.
    pub(super) fn step(
        &mut self,
        mood: Mood,
        moving: bool,
        played: &[(Shot, Instant)],
        look: (f32, f32),
    ) -> Frame {
        let pose = mood.pose();
        let now = Instant::now();
        self.enter(mood, &pose, now, moving);
        if !moving {
            return self.still(&pose);
        }
        let dt = self
            .last
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.1));
        self.last = Some(now);
        self.spring(&target(&pose, look), dt);
        let ease = 1.0 - (-EASE * dt).exp();
        for (at, goal) in self.soft.iter_mut().zip(soft(&pose)) {
            *at += (goal - *at) * ease;
        }
        for (phase, pace) in self.sway.iter_mut().zip(pose.pace) {
            *phase += dt / pace;
        }
        self.bob += dt / pose.bob;
        self.clock += dt;
        if now >= self.blink + BLINK.0 {
            self.blink = now + Duration::from_secs_f32(gap(now));
        }
        let blink = if now >= self.blink { 0.1 } else { 1.0 };
        let mix = self.swapped.map_or(1.0, |at| {
            (now.duration_since(at).as_secs_f32() / SWAP).min(1.0)
        });
        self.shots
            .retain(|(shot, at)| now.duration_since(*at) < shot.length());
        let mut react = Hold::REST;
        let (mut sparks, mut sweep) = (None, None);
        for &(shot, at) in self.shots.iter().chain(played) {
            let time = now.duration_since(at);
            if time >= shot.length() {
                continue;
            }
            let part = time.as_secs_f32() / shot.length().as_secs_f32();
            match shot {
                Shot::Hop => react = keys(&HOP, part, HOP_CURVE),
                Shot::Flinch => react = keys(&FLINCH, part, FLINCH_CURVE),
                Shot::Burst => sparks = Some(curve(SPARK_CURVE, part)),
                Shot::Sweep => sweep = Some(time.as_secs_f32()),
            }
        }
        let [glow, embers, ring, sig] = self.soft;
        let pulse = wave(self.clock / PULSE);
        let [sx, sy, tilt, x, y, fade, gx, gy] = self.at;
        Frame {
            pose: Hold::new(tilt, 0.0, (sx, sy), (x, y)),
            react,
            fade,
            sway: self.sway.map(wave),
            bob: BOB * (1.0 - (self.bob * std::f32::consts::TAU).cos()) / 2.0,
            glow: (glow + (0.68 - glow) * pulse, 1.0 + 0.05 * pulse),
            ring,
            gaze: (gx, gy),
            face: self.face,
            was: self.was,
            mix,
            blink,
            embers: (embers, self.clock),
            sparks,
            mote: pose.mote.then(|| (self.clock / MOTE).fract()),
            sig: (sig > 0.01).then(|| {
                let grow = if self.sig == Sig::Warn {
                    1.0 + 0.35 * wave(2.0 * self.clock / PING)
                } else {
                    1.0
                };
                (self.sig, sig, grow)
            }),
            sweep: sweep.filter(|time| *time < SWEEP.0 + SWEEP.1),
        }
    }

    fn enter(&mut self, mood: Mood, pose: &Pose, now: Instant, moving: bool) {
        if mood == self.mood {
            return;
        }
        self.mood = mood;
        if pose.face != self.face {
            self.was = self.face;
            self.face = pose.face;
            self.swapped = Some(now);
        }
        if let Some(sig) = pose.sig {
            self.sig = sig;
        }
        if !moving {
            return;
        }
        match mood {
            Mood::Passed => {
                self.shots.push((Shot::Burst, now));
                if self
                    .swept
                    .is_none_or(|at| now.duration_since(at) >= COOLDOWN)
                {
                    self.shots.push((Shot::Sweep, now));
                    self.swept = Some(now);
                }
            }
            Mood::Failed => self.shots.push((Shot::Flinch, now)),
            Mood::Idle
            | Mood::Summoned
            | Mood::Notice
            | Mood::Typing
            | Mood::Thinking
            | Mood::Working
            | Mood::Needs
            | Mood::Stopped
            | Mood::Doze => {}
        }
    }

    fn still(&mut self, pose: &Pose) -> Frame {
        self.at = target(pose, (0.0, 0.0));
        self.speed = [0.0; 8];
        self.soft = soft(pose);
        self.last = None;
        let [sx, sy, tilt, x, y, fade, gx, gy] = self.at;
        Frame {
            pose: Hold::new(tilt, 0.0, (sx, sy), (x, y)),
            react: Hold::REST,
            fade,
            sway: [0.5; 3],
            bob: 0.0,
            glow: (pose.glow, 1.0),
            ring: f32::from(u8::from(pose.ring)),
            gaze: (gx, gy),
            face: pose.face,
            was: pose.face,
            mix: 1.0,
            blink: 1.0,
            embers: (0.0, 0.0),
            sparks: None,
            mote: None,
            sig: pose.sig.map(|sig| (sig, 1.0, 1.0)),
            sweep: None,
        }
    }

    fn spring(&mut self, goal: &[f32; 8], dt: f32) {
        const STEP: f32 = 1.0 / 120.0;
        let mut left = dt;
        while left > 0.0 {
            let h = left.min(STEP);
            for (at, goal) in goal.iter().enumerate() {
                let (stiff, damp) = if (6..8).contains(&at) { GLANCE } else { SPRING };
                let pull = stiff * (goal - self.at[at]) - damp * self.speed[at];
                self.speed[at] += pull * h;
                self.at[at] += self.speed[at] * h;
            }
            left -= h;
        }
    }
}

const fn target(pose: &Pose, look: (f32, f32)) -> [f32; 8] {
    let hold = pose.hold;
    [
        hold.scale.0,
        hold.scale.1,
        hold.tilt,
        hold.shift.0,
        hold.shift.1,
        pose.fade,
        pose.gaze.0 + look.0,
        pose.gaze.1 + look.1,
    ]
}

fn soft(pose: &Pose) -> [f32; 4] {
    [
        pose.glow,
        f32::from(u8::from(pose.embers)),
        f32::from(u8::from(pose.ring)),
        f32::from(u8::from(pose.sig.is_some())),
    ]
}

/// Where a sway that turns once per unit of `phase` sits, from 0 to 1, easing at each end.
fn wave(phase: f32) -> f32 {
    (1.0 - (phase * std::f32::consts::PI).cos()) / 2.0
}

type Curve = (f32, f32, f32, f32);

const HOP_CURVE: Curve = (0.3, 1.5, 0.5, 1.0);
const FLINCH_CURVE: Curve = (0.0, 0.0, 0.58, 1.0);
const SPARK_CURVE: Curve = (0.2, 0.8, 0.3, 1.0);

/// Squash, rise 14 with a stretch, land, settle.
const HOP: [(f32, Hold); 5] = [
    (0.0, Hold::REST),
    (0.25, Hold::new(0.0, 0.0, (1.08, 0.88), (0.0, 0.0))),
    (0.55, Hold::new(0.0, 0.0, (0.94, 1.08), (0.0, -14.0))),
    (0.8, Hold::new(0.0, 0.0, (1.05, 0.95), (0.0, 0.0))),
    (1.0, Hold::REST),
];
/// Jerk back and lean away, then return.
const FLINCH: [(f32, Hold); 3] = [
    (0.0, Hold::REST),
    (0.3, Hold::new(-6.0, 0.0, (0.95, 0.95), (-5.0, 0.0))),
    (1.0, Hold::REST),
];

/// The hold `part` of the way through `frames`, easing each step on `ease`.
fn keys(frames: &[(f32, Hold)], part: f32, ease: Curve) -> Hold {
    frames
        .windows(2)
        .find(|pair| part <= pair[1].0)
        .map_or(Hold::REST, |pair| {
            let ((start, from), (end, to)) = (pair[0], pair[1]);
            from.mix(to, curve(ease, (part - start) / (end - start)))
        })
}

/// A CSS cubic-bezier timing curve at `x`.
fn curve((x1, y1, x2, y2): Curve, x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let bezier = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * a * t * u * u + 3.0 * b * t * t * u + t * t * t
    };
    let mut t = x;
    for _ in 0..8 {
        let slope = (bezier(x1, x2, t + 1e-3) - bezier(x1, x2, t - 1e-3)) / 2e-3;
        if slope.abs() < 1e-5 {
            break;
        }
        t = (t - (bezier(x1, x2, t) - x) / slope).clamp(0.0, 1.0);
    }
    bezier(y1, y2, t)
}

/// A gap before the next blink, spread between the shortest and longest from the clock.
fn gap(now: Instant) -> f32 {
    let start = *CLOCK.get_or_init(Instant::now);
    let spread = (now.duration_since(start).subsec_micros() % 1000) as f32 / 1000.0;
    BLINK.1 + (BLINK.2 - BLINK.1) * spread
}
