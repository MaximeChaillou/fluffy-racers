//! Pure camera maths (no ECS): where the chase camera sits and looks, given the kart's
//! interpolated pose. Unit-tested at several frame rates.

use bevy::prelude::*;

use super::CameraTuning;

/// What the rig needs to know about the kart this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigInput {
    /// Interpolated kart position.
    pub position: Vec3,
    /// Direction the kart faces.
    pub heading: Vec3,
    /// Yaw rate (rad/s, counter-clockwise positive, like Bevy): used to anticipate turns.
    pub yaw_rate: f32,
    /// Speed / max speed (can exceed 1 while boosting).
    pub speed_ratio: f32,
    pub boosting: bool,
    /// -1 to 1: side shift asked by a drift (positive = camera shifts right).
    pub lean: f32,
    /// Issue #78: the kart's up (`kart::KartEnvironment::up`): the world's up, or in anti-gravity
    /// the road's normal under its wheels. The camera turns its own up toward it, smoothly.
    pub up: Vec3,
}

/// Camera placement for this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigPose {
    pub eye: Vec3,
    pub look_at: Vec3,
    pub fov_degrees: f32,
    /// The camera's up (issue #78): the world's up, but in anti-gravity the kart's, smoothed.
    pub up: Vec3,
}

/// Smoothed camera state. Every value follows its target through a critically damped spring:
/// fast, no overshoot, no oscillation, and frame-rate independent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraRig {
    initialized: bool,
    /// Horizontal unit direction from the kart to the camera (perpendicular to `up`).
    behind: Vec3,
    behind_velocity: Vec3,
    /// Smoothed kart height: bumps and jumps are softened. Issue #78: the camera's base is
    /// `height_lag` meters along `up` from the kart (the lag of that spring), its target 0.
    height_lag: f32,
    height_velocity: f32,
    /// Where the kart was on the last frame: how far it rose along `up` since.
    last_position: Vec3,
    /// Look-at point relative to the kart (horizontal position, smoothed height).
    look: Vec3,
    look_velocity: Vec3,
    lean: f32,
    lean_velocity: f32,
    fov: f32,
    /// Issue #78: the camera's up, following the kart's (`RigInput::up`) through a spring: the
    /// world's up, except in anti-gravity.
    up: Vec3,
    up_velocity: Vec3,
}

/// One step of a critically damped spring (exact solution, stable for any `dt`).
/// `frequency` is the natural frequency in rad/s: higher = stiffer.
pub fn spring(
    position: Vec3,
    velocity: Vec3,
    target: Vec3,
    frequency: f32,
    dt: f32,
) -> (Vec3, Vec3) {
    let offset = position - target;
    let decay = (-frequency * dt).exp();
    let temp = (velocity + offset * frequency) * dt;
    (
        target + (offset + temp) * decay,
        (velocity - temp * frequency) * decay,
    )
}

/// Scalar version of [`spring`].
pub fn spring_f32(
    position: f32,
    velocity: f32,
    target: f32,
    frequency: f32,
    dt: f32,
) -> (f32, f32) {
    let (p, v) = spring(
        Vec3::X * position,
        Vec3::X * velocity,
        Vec3::X * target,
        frequency,
        dt,
    );
    (p.x, v.x)
}

/// Field of view the camera aims for: wider with speed, a bit more while boosting.
pub fn target_fov(tuning: &CameraTuning, speed_ratio: f32, boosting: bool) -> f32 {
    let speed = speed_ratio.clamp(0.0, 1.0);
    let boost = if boosting { tuning.boost_fov } else { 0.0 };
    tuning.base_fov + (tuning.max_speed_fov - tuning.base_fov) * speed + boost
}

/// `v` without its part along `up` (unit): its projection on the plane across `up`.
fn level(v: Vec3, up: Vec3) -> Vec3 {
    v - up * v.dot(up)
}

