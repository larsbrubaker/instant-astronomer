//! Rigid-body sensor-fusion for the device-orientation channel.
//!
//! The `view_quat` target is built from the **full W3C
//! `(alpha, beta, gamma)` triple** as a continuous rotation matrix,
//! never decomposed back into a single yaw/pitch pair. That mirrors
//! Sky Map (sky-map-team/stardroid), which consumes Android's fused
//! rotation-vector quaternion directly and never touches Euler. The
//! earlier per-axis path here dropped `gamma`, which made the
//! Tait-Bryan ZXY decomposition gimbal-lock visible to the user as
//! a 180° yaw flip when the phone tilted past the horizon
//! (β = ±π/2).
//!
//! Contains the slerp-weight math, the `apply_device_orientation`
//! entry point WASM calls on every `deviceorientation` event, and
//! the `view_quat_heading_rad` helper the Calibrate button uses to
//! snapshot the current compass heading.

use nalgebra::{Matrix3, Rotation3, UnitQuaternion};

use crate::AstronomerHandles;

/// Extract the W3C-convention compass heading (CCW from north, in
/// radians) from a world→view quaternion. Used by the Calibrate
/// button and the HorizonTapeWidget so they agree on "which direction
/// is the camera pointing right now?"
///
/// Implementation: the camera-forward direction in **world** coords is
/// `view_quat.inverse() * (0, 0, 1)`. Heading = `atan2(-x, z)` puts
/// north (0,0,1)→0, east (1,0,0)→-π/2 (i.e. CCW = +90°/east in W3C
/// world). Negating recovers W3C alpha.
pub fn view_quat_heading_rad(view_quat: UnitQuaternion<f64>) -> f64 {
    let forward_world = view_quat.inverse_transform_vector(&nalgebra::Vector3::new(0.0, 0.0, 1.0));
    -forward_world.x.atan2(forward_world.z)
}

/// Slerp weight for **tilt-dominated** events (pitch / roll). Tilt is
/// driven by the device's accelerometer — physical and stable — so we
/// track it aggressively. Matches the high-alpha (0.7) damping Sky Map
/// applies to its `TYPE_ACCELEROMETER` channel.
pub const FUSION_TILT_WEIGHT: f64 = 0.30;

/// Angle gap (radians) at which a **yaw-dominated** event reaches the
/// full tilt-weight pass-through. Below this knee, the slerp weight
/// scales quadratically with the gap — tiny compass jitter (a few
/// tenths of a degree) is essentially frozen, while genuine head
/// turns pass through. Mirrors the `ExponentiallyWeightedSmoother`
/// shape Sky Map runs on its magnetometer channel, lifted to
/// quaternion space.
pub const FUSION_YAW_KNEE_RAD: f64 = 5.0 * std::f64::consts::PI / 180.0;

/// Slerp weight for a sensor-fusion event, computed from the
/// rotation needed to take `current` → `target`. Single coherent
/// rotation, single weight — no per-Euler-axis filtering — but the
/// weight depends on **what kind** of rotation it is.
///
/// Yaw rotations (axis ≈ world-up) get magnitude-gain smoothing:
/// tiny gaps are crushed, large gaps follow. Tilt rotations (axis ≈
/// horizontal) get the full `FUSION_TILT_WEIGHT`. Mixed axes
/// linearly interpolate, so the slerp remains rigid-body coherent —
/// no risk of yaw lagging pitch when the user turns the phone.
fn fusion_slerp_weight(
    current: UnitQuaternion<f64>,
    target: UnitQuaternion<f64>,
) -> f64 {
    let delta = target * current.inverse();
    let angle = delta.angle();
    if angle < 1e-9 {
        return 0.0;
    }
    let yaw_share = delta.axis().map(|a| a.y.abs()).unwrap_or(0.0);
    let yaw_gain = (angle / FUSION_YAW_KNEE_RAD).powi(2).min(1.0);
    let yaw_weight = FUSION_TILT_WEIGHT * yaw_gain;
    yaw_share * yaw_weight + (1.0 - yaw_share) * FUSION_TILT_WEIGHT
}

