//! HUD painters for the sky view — horizon strip, altitude ladder,
//! centre reticle, and the tapped-body info card.
//!
//! Pulled out of `sky_view.rs` purely to keep that file under the
//! 800-line guardrail. Each function takes everything it needs by
//! value / reference — no shared state with `SkyViewWidget` beyond
//! the structs defined in the parent module, which we import via
//! `use super::*`.

use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::geometry::{Point, Rect};
use agg_gui::text::Font;

use crate::math::{horizontal_to_cartesian, HorizontalCoords};

use super::{PaintedBody, Selection};

use std::f64::consts::PI;

/// Paint a horizontal horizon line at the bottom of the sky view
/// with a faint "ground" band below it and cardinal direction
/// labels (N / NE / E / …) sliding along its top edge.
///
/// The line itself sits at a fixed Y so the user always has a
/// stable bottom-of-screen reference. Cardinal labels use the
/// *current* projection of each compass direction on the celestial
/// sphere to pick their X position, so as the user pans the sky the
/// labels slide accordingly.
pub(super) fn paint_horizon_strip(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    w: f64,
    _h: f64,
    rot: &nalgebra::Matrix3<f64>,
    center: Point,
    focal_length: f64,
) {
    let ground_h = 36.0_f64;
    let horizon_y = ground_h;

    ctx.set_fill_color(Color::from_rgba8(4, 4, 10, 220));
    ctx.begin_path();
    ctx.rect(0.0, 0.0, w, ground_h);
    ctx.fill();

    for i in 0..6 {
        let alpha = 18 - i * 3;
        let yy = horizon_y + i as f64;
        ctx.set_stroke_color(Color::from_rgba8(120, 100, 80, alpha.max(0) as u8));
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(0.0, yy);
        ctx.line_to(w, yy);
        ctx.stroke();
    }

    ctx.set_stroke_color(Color::from_rgba8(255, 180, 120, 200));
    ctx.set_line_width(1.2);
    ctx.begin_path();
    ctx.move_to(0.0, horizon_y);
    ctx.line_to(w, horizon_y);
    ctx.stroke();

    let directions: [(&str, f64); 8] = [
        ("N", 0.0),
        ("NE", PI / 4.0),
        ("E", PI / 2.0),
        ("SE", 3.0 * PI / 4.0),
        ("S", PI),
        ("SW", 5.0 * PI / 4.0),
        ("W", 3.0 * PI / 2.0),
        ("NW", 7.0 * PI / 4.0),
    ];

    ctx.set_font(font);
    for (name, az) in directions {
        let hc = HorizontalCoords { alt: 0.0, az };
        let v_cart = horizontal_to_cartesian(hc);
        let v_rot = rot * v_cart;
        let (x, _, z) = (v_rot.x, v_rot.y, v_rot.z);
        if z <= 0.05 {
            continue;
        }
        let projected_x = center.x + (x / z) * focal_length;
        if projected_x < -20.0 || projected_x > w + 20.0 {
            continue;
        }

        ctx.set_stroke_color(Color::from_rgba8(255, 200, 140, 220));
        ctx.set_line_width(if name.len() == 1 { 1.6 } else { 1.0 });
        ctx.begin_path();
        ctx.move_to(projected_x, horizon_y - 6.0);
        ctx.line_to(projected_x, horizon_y + 6.0);
        ctx.stroke();

        let is_cardinal = name.len() == 1;
        let label_size = if is_cardinal { 13.0 } else { 10.0 };
        let label_color = if is_cardinal {
            if name == "N" {
                Color::from_rgb8(255, 110, 110)
            } else {
                Color::from_rgb8(255, 220, 160)
            }
        } else {
            Color::from_rgba8(255, 200, 150, 180)
        };
        let approx_w = name.chars().count() as f64 * label_size * 0.6;
        ctx.set_fill_color(label_color);
        ctx.set_font_size(label_size);
        ctx.fill_text(name, projected_x - approx_w / 2.0, 6.0);
    }
}

/// Altitude (radians above the horizon) the camera is currently
/// aimed at — derived from the camera-forward direction extracted
/// from the rotation matrix's third row.
pub(super) fn screen_centre_altitude(rot: &nalgebra::Matrix3<f64>) -> f64 {
    let cam_forward = nalgebra::Vector3::new(rot[(2, 0)], rot[(2, 1)], rot[(2, 2)]);
    let len = cam_forward.norm().max(1e-9);
    (cam_forward.y / len).asin()
}