impl CameraRig {
    /// The camera's up now (issue #78): the world's up, but in anti-gravity the kart's, smoothed.
    pub fn up(&self) -> Vec3 {
        if self.initialized { self.up } else { Vec3::Y }
    }

    /// Advances the rig by `dt` seconds and returns the camera pose. Everything is measured
    /// across the camera's up (the world's up, or the kart's in anti-gravity, issue #78): its
    /// heading, its height, its lean.
    pub fn update(&mut self, tuning: &CameraTuning, input: &RigInput, dt: f32) -> RigPose {
        let target_up = input.up.normalize_or(Vec3::Y);
        if !self.initialized {
            self.up = target_up;
            self.up_velocity = Vec3::ZERO;
        } else if dt > 0.0 && (self.up != target_up || self.up_velocity != Vec3::ZERO) {
            let (up, velocity) = spring(
                self.up,
                self.up_velocity,
                target_up,
                tuning.up_frequency,
                dt,
            );
            self.up = up.normalize_or(target_up);
            self.up_velocity = velocity;
            // Back on the world's up (or settled on the kart's): exactly it, the rig of the other
            // circuits (a thousandth of a radian away, still: no visible jump).
            if self.up.distance_squared(target_up) < 1e-6 && velocity.length_squared() < 1e-4 {
                self.up = target_up;
                self.up_velocity = Vec3::ZERO;
            }
        }
        let up = self.up;
        let ahead = level(input.heading, up)
            .normalize_or(level(Vec3::NEG_Z, up).normalize_or(up.any_orthonormal_vector()));
        let right = ahead.cross(up);
        let look_offset = ahead * tuning.look_ahead
            + up * tuning.look_height
            // Turning right (negative yaw rate) shifts the look-at point to the right.
            - right * input.yaw_rate * tuning.turn_lead;
        let fov = target_fov(tuning, input.speed_ratio, input.boosting);

        if !self.initialized {
            *self = Self {
                initialized: true,
                behind: -ahead,
                last_position: input.position,
                look: look_offset,
                lean: input.lean,
                fov,
                up,
                ..default()
            };
        } else if dt > 0.0 {
            (self.behind, self.behind_velocity) = spring(
                self.behind,
                self.behind_velocity,
                -ahead,
                tuning.swing_frequency,
                dt,
            );
            self.behind = level(self.behind, up).normalize_or(-ahead);
            // The smoothed height, as the lag of the camera's base behind the kart along `up`:
            // the kart rising by `rise` leaves it that much lower, and the spring brings it back.
            let rise = (input.position - self.last_position).dot(up);
            (self.height_lag, self.height_velocity) = spring_f32(
                self.height_lag - rise,
                self.height_velocity,
                0.0,
                tuning.vertical_frequency,
                dt,
            );
            (self.look, self.look_velocity) = spring(
                self.look,
                self.look_velocity,
                look_offset,
                tuning.look_frequency,
                dt,
            );
            (self.lean, self.lean_velocity) = spring_f32(
                self.lean,
                self.lean_velocity,
                input.lean,
                tuning.swing_frequency,
                dt,
            );
            self.fov += (fov - self.fov) * (1.0 - (-tuning.fov_response * dt).exp());
        }
        self.last_position = input.position;

        let base = self.base(input);
        let camera_right = (-self.behind).cross(up);
        RigPose {
            eye: base
                + self.behind * tuning.distance
                + up * tuning.height
                + camera_right * self.lean * tuning.drift_side_offset,
            look_at: base + self.look,
            fov_degrees: self.fov,
            up,
        }
    }

    /// The camera's base: the kart, at the smoothed height along the camera's up.
    fn base(&self, input: &RigInput) -> Vec3 {
        input.position + self.up * self.height_lag
    }
}