/// Build the world→view rotation quaternion from a W3C
/// `(alpha, beta, gamma)` triple **without decomposing into a
/// single yaw / pitch pair**.
///
/// The W3C spec defines the device→earth rotation as
/// `R = Rz(α) · Rx(β) · Ry(γ)`, where W3C earth coords are
/// X-east, Y-north, Z-up (right-handed) and W3C device coords are
/// X-right-of-screen, Y-top-of-screen, Z-out-of-screen.
///
/// Our world frame is X-east, Y-up, Z-north — a Y↔Z swap of the
/// W3C earth frame. We apply that swap inline and project R onto
/// the three view-basis vectors to get the world→view matrix:
///
/// - row 0 = screen-right direction expressed in our world coords
/// - row 1 = screen-up direction expressed in our world coords
/// - row 2 = camera-forward direction (= back of phone) in our world
///
/// Sky Map's port of the same idea reads the analogous three rows
/// straight out of Android's `getRotationMatrixFromVector` output;
/// we compute them analytically because the browser hands us Euler
/// angles rather than a fused rotation vector. **Critically**: the
/// matrix is a continuous function of `(α, β, γ)` — including
/// across `β = ±π/2` where the underlying Tait-Bryan ZXY
/// decomposition is gimbal-locked. Dropping γ (the previous
/// behaviour) made that gimbal-lock visible as a 180° yaw jump
/// when the user tilted the phone past the horizon line.
fn build_view_quat_w3c(
    alpha_rad: f64,
    beta_rad: f64,
    gamma_rad: f64,
) -> UnitQuaternion<f64> {
    let (sa, ca) = alpha_rad.sin_cos();
    let (sb, cb) = beta_rad.sin_cos();
    let (sg, cg) = gamma_rad.sin_cos();

    let m = Matrix3::new(
        cg * ca - sg * sb * sa, -sg * cb, cg * sa + sg * sb * ca,
        -sa * cb,                sb,       ca * cb,
        -ca * sg - sa * sb * cg, -cb * cg, ca * sb * cg - sa * sg,
    );
    UnitQuaternion::from(Rotation3::from_matrix_unchecked(m))
}