/// Paint a small vertical pitch tape along the right edge of the
/// sky view. Tick marks every 10° from −90° (nadir) to +90°
/// (zenith) with major labels every 30°. The current centre
/// altitude is marked with a glowing amber chevron.
pub(super) fn paint_altitude_ladder(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    w: f64,
    h: f64,
    centre_alt: f64,
) {
    let strip_w = 30.0_f64;
    let ground_h = 36.0_f64;
    let ladder_top = h - 12.0;
    let ladder_bottom = ground_h + 12.0;
    let ladder_h = (ladder_top - ladder_bottom).max(40.0);
    let x0 = w - strip_w - 8.0;

    ctx.set_fill_color(Color::from_rgba8(0, 0, 0, 110));
    ctx.begin_path();
    ctx.rounded_rect(x0, ladder_bottom, strip_w, ladder_h, 6.0);
    ctx.fill();

    let alt_to_y = |alt_deg: f64| -> f64 { ladder_bottom + ((alt_deg + 90.0) / 180.0) * ladder_h };

    ctx.set_stroke_color(Color::from_rgba8(255, 180, 120, 220));
    ctx.set_line_width(1.5);
    let y_h = alt_to_y(0.0);
    ctx.begin_path();
    ctx.move_to(x0, y_h);
    ctx.line_to(x0 + strip_w, y_h);
    ctx.stroke();

    ctx.set_font(font);
    for deg in (-90..=90).step_by(10) {
        let y = alt_to_y(deg as f64);
        let major = deg % 30 == 0;
        let tick_color = if major {
            Color::from_rgba8(220, 220, 240, 200)
        } else {
            Color::from_rgba8(180, 180, 200, 120)
        };
        ctx.set_stroke_color(tick_color);
        ctx.set_line_width(if major { 1.2 } else { 0.8 });
        let tick_inset = if major { 4.0 } else { 8.0 };
        ctx.begin_path();
        ctx.move_to(x0 + tick_inset, y);
        ctx.line_to(x0 + strip_w - 2.0, y);
        ctx.stroke();
        if major {
            let label = format!("{}", deg);
            ctx.set_fill_color(Color::from_rgba8(220, 220, 240, 200));
            ctx.set_font_size(10.0);
            let est_w = label.chars().count() as f64 * 5.5;
            ctx.fill_text(&label, x0 + (strip_w - est_w) * 0.5 - 4.0, y - 3.0);
        }
    }

    let alt_deg = centre_alt.to_degrees();
    let clamped = alt_deg.clamp(-90.0, 90.0);
    let y_now = alt_to_y(clamped);
    let chev = 5.0_f64;
    ctx.set_fill_color(Color::from_rgb8(255, 200, 90));
    ctx.begin_path();
    ctx.move_to(x0 - 1.0, y_now);
    ctx.line_to(x0 - 1.0 - chev, y_now + chev * 0.7);
    ctx.line_to(x0 - 1.0 - chev, y_now - chev * 0.7);
    ctx.close_path();
    ctx.fill();
    ctx.set_fill_color(Color::from_rgb8(255, 200, 90));
    ctx.begin_path();
    ctx.move_to(x0 + strip_w + 1.0, y_now);
    ctx.line_to(x0 + strip_w + 1.0 + chev, y_now + chev * 0.7);
    ctx.line_to(x0 + strip_w + 1.0 + chev, y_now - chev * 0.7);
    ctx.close_path();
    ctx.fill();

    let label = format!("alt {:+.0}°", clamped);
    ctx.set_fill_color(Color::from_rgb8(255, 220, 160));
    ctx.set_font_size(11.0);
    let est_w = label.chars().count() as f64 * 6.5;
    let label_y = (y_now - 18.0).max(ladder_bottom + 2.0);
    ctx.fill_text(&label, x0 + (strip_w - est_w) * 0.5, label_y);
}

/// Paint a small crosshair at the centre of the sky view and a
/// ribbon at the top showing both the current centre altitude and
/// the nearest celestial body to the crosshair. Lets the user
/// "aim" at a bright object and read what it is without tapping.
/// Reticle radius in logical pixels. Anything painted inside this
/// circle counts as "the user is aiming at it" and gets its name
/// printed below.
pub(super) const RETICLE_RADIUS: f64 = 16.0;