impl CameraRig {
    /// Looking behind (issue #17): in front of the kart, looking back at the track behind it, at
    /// the rig's smoothed height and field of view (bumps stay soft). Call it after
    /// [`CameraRig::update`] of the same frame; it does not touch the rig's springs, so the chase
    /// camera is exactly where it would have been when the button is released.
    pub fn look_back(&self, tuning: &CameraTuning, input: &RigInput) -> RigPose {
        let up = self.up();
        let ahead = level(input.heading, up).normalize_or(-self.behind);
        let base = self.base(input);
        RigPose {
            eye: base + ahead * tuning.look_back_distance + up * tuning.look_back_height,
            look_at: base - ahead * tuning.look_back_target + up * tuning.look_height,
            fov_degrees: self.fov,
            up,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A kart driving at `speed` m/s on a circle at `yaw_rate` rad/s, starting at the origin
    /// looking down -Z. Returns the rig input at time `t`.
    fn circling(t: f32, speed: f32, yaw_rate: f32) -> RigInput {
        let angle = yaw_rate * t;
        let heading = Quat::from_rotation_y(angle) * Vec3::NEG_Z;
        let position = if yaw_rate.abs() < 1e-6 {
            Vec3::NEG_Z * speed * t
        } else {
            // Integral of the heading.
            let r = speed / yaw_rate;
            Vec3::new(-r * (1.0 - angle.cos()), 0.0, -r * angle.sin())
        };
        RigInput {
            position,
            heading,
            yaw_rate,
            speed_ratio: speed / 26.0,
            boosting: false,
            lean: 0.0,
            up: Vec3::Y,
        }
    }

    /// Straight line at `speed` until `turn_at`, then a circle at `yaw_rate` (continuous path).
    fn straight_then_turn(t: f32, speed: f32, turn_at: f32, yaw_rate: f32) -> RigInput {
        if t < turn_at {
            return circling(t, speed, 0.0);
        }
        let start = circling(turn_at, speed, 0.0).position;
        let turn = circling(t - turn_at, speed, yaw_rate);
        RigInput {
            position: start + turn.position,
            ..turn
        }
    }

    fn simulate(
        fps: f32,
        seconds: f32,
        input_at: impl Fn(f32) -> RigInput,
    ) -> Vec<(RigInput, RigPose)> {
        let tuning = CameraTuning::default();
        let mut rig = CameraRig::default();
        let frames = (seconds * fps).round() as usize;
        (0..=frames)
            .map(|i| {
                let input = input_at(i as f32 / fps);
                let dt = if i == 0 { 0.0 } else { 1.0 / fps };
                (input, rig.update(&tuning, &input, dt))
            })
            .collect()
    }

    #[test]
    fn springs_converge_without_overshoot() {
        let (mut p, mut v) = (0.0, 0.0);
        let mut highest = 0.0f32;
        for _ in 0..120 {
            (p, v) = spring_f32(p, v, 1.0, 8.0, 1.0 / 60.0);
            highest = highest.max(p);
        }
        assert!((p - 1.0).abs() < 1e-3, "{p}");
        assert!(highest <= 1.0 + 1e-5, "overshoot {highest}");
    }

    #[test]
    fn spring_is_stable_with_huge_time_steps() {
        let (p, _) = spring_f32(0.0, 0.0, 1.0, 10.0, 5.0);
        assert!((p - 1.0).abs() < 1e-3, "{p}");
    }

    #[test]
    fn sits_behind_and_above_a_kart_driving_straight() {
        let frames = simulate(60.0, 2.0, |t| circling(t, 26.0, 0.0));
        let (input, pose) = frames.last().unwrap();
        let tuning = CameraTuning::default();
        let offset = pose.eye - input.position;
        assert!((offset.z - tuning.distance).abs() < 0.01, "{offset}");
        assert!((offset.y - tuning.height).abs() < 0.01, "{offset}");
        assert!(offset.x.abs() < 0.01, "{offset}");
        // Looks ahead of the kart.
        assert!(pose.look_at.z < input.position.z);
    }

    #[test]
    fn same_result_at_30_60_and_144_fps() {
        let input = |t: f32| straight_then_turn(t, 26.0, 1.0, -1.9);
        let at_30 = simulate(30.0, 3.0, input).last().unwrap().1;
        let at_60 = simulate(60.0, 3.0, input).last().unwrap().1;
        let at_144 = simulate(144.0, 3.0, input).last().unwrap().1;
        for pose in [at_30, at_144] {
            assert!(pose.eye.distance(at_60.eye) < 0.15, "{pose:?} vs {at_60:?}");
            assert!(pose.look_at.distance(at_60.look_at) < 0.15);
        }
    }

    #[test]
    fn no_shaking_when_the_kart_starts_turning() {
        // Steering step at t = 1 s. Seen from the kart, the camera must slide smoothly to its
        // new place: no frame-to-frame jump, no back-and-forth.
        for fps in [30.0, 60.0, 144.0] {
            let frames = simulate(fps, 3.0, |t| straight_then_turn(t, 26.0, 1.0, -1.9));
            let sides: Vec<f32> = frames
                .iter()
                .map(|(input, pose)| (pose.eye - input.position).dot(input.heading.cross(Vec3::Y)))
                .collect();
            let mut direction_changes = 0;
            let mut previous = 0.0f32;
            for pair in sides.windows(2) {
                let step = pair[1] - pair[0];
                assert!(step.abs() < 0.5, "jump of {step} m at {fps} fps");
                if step.abs() > 1e-4 {
                    if previous != 0.0 && step.signum() != previous.signum() {
                        direction_changes += 1;
                    }
                    previous = step;
                }
            }
            assert!(
                direction_changes == 0,
                "camera wobbles: {direction_changes} direction changes at {fps} fps"
            );
            // ...and it does swing out in the turn (otherwise this test proves nothing).
            let swing = sides.last().unwrap().abs();
            assert!(
                swing > 1.0,
                "the camera did not swing: {swing} m at {fps} fps"
            );
        }
    }

    #[test]
    fn swings_out_in_turns_but_stays_close() {
        let frames = simulate(60.0, 4.0, |t| circling(t, 26.0, -1.9));
        let tuning = CameraTuning::default();
        for (input, pose) in &frames {
            let distance = pose.eye.distance(input.position);
            let expected = (tuning.distance.powi(2) + tuning.height.powi(2)).sqrt();
            assert!((distance - expected).abs() < 0.5, "distance {distance}");
        }
        // In a right turn, the look-at point leads toward the right of the kart.
        let (input, pose) = frames.last().unwrap();
        let right = input.heading.cross(Vec3::Y);
        assert!((pose.look_at - input.position).dot(right) > 0.5);
    }

    #[test]
    fn field_of_view_widens_with_speed_and_boost() {
        let tuning = CameraTuning::default();
        assert_eq!(target_fov(&tuning, 0.0, false), tuning.base_fov);
        assert_eq!(target_fov(&tuning, 1.0, false), tuning.max_speed_fov);
        assert_eq!(
            target_fov(&tuning, 1.3, true),
            tuning.max_speed_fov + tuning.boost_fov
        );
        let frames = simulate(60.0, 3.0, |t| RigInput {
            boosting: t > 1.0,
            ..circling(t, 26.0, 0.0)
        });
        let fov = frames.last().unwrap().1.fov_degrees;
        assert!(fov > tuning.max_speed_fov + 0.8 * tuning.boost_fov, "{fov}");
    }

    /// Issue #17: looking behind puts the camera in front of the kart, above it, looking back
    /// over it: a cut, far from the chase camera, at 30, 60 and 144 fps. `look_back` only reads
    /// the rig, whose chase springs run on (end to end in the game:
    /// `holding_c_looks_behind_then_the_chase_camera_is_back`).
    #[test]
    fn looking_behind_cuts_to_the_front_of_the_kart() {
        let tuning = CameraTuning::default();
        for fps in [30.0, 60.0, 144.0] {
            let mut rig = CameraRig::default();
            let frames = (3.0 * fps) as usize;
            for i in 0..=frames {
                let t = i as f32 / fps;
                let dt = if i == 0 { 0.0 } else { 1.0 / fps };
                let at = straight_then_turn(t, 26.0, 1.0, -1.9);
                let chase = rig.update(&tuning, &at, dt);
                let back = rig.look_back(&tuning, &at);
                let ahead = at.heading.with_y(0.0).normalize();
                let eye = back.eye - at.position;
                assert!((eye.dot(ahead) - tuning.look_back_distance).abs() < 1e-3);
                assert!(eye.y > 1.0, "{eye}");
                assert!((back.look_at - back.eye).dot(ahead) < -10.0);
                assert_eq!(back.fov_degrees, chase.fov_degrees);
                assert!(back.eye.distance(chase.eye) > 8.0, "a cut, not a pan");
            }
        }
    }

    /// Issue #78: a kart driving onto a wall (anti-gravity): its up turns from the world's up to
    /// the side (+X) in 1.5 s, stays there, then turns back. The camera's up follows it without a
    /// jolt (no jump from one frame to the next, at 30, 60 and 144 fps), sits behind and above
    /// the kart in the kart's frame once the turn is over (the same place as on the ground, turned),
    /// and is exactly the world's up again once the kart is back on the ground.
    #[test]
    fn the_camera_rolls_with_a_kart_onto_a_wall_and_back() {
        let tuning = CameraTuning::default();
        let wall = Vec3::X;
        let up_at = |t: f32| {
            let roll = |from: f32| ((t - from) / 1.5).clamp(0.0, 1.0);
            let angle = (roll(1.0) - roll(5.0)) * std::f32::consts::FRAC_PI_2;
            Quat::from_rotation_z(-angle) * Vec3::Y
        };
        for fps in [30.0, 60.0, 144.0] {
            let frames = simulate(fps, 9.0, |t| RigInput {
                up: up_at(t),
                ..circling(t, 26.0, 0.0)
            });
            let mut previous: Option<(Vec3, Vec3)> = None;
            for (input, pose) in &frames {
                let offset = pose.eye - input.position;
                if let Some((up, eye)) = previous {
                    assert!(pose.up.angle_between(up) < 0.06, "{fps} fps: the up jumps");
                    assert!(offset.distance(eye) < 0.5, "{fps} fps: the eye jumps");
                }
                assert!((pose.up.length() - 1.0).abs() < 1e-4);
                previous = Some((pose.up, offset));
            }
            // On the wall, settled: behind the kart and above it along the wall's normal.
            let (input, pose) = &frames[(4.5 * fps) as usize];
            let offset = pose.eye - input.position;
            assert!(pose.up.dot(wall) > 0.999, "{fps} fps: {}", pose.up);
            assert!((offset.dot(wall) - tuning.height).abs() < 0.05, "{offset}");
            assert!(
                (offset.dot(-input.heading) - tuning.distance).abs() < 0.1,
                "{offset}"
            );
            // Back on the ground: exactly the world's up, the camera of the other circuits.
            let (_, pose) = frames.last().unwrap();
            assert_eq!(pose.up, Vec3::Y, "{fps} fps");
        }
    }

    #[test]
    fn lean_shifts_the_camera_sideways() {
        let tuning = CameraTuning::default();
        let frames = simulate(60.0, 2.0, |t| RigInput {
            lean: 1.0,
            ..circling(t, 10.0, 0.0)
        });
        let (input, pose) = frames.last().unwrap();
        let right = input.heading.cross(Vec3::Y);
        let side = (pose.eye - input.position).dot(right);
        assert!((side - tuning.drift_side_offset).abs() < 0.05, "{side}");
    }
}