/// Apply a device-orientation reading to the shared `view_quat` using
/// rigid-body sensor fusion: slerp the **whole** quaternion toward
/// the target each event, with a magnitude- and axis-dependent weight.
///
/// The earlier per-axis approach (low-pass alpha, pass beta through
/// unfiltered) violated the geometric coupling between yaw and pitch
/// — when the user turned the phone, pitch updated immediately and
/// yaw arrived 200 ms later, producing the "view swings around
/// later" feel reported on mobile. Filtering the orientation as a
/// single rigid-body rotation fixes that, but a fixed slerp weight
/// trades off compass jitter against responsive tilt tracking.
///
/// Sky Map (sky-map-team/stardroid) resolves the same trade-off by
/// running separate damping on the gravity channel (alpha 0.7,
/// responsive) and the magnetometer channel (alpha 0.05 plus a cubic
/// `ExponentiallyWeightedSmoother` that crushes sub-degree jitter).
/// We can't separate the channels — the browser hands us a fused
/// Euler triple — but we can recover the same effect by looking at
/// the **axis** of the current→target rotation: if it points along
/// world-up the change is yaw-like (compass-driven, smooth hard);
/// otherwise it's tilt-like (gravity-driven, follow fast). See
/// [`fusion_slerp_weight`] for the math.
///
/// Inputs are radians and correspond directly to the W3C
/// `DeviceOrientationEvent` triple: `alpha_rad` (CCW from north
/// about the up axis), `beta_rad` (front-to-back tilt; π/2 = phone
/// upright facing horizon), `gamma_rad` (left-to-right tilt /
/// roll). All three are required for the resulting quaternion to
/// stay continuous when the phone tilts past the horizon.
///
/// First event after the handle is created snaps to the target so
/// the view doesn't visibly drift from identity to the device's
/// real orientation over half a second on startup.
pub fn apply_device_orientation(
    handles: &AstronomerHandles,
    alpha_rad: f64,
    beta_rad: f64,
    gamma_rad: f64,
) {
    if !handles.use_device_orientation.get() {
        return;
    }
    let target = build_view_quat_w3c(alpha_rad, beta_rad, gamma_rad);

    let next = if handles.fusion_seeded.get() {
        let current = handles.view_quat.get();
        let weight = fusion_slerp_weight(current, target);
        current.slerp(&target, weight)
    } else {
        // First event — snap so we don't visibly drift from identity
        // (or from wherever a previous mouse drag left things)
        // toward the device's actual orientation.
        handles.fusion_seeded.set(true);
        target
    };
    handles.view_quat.set(next);
    agg_gui::animation::request_draw();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    fn make_handles() -> AstronomerHandles {
        AstronomerHandles {
            latitude: Rc::new(Cell::new(0.0)),
            longitude: Rc::new(Cell::new(0.0)),
            timestamp_ms: Rc::new(Cell::new(0)),
            view_quat: Rc::new(Cell::new(UnitQuaternion::<f64>::identity())),
            calibration_yaw: Rc::new(Cell::new(0.0)),
            use_device_orientation: Rc::new(Cell::new(true)),
            fusion_seeded: Rc::new(Cell::new(false)),
        }
    }

    use std::f64::consts::{FRAC_PI_2, FRAC_PI_3, PI};

    /// Phone-upright reading: β = π/2 means the user is holding the
    /// phone vertically with the back of the screen pointing at
    /// the horizon. This is the W3C-spec equivalent of the
    /// previous `pitch = 0` shorthand and is what the tests below
    /// use as their "horizon-pointing" baseline.
    const HORIZON_BETA: f64 = FRAC_PI_2;

    /// First `apply_device_orientation` event should snap so the view
    /// doesn't visibly drift from identity to the device's real
    /// orientation over ~500 ms on startup. The user reported this
    /// as a "settling" pause when first turning the compass on.
    #[test]
    fn apply_device_orientation_snaps_first_event() {
        let h = make_handles();
        apply_device_orientation(&h, 1.0, HORIZON_BETA + 0.5, 0.0);
        let q = h.view_quat.get();
        let expected = build_view_quat_w3c(1.0, HORIZON_BETA + 0.5, 0.0);
        assert!(
            q.angle_to(&expected) < 1e-9,
            "first event must snap to target, off by {} rad",
            q.angle_to(&expected)
        );
        assert!(h.fusion_seeded.get(), "fusion_seeded should flip true");
    }

    /// A large yaw-dominated event after the snap should pass
    /// through at `FUSION_TILT_WEIGHT` — well above the knee, the
    /// quadratic gain saturates at 1.0 so yaw weight equals tilt
    /// weight. The slerp is still a single rigid-body rotation.
    #[test]
    fn apply_device_orientation_slerps_large_yaw_at_full_weight() {
        let h = make_handles();
        // Snap to (α=1.0, looking just below horizon).
        apply_device_orientation(&h, 1.0, HORIZON_BETA - 0.5, 0.0);
        let q_first = h.view_quat.get();
        // 1 rad yaw gap, well above knee. Same β, same γ → pure yaw.
        apply_device_orientation(&h, 2.0, HORIZON_BETA - 0.5, 0.0);
        let q_second = h.view_quat.get();
        let target = build_view_quat_w3c(2.0, HORIZON_BETA - 0.5, 0.0);
        let total = q_first.angle_to(&target);
        let moved = q_first.angle_to(&q_second);
        let ratio = moved / total;
        assert!(
            (ratio - FUSION_TILT_WEIGHT).abs() < 0.02,
            "large-gap slerp ratio should be ~{FUSION_TILT_WEIGHT}, got {ratio:.3}"
        );
    }

    /// Sub-degree yaw "jitter" — typical of magnetometer noise — must
    /// be crushed. Mirrors Sky Map's `ExponentiallyWeightedSmoother`
    /// behaviour on its magnetometer channel: quadratic gain below
    /// the knee renders compass noise effectively frozen.
    #[test]
    fn apply_device_orientation_crushes_small_yaw_jitter() {
        let h = make_handles();
        // Seed at horizon-north, slightly off the pole so the rotation
        // axis between consecutive events is well-defined.
        apply_device_orientation(&h, 0.0, HORIZON_BETA - 0.2, 0.0);
        let q_seed = h.view_quat.get();
        let jitter = 0.5_f64.to_radians();
        apply_device_orientation(&h, jitter, HORIZON_BETA - 0.2, 0.0);
        let moved = q_seed.angle_to(&h.view_quat.get());
        let ratio = moved / jitter;
        // (0.5° / 5°)² * 0.30 = 0.003 — view barely moves.
        assert!(
            ratio < 0.01,
            "small yaw jitter must be crushed, ratio={ratio:.4}"
        );
    }

    /// A tilt-dominated event (gravity is the stable channel) must
    /// pass through at full `FUSION_TILT_WEIGHT` even for small
    /// angles. We don't deadband pitch — that's what gave the
    /// "settles late" feel on real motion.
    #[test]
    fn apply_device_orientation_tracks_small_tilt() {
        let h = make_handles();
        apply_device_orientation(&h, 0.0, HORIZON_BETA - 0.2, 0.0); // snap
        let q_seed = h.view_quat.get();
        let tilt = 0.5_f64.to_radians();
        apply_device_orientation(&h, 0.0, HORIZON_BETA - 0.2 + tilt, 0.0);
        let moved = q_seed.angle_to(&h.view_quat.get());
        let ratio = moved / tilt;
        assert!(
            (ratio - FUSION_TILT_WEIGHT).abs() < 0.02,
            "small tilt should track at {FUSION_TILT_WEIGHT}, got {ratio:.3}"
        );
    }

    /// `use_device_orientation = false` should leave view_quat alone
    /// even when an event fires. Also must NOT flip `fusion_seeded`
    /// — otherwise re-enabling the compass would silently skip the
    /// startup snap on the next event.
    #[test]
    fn apply_device_orientation_no_op_when_disabled() {
        let h = make_handles();
        h.use_device_orientation.set(false);
        apply_device_orientation(&h, 1.0, HORIZON_BETA + 0.5, 0.0);
        assert!(h.view_quat.get().angle() < 1e-9, "view_quat must not change");
        assert!(!h.fusion_seeded.get(), "must not seed while disabled");
    }

    /// At the Tait-Bryan ZXY pole β = π/2 only `(α + γ)` is
    /// physically determined — `α` and `γ` individually can swap by
    /// any amount as long as their sum stays fixed and represent
    /// the **same** physical rotation. Real `deviceorientation`
    /// sensors hit this when the phone is held vertically (i.e.
    /// camera pointing at the horizon, the app's primary
    /// orientation), and the user saw it as a 180° jump when the
    /// previous code dropped γ. The matrix construction must
    /// produce the same quaternion for any Euler triple that
    /// represents the same rotation.
    #[test]
    fn build_view_quat_w3c_collapses_gimbal_lock_pole() {
        let q_zero = build_view_quat_w3c(0.0, FRAC_PI_2, 0.0);
        let q_swapped = build_view_quat_w3c(FRAC_PI_3, FRAC_PI_2, -FRAC_PI_3);
        let gap = q_zero.angle_to(&q_swapped);
        assert!(
            gap < 1e-9,
            "Euler triples with the same (α+γ) at β=π/2 must yield the same quaternion; gap = {} rad",
            gap,
        );
    }

    /// Continuity test for the horizon-crossing path the user
    /// reported. As β sweeps through π/2 (phone tilts past
    /// vertical) the resulting view_quat must change by an amount
    /// proportional to the physical motion — not flip 180° because
    /// of a hidden Euler discontinuity.
    #[test]
    fn build_view_quat_w3c_smooth_across_horizon() {
        let q_below = build_view_quat_w3c(0.0, FRAC_PI_2 - 0.001, 0.0);
        let q_above = build_view_quat_w3c(0.0, FRAC_PI_2 + 0.001, 0.0);
        let gap = q_below.angle_to(&q_above);
        // The two readings are 0.002 rad apart physically; allow up to
        // 10× slack for numerical noise. The previous gamma-dropped
        // build would have produced ~π rad here.
        assert!(
            gap < 0.02,
            "view_quat must be continuous across β=π/2; got {} rad gap (expected ~0.002)",
            gap,
        );
    }

    /// End-to-end horizon-crossing test through the full fusion
    /// entry point. Mirrors what the browser actually delivers when
    /// the user tilts the phone past vertical — and what previously
    /// produced the visible jump.
    #[test]
    fn apply_device_orientation_no_jump_at_horizon() {
        let h = make_handles();
        // Snap looking slightly below horizon, north.
        apply_device_orientation(&h, 0.0, FRAC_PI_2 - 0.05, 0.0);
        let q_before = h.view_quat.get();
        // One frame later, β has crossed the pole.
        apply_device_orientation(&h, 0.0, FRAC_PI_2 + 0.05, 0.0);
        let q_after = h.view_quat.get();
        let gap = q_before.angle_to(&q_after);
        // Physical change is ~0.1 rad; slerp will move ~30% of that,
        // so a ~0.03 rad delta is expected. Anything close to π
        // means the old gimbal-lock jump is back.
        assert!(
            gap < 0.1,
            "no jump expected across horizon, got {} rad ({}°)",
            gap,
            gap.to_degrees(),
        );
        let _ = PI; // keep the import warning quiet
    }
}
