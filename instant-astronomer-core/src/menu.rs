//! Search + three-dot "more options" controls for the mobile left rail.
//!
//! These used to live in a slide-in top bar; that bar was removed in favour
//! of folding the two buttons into the existing left-edge button rail —
//! `search` at the top, the three-dot `menu` at the bottom. The three-dot
//! button toggles a small flyout panel that pops out to the **right** of the
//! rail, bottom-aligned with the kebab button, so opening it never resizes
//! or shifts the rail itself.
//!
//! Mobile-only: desktop keeps every control in the bottom tray.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::geometry::Size;
use agg_gui::layout_props::{Insets, VAnchor};
use agg_gui::text::Font;
use agg_gui::widget::Widget;
use agg_gui::widgets::{Button, Conditional, FlexColumn};

use crate::control_panel::ICON_BUTTON_PX;
use crate::icons::{FA_ELLIPSIS_V, FA_SEARCH};

/// Same see-through black backing as the rail and altitude HUD so every
/// overlay reads with identical transparency over the sky.
fn overlay_bg() -> Color {
    Color::from_rgba8(0, 0, 0, 110)
}

/// The three pieces the control panel slots into the left rail.
pub(crate) struct RailMenu {
    /// Magnifying-glass button — goes at the top of the rail.
    pub search_button: Box<dyn Widget>,
    /// Three-dot button — goes at the bottom of the rail. Toggles `flyout`.
    pub menu_button: Box<dyn Widget>,
    /// The "more options" panel. Add it as the second child of a
    /// content-fit [`FlexRow`](agg_gui::widgets::FlexRow) after the rail
    /// column so it floats to the rail's right, bottom-aligned with the
    /// three-dot button. Collapses to zero size while closed.
    pub flyout: Box<dyn Widget>,
}

/// Build the rail's search button, three-dot button, and flyout panel.
///
/// * `panel_items` — the lesser-used controls parked in the flyout (e.g.
///   "Locate me", "Calibrate"), stacked top-to-bottom in the order given.
pub(crate) fn build_rail_menu(
    font: Arc<Font>,
    icon_font: Arc<Font>,
    panel_items: Vec<Box<dyn Widget>>,
    search_active: Rc<Cell<bool>>,
    search_query: Rc<RefCell<String>>,
) -> RailMenu {
    build_rail_menu_parts(font, icon_font, panel_items, search_active, search_query).0
}