pub(super) fn paint_centre_reticle(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    w: f64,
    h: f64,
    _centre_alt: f64,
    painted: &[PaintedBody],
) {
    let centre = Point::new(w / 2.0, h * 0.6);

    // Ring + tiny centre dot.
    ctx.set_stroke_color(Color::from_rgba8(255, 240, 180, 180));
    ctx.set_line_width(1.4);
    ctx.begin_path();
    ctx.circle(centre.x, centre.y, RETICLE_RADIUS);
    ctx.stroke();
    ctx.set_fill_color(Color::from_rgba8(255, 240, 180, 220));
    ctx.begin_path();
    ctx.circle(centre.x, centre.y, 1.5);
    ctx.fill();

    // Find the brightest body fully inside the reticle ring; ties
    // broken toward the nearer body.
    let mut best: Option<(f64, &PaintedBody)> = None;
    for body in painted {
        let dx = body.pos.x - centre.x;
        let dy = body.pos.y - centre.y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist > RETICLE_RADIUS {
            continue;
        }
        let score = (body.magnitude as f64) + dist * 0.05;
        match &best {
            Some((best_score, _)) if score >= *best_score => {}
            _ => best = Some((score, body)),
        }
    }

    let Some((_, body)) = best else {
        return;
    };

    // Paint the name immediately below the reticle so the eye can read
    // it without moving — proximate to where the user is aiming.
    ctx.set_font(font);
    let name_size = 14.0_f64;
    let detail_size = 11.0_f64;
    let pad_x = 12.0_f64;
    let pad_y = 9.0_f64;
    let line_gap = 5.0_f64;
    let name = body.name.clone();
    let mag_label = format!("mag {:+.1}", body.magnitude);
    let approx = |s: &str, sz: f64| (s.chars().count() as f64) * sz * 0.6 + pad_x * 2.0;
    let card_w = approx(&name, name_size).max(approx(&mag_label, detail_size));
    let card_h = name_size + detail_size + line_gap + pad_y * 2.0;
    let card_x = (centre.x - card_w / 2.0).clamp(8.0, w - card_w - 8.0);
    let card_top = centre.y - RETICLE_RADIUS - 6.0;
    let card_y = (card_top - card_h).max(8.0);

    ctx.set_fill_color(Color::from_rgba8(15, 20, 38, 230));
    ctx.begin_path();
    ctx.rounded_rect(card_x, card_y, card_w, card_h, 7.0);
    ctx.fill();
    ctx.set_stroke_color(Color::from_rgba8(255, 215, 90, 180));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.rounded_rect(card_x, card_y, card_w, card_h, 7.0);
    ctx.stroke();

    ctx.set_fill_color(Color::from_rgb8(255, 235, 150));
    ctx.set_font_size(name_size);
    let name_baseline = card_y + card_h - pad_y - name_size * 0.25;
    ctx.fill_text(&name, card_x + pad_x, name_baseline);

    ctx.set_fill_color(Color::from_rgb8(200, 205, 225));
    ctx.set_font_size(detail_size);
    let mag_baseline = card_y + pad_y - detail_size * 0.15;
    ctx.fill_text(&mag_label, card_x + pad_x, mag_baseline);
}

/// Paint the projected `alt = 0` horizon line as a dim curve across
/// the field of view. Sampled at every 2° of azimuth around the full
/// horizon ring; pairs whose either endpoint is behind the camera are
/// skipped, leaving only the visible arc.
///
/// When the camera is tilted up the line drops below the centre of
/// the screen; when level it sits at the centre; when tilted down it
/// rides above centre — giving the user a "you are above / below the
/// horizon by this much" cue.
pub(super) fn paint_alt_zero_line(
    ctx: &mut dyn DrawCtx,
    w: f64,
    h: f64,
    rot: &nalgebra::Matrix3<f64>,
    center: Point,
    focal_length: f64,
) {
    ctx.set_stroke_color(Color::from_rgba8(255, 200, 140, 70));
    ctx.set_line_width(1.0);

    // Skip everything below the painted ground band (paint_horizon_strip
    // covers y=0..36) and above the top edge, so the line doesn't
    // extend into UI surfaces.
    let ground_h = 36.0;
    let clip_y = |y: f64| -> bool { y >= ground_h && y <= h - 6.0 };

    let mut prev: Option<Point> = None;
    let mut prev_in_front = false;
    let step = (2.0_f64).to_radians();
    let mut az = 0.0_f64;
    while az <= 2.0 * std::f64::consts::PI + step {
        let hc = HorizontalCoords { alt: 0.0, az };
        let v_cart = horizontal_to_cartesian(hc);
        let v_rot = rot * v_cart;
        let (x, y, z) = (v_rot.x, v_rot.y, v_rot.z);

        let in_front = z > 0.02;
        let proj = if in_front {
            Some(Point::new(
                center.x + (x / z) * focal_length,
                center.y + (y / z) * focal_length,
            ))
        } else {
            None
        };

        if let (Some(p), Some(q), true, true) = (prev, proj, prev_in_front, in_front) {
            // Skip absurdly long segments that imply we crossed behind
            // the camera between samples even though both samples
            // claim z > 0 (large jump in screen space).
            let dx = q.x - p.x;
            let dy = q.y - p.y;
            let len = (dx * dx + dy * dy).sqrt();
            let in_view = (clip_y(p.y) || clip_y(q.y))
                && p.x > -32.0
                && p.x < w + 32.0
                && q.x > -32.0
                && q.x < w + 32.0;
            if len < w.max(h) && in_view {
                ctx.begin_path();
                ctx.move_to(p.x, p.y);
                ctx.line_to(q.x, q.y);
                ctx.stroke();
            }
        }
        prev = proj;
        prev_in_front = in_front;
        az += step;
    }
}

