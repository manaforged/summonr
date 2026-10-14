use super::{Sample, Shape, fbm, gauss, noise, ridged};

const DISK: [f32; 2] = [0.5, 1.25];

const RADIUS: [[f32; 2]; 2] = [[1.25, 0.45], [0.36, 0.14]];

const PITCH: [f32; 2] = [0.3, 0.2];

const TILT: [f32; 2] = [0.3, 0.4];

const BAR: [f32; 3] = [0.18, 0.27, 0.075];

const CORE: f32 = 0.09;

const SWIRL: f32 = 2.2;

const REDDEN: [f32; 2] = [0.5, 0.2];

pub(super) struct Disk {
    arms: f32,
    pitch: f32,
    tilt: f32,
    center: (f32, f32),
    radius: f32,
    bar: f32,
    spin: f32,
    lane: f32,
}

impl Disk {
    pub(super) fn of(roll: &dyn Fn(u16) -> f32, distant: bool) -> Self {
        let side = if roll(11) > 0.5 { 1.0 } else { -1.0 };
        let center = if distant {
            let below = if roll(20) > 0.5 { 1.0 } else { -1.0 };
            (
                -side * (0.9 + 0.4 * roll(12)),
                below * (0.55 + 0.2 * roll(13)),
            )
        } else {
            (side * (0.45 + 0.55 * roll(12)), (roll(13) - 0.5) * 0.5)
        };
        let [base, spread] = RADIUS[usize::from(distant)];
        Self {
            arms: if roll(8) > 0.6 { 3.0 } else { 2.0 },
            pitch: PITCH[0] + PITCH[1] * roll(9),
            tilt: TILT[0] + TILT[1] * roll(10),
            center,
            radius: base + spread * roll(14),
            bar: BAR[0] + BAR[1] * roll(15),
            spin: if roll(16) > 0.5 { 1.0 } else { -1.0 },
            lane: if roll(17) > 0.5 { 0.03 } else { -0.03 },
        }
    }
}

pub(super) fn sample(px: f32, py: f32, shape: &Shape, disk: &Disk) -> Sample {
    let (dx, dy) = (px - disk.center.0, py - disk.center.1);
    let xr = dx * shape.cos + dy * shape.sin;
    let yr = (dy * shape.cos - dx * shape.sin) / disk.tilt;
    let r = xr.hypot(yr) / disk.radius;
    let wind = (r + 0.04).ln() / disk.pitch.tan();
    let (sw, cw) = wind.sin_cos();
    let scale = SWIRL / disk.radius;
    let ux = (xr * cw + yr * sw) * scale + shape.offset;
    let uy = (yr * cw - xr * sw) * scale;
    let wu = fbm(ux * 0.8 + 3.0, uy * 0.8 + 7.0, 3) - 0.5;
    let wv = fbm(ux * 0.8 - 5.0, uy * 0.8 + 1.0, 3) - 0.5;
    let (wx, wy) = (ux + 0.9 * wu, uy + 0.9 * wv);
    let phase = (yr.atan2(xr) * disk.spin - wind) * disk.arms + 2.2 * wu;
    let body = (-r / DISK[0]).exp() * ((DISK[1] - r) * 2.5).clamp(0.0, 1.0);
    let arm = (0.5 + 0.5 * phase.cos()).powf(2.2);
    let clouds = ((fbm(wx * 1.7, wy * 2.4, 5) - 0.3) * 1.8).clamp(0.0, 1.0);
    let grain = 0.88 + 0.24 * noise(wx * 9.0 + 5.0, wy * 7.0);
    let stars = body * (0.2 + 0.95 * arm * (0.35 + 0.75 * clouds)) * grain * 2.1;
    let (bx, by) = (xr / (disk.radius * disk.bar), yr / (disk.radius * BAR[2]));
    let core = gauss(r / CORE).max((-(bx * bx + by * by)).exp() * 0.7);
    let edge = 0.6 + 0.8 * fbm(wx * 2.0 + 40.0, wy * 2.0, 3);
    let bulge = core.powf(1.0 / edge).min(1.0);
    let broken = ((fbm(wx * 1.4 + 11.0, wy * 2.2, 5) - 0.3) * 2.4).clamp(0.0, 1.0);
    let trail = (0.5 + 0.5 * (phase - 0.9).cos()).powi(6) * broken * (r * 3.0).min(1.0);
    let ridge = ((ridged(wx * 2.0 + 30.0, wy * 2.0 - 4.0) - 0.45) * 1.6).clamp(0.0, 1.0);
    let lane = gauss((yr * disk.tilt + disk.lane) / (0.035 + 0.02 * (1.0 - disk.tilt)))
        * ((0.62 - disk.tilt) * 3.0).clamp(0.0, 1.0)
        * (1.2 - r).clamp(0.0, 1.0);
    let dust = (trail * 0.8).max(ridge * body).max(lane).min(1.0);
    let light = (stars + bulge * 0.9).min(1.0) * 1.15;
    let nebula = ((fbm(wx * 2.6 + 50.0, wy * 3.4, 4) - 0.5) * 4.0).clamp(0.0, 1.0);
    Sample {
        shade: light * (1.0 - 0.8 * dust),
        glow: bulge * (1.0 - 0.6 * dust),
        nebula: nebula * arm * (body * 3.0).min(1.0) * (0.6 + dust),
        hue: fbm(wx * 0.7 + 90.0, wy * 0.5, 3) - REDDEN[0] * (r - REDDEN[1]),
    }
}
