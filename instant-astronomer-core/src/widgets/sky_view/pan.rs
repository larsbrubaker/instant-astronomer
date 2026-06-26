//! Grab-to-pan camera math for the sky view.
//!
//! "Drag to look" should feel like grabbing the sky and spinning the whole
//! celestial sphere under your finger — the same model Google Sky Map uses.
//! Sky Map's `ManualOrientationController` rotates the pointing about the
//! camera's *own* up/right axes (roll is allowed; the horizon can tilt), so a
//! drag is a single rigid rotation of everything on screen, never a shear.
//!
//! [`drag_view_quat`] reproduces that exactly: it reconstructs the pin-hole
//! view-space rays (see `SkyViewWidget::project_horizontal`) for the previous
//! and current pointer pixels and pre-multiplies the camera by the single
//! rotation that carries the old ray onto the new one. That rotation maps the
//! grabbed point precisely onto the new finger position while rotating the
//! rest of the sky rigidly with it.
//!
//! Earlier attempts went wrong by *levelling out roll* after this rotation
//! (which re-introduced drift away from the optical-axis row) or by solving
//! for a zero-roll yaw/pitch that pinned one point but sheared everything
//! else (horizontal drags bled into vertical motion). Both fought the
//! direct-manipulation feel; this does not.

use agg_gui::geometry::Point;
use nalgebra::{UnitQuaternion, Vector3};

/// View-space ray direction (unnormalised) for a screen pixel `p`, using the
/// same pin-hole model as the projection: `screen = center + (v.xy / v.z) *
/// focal`, so a pixel maps back to the ray `(p - center, focal)`.
fn ray_for_pixel(p: Point, center: Point, focal: f64) -> Vector3<f64> {
    Vector3::new(p.x - center.x, p.y - center.y, focal)
}

/// New world→view quaternion after dragging the pointer from `last` to `pos`,
/// keeping the world point under `last` pinned to `pos` (grab-to-pan) by
/// rotating the whole view rigidly. `center`/`focal` must match the
/// projection's values. Returns the input unchanged for a degenerate
/// (zero-length) move.
///
/// The rotation is applied in *view* space (pre-multiplied), so it composes
/// correctly regardless of the right-applied calibration offset
/// (`effective = view * q_cal`): the calibration cancels, leaving the grabbed
/// world direction landing exactly under the new pixel. Roll is intentionally
/// preserved — the horizon may tilt, matching Sky Map's free rotation.
pub(super) fn drag_view_quat(
    view_quat: UnitQuaternion<f64>,
    last: Point,
    pos: Point,
    center: Point,
    focal: f64,
) -> UnitQuaternion<f64> {
    let d_old = ray_for_pixel(last, center, focal);
    let d_new = ray_for_pixel(pos, center, focal);
    match UnitQuaternion::rotation_between(&d_old, &d_new) {
        Some(q_v) => q_v * view_quat,
        None => view_quat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CENTER: Point = Point { x: 200.0, y: 180.0 };
    const FOCAL: f64 = 270.0;

    fn cal_quat(cal: f64) -> UnitQuaternion<f64> {
        UnitQuaternion::from_axis_angle(&Vector3::y_axis(), -cal)
    }

    /// World direction under a screen pixel for a given view + calibration,
    /// matching `SkyViewWidget::project_horizontal`'s `effective = view *
    /// q_cal` convention.
    fn world_under(view: UnitQuaternion<f64>, cal: f64, p: Point) -> Vector3<f64> {
        (view * cal_quat(cal))
            .inverse_transform_vector(&ray_for_pixel(p, CENTER, FOCAL))
            .normalize()
    }

    /// Project a world direction the way the widget does.
    fn project(view: UnitQuaternion<f64>, cal: f64, world: Vector3<f64>) -> Option<Point> {
        let v = (view * cal_quat(cal)).transform_vector(&world);
        if v.z <= 0.05 {
            return None;
        }
        Some(Point::new(
            CENTER.x + (v.x / v.z) * FOCAL,
            CENTER.y + (v.y / v.z) * FOCAL,
        ))
    }

    /// Grab the world point under `start`, drag to `end`, and assert it lands
    /// exactly under `end` in the new view.
    fn assert_grab(view: UnitQuaternion<f64>, cal: f64, start: Point, end: Point) {
        let grabbed = world_under(view, cal, start);
        let new_view = drag_view_quat(view, start, end, CENTER, FOCAL);
        let landed = project(new_view, cal, grabbed).expect("grabbed point in front");
        assert!(
            (landed.x - end.x).abs() < 1e-6 && (landed.y - end.y).abs() < 1e-6,
            "grabbed point should land under {end:?}, got {landed:?}"
        );
    }

    #[test]
    fn horizontal_drag_on_center_row() {
        assert_grab(
            UnitQuaternion::identity(),
            0.0,
            Point::new(260.0, 180.0),
            Point::new(120.0, 180.0),
        );
    }

    /// The case the user hit: a horizontal drag *off* the optical-axis row.
    #[test]
    fn horizontal_drag_off_center_row() {
        assert_grab(
            UnitQuaternion::identity(),
            0.0,
            Point::new(260.0, 60.0),
            Point::new(110.0, 60.0),
        );
    }

    #[test]
    fn vertical_drag_off_center_column() {
        assert_grab(
            UnitQuaternion::identity(),
            0.0,
            Point::new(320.0, 120.0),
            Point::new(320.0, 300.0),
        );
    }

    #[test]
    fn diagonal_drag_keeps_point_under_finger() {
        assert_grab(
            UnitQuaternion::identity(),
            0.0,
            Point::new(150.0, 110.0),
            Point::new(300.0, 250.0),
        );
    }

    /// Works from an already-rotated start orientation and with a non-zero
    /// calibration yaw offset (the offset must cancel out).
    #[test]
    fn drag_from_rotated_view_with_calibration() {
        let view: UnitQuaternion<f64> = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), 0.6)
            * UnitQuaternion::from_axis_angle(&Vector3::x_axis(), 0.25);
        assert_grab(view, 0.3, Point::new(240.0, 90.0), Point::new(130.0, 230.0));
    }

    /// A degenerate (zero-length) move leaves the orientation untouched.
    #[test]
    fn zero_move_is_identity() {
        let view = UnitQuaternion::identity();
        let same = drag_view_quat(view, Point::new(50.0, 70.0), Point::new(50.0, 70.0), CENTER, FOCAL);
        assert!((view.angle_to(&same)) < 1e-12);
    }
}