/// Paint a tapped-body info card anchored near `target`. Card stays
/// inside `viewport` — flips to the other side of the body if it
/// would otherwise clip the right / top edges.
pub(super) fn paint_info_card(
    ctx: &mut dyn DrawCtx,
    font: Arc<Font>,
    target: Point,
    viewport: Rect,
    sel: &Selection,
) {
    let mut lines: Vec<String> = Vec::with_capacity(3);
    lines.push(sel.name.clone());
    lines.push(format!("magnitude {:+.2}", sel.magnitude));
    if let Some(detail) = &sel.detail {
        lines.push(detail.clone());
    }

    let title_size = 14.0_f64;
    let body_size = 11.0_f64;
    let pad = 10.0_f64;
    let line_gap = 4.0_f64;

    let approx_width = |text: &str, size: f64| (text.chars().count() as f64) * size * 0.55;
    let mut card_w = lines
        .iter()
        .enumerate()
        .map(|(i, l)| approx_width(l, if i == 0 { title_size } else { body_size }))
        .fold(0.0_f64, f64::max)
        + pad * 2.0;
    card_w = card_w.clamp(160.0, viewport.width - 24.0);
    let card_h = title_size
        + (lines.len() - 1) as f64 * (body_size + line_gap)
        + line_gap
        + pad * 2.0;

    let anchor_dx = 14.0_f64;
    let anchor_dy = 14.0_f64;
    let mut x = target.x + anchor_dx;
    let mut y = target.y + anchor_dy;
    if x + card_w > viewport.width - 8.0 {
        x = target.x - card_w - anchor_dx;
    }
    if y + card_h > viewport.height - 8.0 {
        y = target.y - card_h - anchor_dy;
    }
    x = x.clamp(8.0, viewport.width - card_w - 8.0);
    y = y.clamp(8.0, viewport.height - card_h - 8.0);

    ctx.set_fill_color(Color::from_rgba8(15, 20, 38, 230));
    ctx.begin_path();
    ctx.rounded_rect(x, y, card_w, card_h, 8.0);
    ctx.fill();
    ctx.set_stroke_color(Color::from_rgba8(255, 215, 90, 200));
    ctx.set_line_width(1.5);
    ctx.begin_path();
    ctx.rounded_rect(x, y, card_w, card_h, 8.0);
    ctx.stroke();

    ctx.set_stroke_color(Color::from_rgba8(255, 215, 90, 180));
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(target.x, target.y);
    let cx = x + card_w / 2.0;
    let cy = y + card_h / 2.0;
    let edge_x = if target.x < x {
        x
    } else if target.x > x + card_w {
        x + card_w
    } else {
        cx
    };
    let edge_y = if target.y < y {
        y
    } else if target.y > y + card_h {
        y + card_h
    } else {
        cy
    };
    ctx.line_to(edge_x, edge_y);
    ctx.stroke();

    ctx.set_font(font);
    let title_baseline = y + card_h - pad - title_size * 0.85;
    ctx.set_fill_color(Color::from_rgb8(255, 235, 150));
    ctx.set_font_size(title_size);
    ctx.fill_text(&lines[0], x + pad, title_baseline);

    ctx.set_fill_color(Color::from_rgb8(220, 222, 240));
    ctx.set_font_size(body_size);
    for (i, line) in lines.iter().enumerate().skip(1) {
        let baseline = title_baseline
            - title_size * 0.15
            - line_gap
            - i as f64 * (body_size + line_gap);
        ctx.fill_text(line, x + pad, baseline);
    }
}