/// Like [`build_rail_menu`] but also returns the internal `menu_open` cell
/// so tests can drive the flyout open/closed.
fn build_rail_menu_parts(
    font: Arc<Font>,
    icon_font: Arc<Font>,
    panel_items: Vec<Box<dyn Widget>>,
    search_active: Rc<Cell<bool>>,
    search_query: Rc<RefCell<String>>,
) -> (RailMenu, Rc<Cell<bool>>) {
    // Whether the three-dot flyout is expanded.
    let menu_open = Rc::new(Cell::new(false));

    // Search — opens the object-search overlay. Lights up while active.
    let search_button: Box<dyn Widget> = {
        let active = Rc::clone(&search_active);
        let click = Rc::clone(&search_active);
        let query = Rc::clone(&search_query);
        Box::new(
            Button::new("", Arc::clone(&font))
                .with_icon(FA_SEARCH, Arc::clone(&icon_font))
                .with_subtle()
                .with_active_fn(move || active.get())
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX))
                .on_click(move || {
                    query.borrow_mut().clear();
                    click.set(true);
                    agg_gui::focus::request_focus(crate::search_panel::SEARCH_FIELD_FOCUS_ID);
                    agg_gui::animation::request_draw();
                }),
        )
    };

    // Three-dot "more options" toggle. Accent/blue while the flyout is open.
    let menu_button: Box<dyn Widget> = {
        let active = Rc::clone(&menu_open);
        let click = Rc::clone(&menu_open);
        Box::new(
            Button::new("", Arc::clone(&font))
                .with_icon(FA_ELLIPSIS_V, Arc::clone(&icon_font))
                .with_subtle()
                .with_active_fn(move || active.get())
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX))
                .on_click(move || {
                    click.set(!click.get());
                    agg_gui::animation::request_draw();
                }),
        )
    };

    // The flyout card: a narrow column of the parked controls.
    let mut panel_card = FlexColumn::new()
        .with_gap(8.0)
        .with_inner_padding(Insets::all(6.0))
        .with_background(overlay_bg())
        .with_fit_width(true);
    for item in panel_items {
        panel_card = panel_card.add(item);
    }
    // Bottom-anchored so, sitting in the rail's FlexRow next to the taller
    // button column, its bottom edge lines up with the kebab button.
    let flyout: Box<dyn Widget> = Box::new(
        Conditional::new(Rc::clone(&menu_open), Box::new(panel_card))
            .with_v_anchor(VAnchor::BOTTOM),
    );

    (
        RailMenu {
            search_button,
            menu_button,
            flyout,
        },
        menu_open,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::layout_props::HAnchor;
    use agg_gui::widgets::{FlexRow, Spacer, Stack};

    fn icon_button() -> Box<dyn Widget> {
        let font = crate::load_default_font();
        let icon = crate::icons::load_icon_font();
        Box::new(
            Button::new("", font)
                .with_icon(FA_SEARCH, icon)
                .with_compact()
                .with_min_size(Size::new(ICON_BUTTON_PX, ICON_BUTTON_PX)),
        )
    }

    /// Reproduce the real rail: `Conditional(show_controls)` wrapping a
    /// content-fit `FlexRow` of a (tall) button column + the flyout,
    /// floated by a `Stack` `add_aligned`. When the kebab opens the flyout
    /// it must resolve to a non-zero, on-screen rect.
    #[test]
    fn flyout_becomes_visible_when_opened() {
        let font = crate::load_default_font();
        let icon = crate::icons::load_icon_font();
        let items: Vec<Box<dyn Widget>> = vec![icon_button(), icon_button()];
        let (menu, menu_open) = build_rail_menu_parts(
            font,
            icon,
            items,
            Rc::new(Cell::new(false)),
            Rc::new(RefCell::new(String::new())),
        );

        // Real-ish button column taller than the flyout.
        let column = FlexColumn::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .with_inner_padding(Insets::all(6.0))
            .add(icon_button())
            .add(icon_button())
            .add(icon_button())
            .add(icon_button())
            .add(icon_button());
        let rail_row = FlexRow::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .add(Box::new(column))
            .add(menu.flyout);
        let show = Rc::new(Cell::new(true));
        let rail = Conditional::new(show, Box::new(rail_row))
            .with_h_anchor(HAnchor::LEFT)
            .with_v_anchor(VAnchor::BOTTOM);

        let viewport = Size::new(400.0, 800.0);
        let mut stack = Stack::new()
            .add(Box::new(Spacer::new()))
            .add_aligned(Box::new(rail));

        menu_open.set(true);
        stack.layout(viewport);

        // stack[1] = rail Conditional → [0] = FlexRow → [1] = flyout Conditional.
        let rail_w = &stack.children()[1];
        let row = &rail_w.children()[0];
        let flyout = &row.children()[1];
        let b = flyout.bounds();
        assert!(
            b.width > 1.0 && b.height > 1.0,
            "flyout should have real size when opened, got {b:?}"
        );
        // Flyout sits to the right of the column, on-screen.
        let abs_x = rail_w.bounds().x + row.bounds().x + b.x;
        assert!(
            abs_x >= 0.0 && abs_x < viewport.width,
            "flyout should be on-screen horizontally, abs_x={abs_x}"
        );
    }

    /// End-to-end: build the real rail inside an `App`, dispatch an actual
    /// tap on the kebab button, and confirm the tap flips `menu_open` and
    /// the flyout then lays out visibly. Exercises event routing + state +
    /// relayout together — the parts a pure layout test can't reach.
    #[test]
    fn tapping_kebab_opens_flyout_end_to_end() {
        use agg_gui::event::{Modifiers, MouseButton};
        use agg_gui::App;

        let font = crate::load_default_font();
        let icon = crate::icons::load_icon_font();
        let items: Vec<Box<dyn Widget>> = vec![icon_button(), icon_button()];
        let (menu, menu_open) = build_rail_menu_parts(
            font,
            icon,
            items,
            Rc::new(Cell::new(false)),
            Rc::new(RefCell::new(String::new())),
        );

        // Mirror control_panel: search at the top, kebab at the bottom.
        let column = FlexColumn::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .with_inner_padding(Insets::all(6.0))
            .add(menu.search_button)
            .add(icon_button())
            .add(icon_button())
            .add(icon_button())
            .add(menu.menu_button);
        let rail_row = FlexRow::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .add(Box::new(column))
            .add(menu.flyout);
        let show = Rc::new(Cell::new(true));
        let rail = Conditional::new(show, Box::new(rail_row))
            .with_h_anchor(HAnchor::LEFT)
            .with_v_anchor(VAnchor::CENTER);
        let stack = Stack::new()
            .add(Box::new(Spacer::new()))
            .add_aligned(Box::new(rail));

        let w = 390.0;
        let h = 844.0;
        let mut app = App::new(Box::new(stack));
        app.layout(Size::new(w, h));

        // Absolute (Y-up) centre of the kebab = sum of bounds offsets down
        // stack[1]=rail → [0]=row → [0]=column → last=kebab.
        let root = app.root();
        let rail_w = &root.children()[1];
        let row = &rail_w.children()[0];
        let col = &row.children()[0];
        let kebab = col.children().last().expect("kebab present");
        let kb = kebab.bounds();
        let abs_x = rail_w.bounds().x + row.bounds().x + col.bounds().x + kb.x + kb.width / 2.0;
        let abs_y_up = rail_w.bounds().y + row.bounds().y + col.bounds().y + kb.y + kb.height / 2.0;
        assert!(kb.width > 1.0 && kb.height > 1.0, "kebab has size: {kb:?}");

        // A touchscreen tap is a bare press→release at one point with NO
        // preceding MouseMove (touch has no hover phase). The kebab must
        // still open the flyout. App input is Y-down screen space, so flip
        // the Y-up centre back.
        let screen_y = h - abs_y_up;
        app.on_mouse_down(abs_x, screen_y, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(abs_x, screen_y, MouseButton::Left, Modifiers::default());

        assert!(
            menu_open.get(),
            "a hover-less tap on the kebab should toggle menu_open on"
        );

        // Relayout (as the render loop does) and confirm the flyout is real.
        app.layout(Size::new(w, h));
        let root = app.root();
        let rail_w = &root.children()[1];
        let row = &rail_w.children()[0];
        let flyout = &row.children()[1];
        let fb = flyout.bounds();
        assert!(
            fb.width > 1.0 && fb.height > 1.0,
            "flyout should be sized after the tap, got {fb:?}"
        );
    }

    /// Pixel-level: after a real tap on the kebab, the flyout must actually
    /// paint visible (non-black) pixels in its on-screen region. Guards
    /// against the flyout laying out correctly but being clipped / hidden /
    /// painted off-screen — the symptom a layout-only test can't catch.
    #[test]
    fn flyout_paints_pixels_after_tap() {
        use agg_gui::event::{Modifiers, MouseButton};
        use agg_gui::{App, Color, Framebuffer, GfxCtx};

        let font = crate::load_default_font();
        let icon = crate::icons::load_icon_font();
        let items: Vec<Box<dyn Widget>> = vec![icon_button(), icon_button()];
        let (menu, menu_open) = build_rail_menu_parts(
            font,
            icon,
            items,
            Rc::new(Cell::new(false)),
            Rc::new(RefCell::new(String::new())),
        );

        let column = FlexColumn::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .with_inner_padding(Insets::all(6.0))
            .add(menu.search_button)
            .add(icon_button())
            .add(icon_button())
            .add(icon_button())
            .add(menu.menu_button);
        let rail_row = FlexRow::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .add(Box::new(column))
            .add(menu.flyout);
        let show = Rc::new(Cell::new(true));
        let rail = Conditional::new(show, Box::new(rail_row))
            .with_h_anchor(HAnchor::LEFT)
            .with_v_anchor(VAnchor::CENTER);
        let stack = Stack::new()
            .add(Box::new(Spacer::new()))
            .add_aligned(Box::new(rail));

        let w = 390usize;
        let h = 844usize;
        let mut app = App::new(Box::new(stack));
        app.layout(Size::new(w as f64, h as f64));

        // Locate + tap the kebab (bare press/release, no hover).
        let (abs_x, abs_y_up) = {
            let root = app.root();
            let rail_w = &root.children()[1];
            let row = &rail_w.children()[0];
            let col = &row.children()[0];
            let kb = col.children().last().unwrap().bounds();
            (
                rail_w.bounds().x + row.bounds().x + col.bounds().x + kb.x + kb.width / 2.0,
                rail_w.bounds().y + row.bounds().y + col.bounds().y + kb.y + kb.height / 2.0,
            )
        };
        let screen_y = h as f64 - abs_y_up;
        app.on_mouse_down(abs_x, screen_y, MouseButton::Left, Modifiers::default());
        app.on_mouse_up(abs_x, screen_y, MouseButton::Left, Modifiers::default());
        assert!(menu_open.get(), "kebab tap must open the menu");
        app.layout(Size::new(w as f64, h as f64));

        // Absolute Y-up rect of the flyout.
        let (fx, fy, fw, fh) = {
            let root = app.root();
            let rail_w = &root.children()[1];
            let row = &rail_w.children()[0];
            let flyout = &row.children()[1];
            let fb = flyout.bounds();
            (
                (rail_w.bounds().x + row.bounds().x + fb.x) as usize,
                (rail_w.bounds().y + row.bounds().y + fb.y) as usize,
                fb.width as usize,
                fb.height as usize,
            )
        };

        let mut fb = Framebuffer::new(w as u32, h as u32);
        {
            let mut ctx = GfxCtx::new(&mut fb);
            ctx.clear(Color::black());
            app.paint(&mut ctx);
        }

        // Count non-black pixels inside the flyout rect (Y-up framebuffer).
        let px = fb.pixels();
        let mut lit = 0usize;
        for y in fy..(fy + fh).min(h) {
            for x in fx..(fx + fw).min(w) {
                let i = (y * w + x) * 4;
                if px[i] > 30 || px[i + 1] > 30 || px[i + 2] > 30 {
                    lit += 1;
                }
            }
        }
        assert!(
            lit > 20,
            "flyout region ({fx},{fy},{fw},{fh}) should paint visible pixels, only {lit} lit"
        );
    }

    /// Opening the flyout must not change the rail row's height — the
    /// button column is taller than the flyout, so the row height is pinned
    /// to the column and the centred rail stays put. Guards the "controls
    /// shift when the menu opens" regression that killed the old top bar.
    #[test]
    fn opening_flyout_does_not_change_row_height() {
        let font = crate::load_default_font();
        let icon = crate::icons::load_icon_font();
        let items: Vec<Box<dyn Widget>> =
            vec![Box::new(Spacer::new().with_max_size(Size::new(32.0, 40.0)))];
        let (menu, menu_open) = build_rail_menu_parts(
            font,
            icon,
            items,
            Rc::new(Cell::new(false)),
            Rc::new(RefCell::new(String::new())),
        );

        // A stand-in button column that's taller than the flyout.
        let tall_column = Spacer::new().with_max_size(Size::new(32.0, 300.0));
        let mut row = FlexRow::new()
            .with_gap(8.0)
            .with_fit_width(true)
            .add(Box::new(tall_column))
            .add(menu.flyout);

        let avail = Size::new(400.0, 800.0);

        menu_open.set(false);
        let closed = row.layout(avail);

        menu_open.set(true);
        let open = row.layout(avail);

        assert!(
            (closed.height - open.height).abs() < 0.5,
            "row height changed with the flyout: closed={}, open={}",
            closed.height,
            open.height
        );
        assert!(
            open.width > closed.width,
            "flyout should widen the row when opened: closed={}, open={}",
            closed.width,
            open.width
        );
    }
}
