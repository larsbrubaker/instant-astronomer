//! Top-of-screen object-search overlay.
//!
//! Built once and floated over the sky as a top-centre `Stack` overlay
//! (see `lib.rs`). It has two states, driven by whether a target is
//! selected:
//!
//! * **Input** — a text field plus a live filtered results list
//!   ([`SearchResultsWidget`]). Typing updates `search_query`; tapping a
//!   row commits a [`SearchTarget`].
//! * **Locked** — a wrapping "Looking for: <name>" banner with a Close
//!   button to leave search.
//!
//! All UI renders through agg-gui per the project's "no HTML/CSS chrome"
//! rule. The results list is a custom `DrawCtx` widget because agg-gui's
//! `Button` labels are fixed at build time and can't restyle per
//! keystroke.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use agg_gui::color::Color;
use agg_gui::draw_ctx::DrawCtx;
use agg_gui::event::{Event, EventResult, MouseButton};
use agg_gui::geometry::{Point, Rect, Size};
use agg_gui::layout_props::{HAnchor, Insets, VAnchor};
use agg_gui::text::{measure_advance, Font};
use agg_gui::widget::Widget;
use agg_gui::widgets::{Button, Conditional, Container, FlexColumn, FlexRow, TextField};

use crate::icons::{load_icon_font, FA_TIMES};
use crate::search::{search_objects, SearchTarget};

/// Maximum panel width so the bar doesn't stretch edge-to-edge on a wide
/// desktop window; on a narrow phone it shrinks to fit.
const PANEL_MAX_W: f64 = 460.0;

/// Stable id for the search text field on agg-gui's programmatic focus
/// channel. The entry points that open the overlay call
/// [`agg_gui::focus::request_focus`] with this so the field is focused (and
/// the on-screen keyboard rises on mobile) the moment search opens.
pub(crate) const SEARCH_FIELD_FOCUS_ID: agg_gui::focus::FocusId = 0x5EA8_C4F1;

/// Edge length (logical px) of the small icon buttons in the overlay.
const SMALL_ICON_PX: f64 = 36.0;

/// Height (logical px) shared by the search text field and overlay close
/// button so the input row reads as one vertically-centred control group.
const SEARCH_INPUT_H: f64 = 36.0;

/// Shared cells the search overlay reads and writes.
#[derive(Clone)]
pub(crate) struct SearchState {
    /// Whether the overlay is shown at all.
    pub active: Rc<Cell<bool>>,
    /// `true` once a target is committed (locked mode). Mirrors `target`.
    pub has_target: Rc<Cell<bool>>,
    /// Always the inverse of `has_target`; drives the input-mode
    /// `Conditional` (which only takes a single bool cell).
    pub no_target: Rc<Cell<bool>>,
    /// Live text-field contents.
    pub query: Rc<RefCell<String>>,
    /// The committed target the sky-view finder points at.
    pub target: Rc<RefCell<Option<SearchTarget>>>,
    /// Projection clock — needed to place Solar System bodies for matching.
    pub timestamp_ms: Rc<Cell<i64>>,
}

impl SearchState {
    /// Commit a chosen target (enter locked mode).
    fn select(&self, target: SearchTarget) {
        *self.target.borrow_mut() = Some(target);
        self.has_target.set(true);
        self.no_target.set(false);
        self.query.borrow_mut().clear();
        agg_gui::animation::request_draw();
    }

    /// Drop the current target (back to input mode), keeping the overlay
    /// open so the user can search again.
    fn clear_target(&self) {
        *self.target.borrow_mut() = None;
        self.has_target.set(false);
        self.no_target.set(true);
        self.query.borrow_mut().clear();
        agg_gui::animation::request_draw();
    }

    /// Close the overlay entirely and reset all search state.
    fn close(&self) {
        self.active.set(false);
        self.clear_target();
    }
}

/// Build the search overlay widget to add to the sky `Stack` via
/// `add_aligned`. Collapses to nothing while `active` is false.
pub(crate) fn build_search_panel(font: Arc<Font>, state: SearchState) -> Box<dyn Widget> {
    let icon_font = load_icon_font();

    let text_field = {
        let query = Rc::clone(&state.query);
        TextField::new(Arc::clone(&font))
            .with_placeholder("Search stars, planets, constellations...")
            .with_text_cell(Rc::clone(&query))
            .with_focus_id(SEARCH_FIELD_FOCUS_ID)
            .with_padding(6.0)
            .with_min_size(Size::new(0.0, SEARCH_INPUT_H))
            .on_change(move |s| {
                *query.borrow_mut() = s.to_string();
                agg_gui::animation::request_draw();
            })
    };

    let close_input = {
        let state = state.clone();
        icon_button(&font, &icon_font, FA_TIMES, move || state.close())
    };
    let input_row = FlexRow::new()
        .with_gap(8.0)
        .add_flex(Box::new(text_field), 1.0)
        .add(close_input);

    let results = SearchResultsWidget::new(Arc::clone(&font), state.clone());

    let input_col = FlexColumn::new()
        .with_gap(8.0)
        .add(Box::new(input_row))
        .add(Box::new(results));
    let input_mode = Conditional::new(Rc::clone(&state.no_target), Box::new(input_col));

    let banner = LookingForText::new(Arc::clone(&font), state.clone());
    let close_locked = {
        let state = state.clone();
        icon_button(&font, &icon_font, FA_TIMES, move || state.close())
    };
    let locked_row = FlexRow::new()
        .with_gap(8.0)
        .add_flex(Box::new(banner), 1.0)
        .add(close_locked);
    let locked_mode = Conditional::new(Rc::clone(&state.has_target), Box::new(locked_row));

    // Only one of the two modes is ever visible, so use no gap between
    // them — otherwise `FlexColumn` reserves `gap` px above the visible
    // mode for the collapsed one, showing up as dead margin at the top.
    let inner = FlexColumn::new()
        .add(Box::new(input_mode))
        .add(Box::new(locked_mode));

    let panel = Container::new()
        .add(Box::new(inner))
        .with_fit_height(true)
        .with_background(Color::from_rgba8(12, 16, 30, 235))
        .with_border(Color::from_rgb8(80, 110, 155), 2.0)
        .with_corner_radius(10.0)
        .with_inner_padding(Insets {
            top: 6.0,
            bottom: 12.0,
            left: 10.0,
            right: 10.0,
        });

    let gated = Conditional::new(Rc::clone(&state.active), Box::new(panel));

    // Transparent positioner: caps the width and anchors the panel
    // top-centre. A `FlexColumn` (not a fit-height `Container`) is used on
    // purpose — it reports its *natural* content height and re-lays its
    // child at exactly that height, mirroring the control-panel rail which
    // renders correctly on mobile. A fit-height `Container` here clipped
    // the panel's bottom edge on HiDPI / ux-scaled (mobile) viewports.
    // The visibility `Conditional` lives inside so the positioner collapses
    // to nothing (no stray padded box) when search is closed.
    Box::new(
        FlexColumn::new()
            .add(Box::new(gated))
            .with_max_size(Size::new(PANEL_MAX_W, f64::INFINITY))
            .with_margin(Insets {
                top: 6.0,
                bottom: 0.0,
                left: 10.0,
                right: 10.0,
            })
            .with_h_anchor(HAnchor::CENTER)
            .with_v_anchor(VAnchor::TOP),
    )
}

/// A small, compact, label-less icon button for the overlay chrome.
fn icon_button(
    font: &Arc<Font>,
    icon_font: &Arc<Font>,
    icon: char,
    on_click: impl Fn() + 'static,
) -> Box<dyn Widget> {
    Box::new(
        Button::new("", Arc::clone(font))
            .with_icon(icon, Arc::clone(icon_font))
            .with_compact()
            .with_min_size(Size::new(SMALL_ICON_PX, SMALL_ICON_PX))
            .on_click(on_click),
    )
}

/// Wrapping locked-mode label. `StatusText` is intentionally single-line, but
/// target names need to wrap before they collide with the close button.
/// Tapping the banner drops the target and returns to the search input.
struct LookingForText {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    state: SearchState,
    lines: Vec<String>,
    /// Whether a press landed inside us (so the release counts as a tap).
    pressed: bool,
}

impl LookingForText {
    fn new(font: Arc<Font>, state: SearchState) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            state,
            lines: Vec::new(),
            pressed: false,
        }
    }

    fn text(&self) -> String {
        match self.state.target.borrow().as_ref() {
            Some(t) => format!("Looking for: {}", t.name),
            None => String::new(),
        }
    }

    fn hit(&self, pos: Point) -> bool {
        pos.x >= 0.0 && pos.x <= self.bounds.width && pos.y >= 0.0 && pos.y <= self.bounds.height
    }

    fn wrap_text(&self, text: &str, max_w: f64, font_size: f64) -> Vec<String> {
        if text.is_empty() {
            return Vec::new();
        }

        let max_w = max_w.max(1.0);
        let mut lines = Vec::new();
        let mut line = String::new();

        for word in text.split_whitespace() {
            let candidate = if line.is_empty() {
                word.to_string()
            } else {
                format!("{line} {word}")
            };

            if measure_advance(&self.font, &candidate, font_size) <= max_w {
                line = candidate;
                continue;
            }

            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }

            if measure_advance(&self.font, word, font_size) <= max_w {
                line = word.to_string();
                continue;
            }

            // Last resort for unusually long names: wrap within the word so
            // the close button always keeps its own column.
            for ch in word.chars() {
                let candidate = format!("{line}{ch}");
                if !line.is_empty() && measure_advance(&self.font, &candidate, font_size) > max_w {
                    lines.push(std::mem::take(&mut line));
                }
                line.push(ch);
            }
        }

        if !line.is_empty() {
            lines.push(line);
        }

        lines
    }
}

impl Widget for LookingForText {
    fn type_name(&self) -> &'static str {
        "LookingForText"
    }

    fn bounds(&self) -> Rect {
        self.bounds
    }

    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }

    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let font_size = 15.0;
        self.lines = self.wrap_text(&self.text(), available.width, font_size);
        let line_h = font_size * 1.45;
        let height = (self.lines.len().max(1) as f64) * line_h;
        self.bounds = Rect::new(0.0, 0.0, available.width, height);
        Size::new(available.width, height)
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let font_size = 15.0;
        let line_h = font_size * 1.45;
        let h = self.bounds.height;

        ctx.set_font(Arc::clone(&self.font));
        ctx.set_font_size(font_size);
        ctx.set_fill_color(Color::from_rgb8(150, 255, 200));

        // Centre the glyph block within each line slot. `fill_text` places the
        // baseline at `y`; glyphs extend up by `ascent` and down by `descent`,
        // so centring the [-descent, +ascent] block in the slot keeps short
        // labels visually centred next to the taller close button.
        let m = ctx.measure_text("Ag").unwrap_or_default();
        let baseline_offset = line_h * 0.5 - (m.ascent - m.descent) * 0.5;

        ctx.save();
        ctx.clip_rect(0.0, 0.0, self.bounds.width, self.bounds.height);
        for (i, line) in self.lines.iter().enumerate() {
            let slot_bottom = h - line_h * (i as f64 + 1.0);
            ctx.fill_text(line, 0.0, slot_bottom + baseline_offset);
        }
        ctx.restore();
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        self.hit(local_pos)
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if self.hit(*pos) {
                    self.pressed = true;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseUp { pos, button: MouseButton::Left, .. } => {
                if !self.pressed {
                    return EventResult::Ignored;
                }
                self.pressed = false;
                if self.hit(*pos) {
                    // Tapping the banner returns to the search input and
                    // re-focuses it (raising the on-screen keyboard on mobile).
                    self.state.clear_target();
                    agg_gui::focus::request_focus(SEARCH_FIELD_FOCUS_ID);
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }
}

/// Per-row height (logical px) for the results list.
const ROW_H: f64 = 32.0;

/// Custom widget that paints the live filtered results as a clickable
/// list. Recomputes matches every layout from `state.query`; a tap on a
/// row commits that target.
struct SearchResultsWidget {
    bounds: Rect,
    children: Vec<Box<dyn Widget>>,
    font: Arc<Font>,
    state: SearchState,
    /// Matches from the most recent layout, reused by paint + hit-testing.
    matches: RefCell<Vec<SearchTarget>>,
    /// Whether a press landed inside us (so the release counts as a tap).
    pressed: bool,
}

impl SearchResultsWidget {
    fn new(font: Arc<Font>, state: SearchState) -> Self {
        Self {
            bounds: Rect::default(),
            children: Vec::new(),
            font,
            state,
            matches: RefCell::new(Vec::new()),
            pressed: false,
        }
    }

    /// Row index (0 = top) under a widget-local Y-up position, if any.
    fn row_at(&self, pos: Point) -> Option<usize> {
        let h = self.bounds.height;
        if pos.x < 0.0 || pos.x > self.bounds.width || pos.y < 0.0 || pos.y > h {
            return None;
        }
        // Y-up: row 0 is at the top (highest Y).
        let idx = ((h - pos.y) / ROW_H).floor() as i64;
        if idx < 0 {
            return None;
        }
        let idx = idx as usize;
        if idx < self.matches.borrow().len() {
            Some(idx)
        } else {
            None
        }
    }
}

impl Widget for SearchResultsWidget {
    fn type_name(&self) -> &'static str {
        "SearchResultsWidget"
    }
    fn bounds(&self) -> Rect {
        self.bounds
    }
    fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
    }
    fn children(&self) -> &[Box<dyn Widget>] {
        &self.children
    }
    fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget>> {
        &mut self.children
    }

    fn layout(&mut self, available: Size) -> Size {
        let query = self.state.query.borrow().clone();
        let now = self.state.timestamp_ms.get();
        let matches = search_objects(&query, now);
        let n = matches.len();
        *self.matches.borrow_mut() = matches;
        let height = n as f64 * ROW_H;
        self.bounds = Rect::new(0.0, 0.0, available.width, height);
        Size::new(available.width, height)
    }

    fn hit_test(&self, local_pos: Point) -> bool {
        local_pos.x >= 0.0
            && local_pos.x <= self.bounds.width
            && local_pos.y >= 0.0
            && local_pos.y <= self.bounds.height
    }

    fn on_event(&mut self, event: &Event) -> EventResult {
        match event {
            Event::MouseDown { pos, button: MouseButton::Left, .. } => {
                if self.row_at(*pos).is_some() {
                    self.pressed = true;
                    return EventResult::Consumed;
                }
                EventResult::Ignored
            }
            Event::MouseUp { pos, button: MouseButton::Left, .. } => {
                if !self.pressed {
                    return EventResult::Ignored;
                }
                self.pressed = false;
                if let Some(idx) = self.row_at(*pos) {
                    if let Some(target) = self.matches.borrow().get(idx).cloned() {
                        self.state.select(target);
                        return EventResult::Consumed;
                    }
                }
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn paint(&mut self, ctx: &mut dyn DrawCtx) {
        let w = self.bounds.width;
        let h = self.bounds.height;
        let matches = self.matches.borrow();
        ctx.set_font(Arc::clone(&self.font));

        for (i, target) in matches.iter().enumerate() {
            // Y-up: row 0 at the top.
            let row_bottom = h - (i as f64 + 1.0) * ROW_H;

            // Subtle alternating row background for legibility over the sky.
            if i % 2 == 0 {
                ctx.set_fill_color(Color::from_rgba8(255, 255, 255, 10));
                ctx.begin_path();
                ctx.rect(0.0, row_bottom, w, ROW_H);
                ctx.fill();
            }

            let baseline = row_bottom + (ROW_H - 14.0) * 0.5;
            ctx.set_font_size(14.0);
            ctx.set_fill_color(Color::from_rgb8(235, 238, 250));
            ctx.fill_text(&target.name, 10.0, baseline);

            // Category tag, right-aligned (approx monospace advance).
            let cat = target.category;
            let cat_size = 11.0;
            let cat_w = cat.chars().count() as f64 * cat_size * 0.6;
            ctx.set_font_size(cat_size);
            ctx.set_fill_color(Color::from_rgba8(150, 170, 210, 220));
            ctx.fill_text(cat, (w - cat_w - 10.0).max(0.0), baseline + 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agg_gui::event::Modifiers;

    fn make_state(query: &str) -> SearchState {
        SearchState {
            active: Rc::new(Cell::new(true)),
            has_target: Rc::new(Cell::new(false)),
            no_target: Rc::new(Cell::new(true)),
            query: Rc::new(RefCell::new(query.to_string())),
            target: Rc::new(RefCell::new(None)),
            // J2000 epoch — deterministic body positions for matching.
            timestamp_ms: Rc::new(Cell::new(946_728_000_000)),
        }
    }

    fn tap(widget: &mut SearchResultsWidget, pos: Point) {
        widget.on_event(&Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        widget.on_event(&Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
    }

    /// Tapping the first result row commits that target and flips the
    /// panel into locked mode (has_target on, no_target off). Exercises
    /// layout → row hit-testing → state mutation together.
    #[test]
    fn tapping_a_result_row_commits_that_target() {
        let font = crate::load_default_font();
        let state = make_state("sir");
        let mut widget = SearchResultsWidget::new(font, state.clone());
        let size = widget.layout(Size::new(400.0, 600.0));
        assert!(size.height >= ROW_H, "expected at least one match for 'sir'");

        // Row 0 sits at the top (highest Y) in this Y-up widget.
        let pos = Point::new(20.0, size.height - ROW_H * 0.5);
        tap(&mut widget, pos);

        assert!(state.has_target.get(), "tap should set has_target");
        assert!(!state.no_target.get(), "tap should clear no_target");
        let target = state.target.borrow();
        assert_eq!(
            target.as_ref().map(|t| t.name.as_str()),
            Some("Sirius"),
            "first 'sir' match should be Sirius"
        );
        // Query is cleared so the list collapses behind the banner.
        assert!(state.query.borrow().is_empty());
    }

    /// Tapping the "Looking for" banner drops the target and returns the
    /// overlay to its search-input mode (no_target on, has_target off).
    #[test]
    fn tapping_looking_for_banner_returns_to_input() {
        let font = crate::load_default_font();
        let state = make_state("");
        let target = search_objects("sirius", state.timestamp_ms.get())
            .into_iter()
            .next()
            .expect("sirius should resolve to a target");
        state.select(target);
        assert!(state.has_target.get(), "precondition: locked mode");

        let mut banner = LookingForText::new(font, state.clone());
        let size = banner.layout(Size::new(300.0, 100.0));
        let pos = Point::new(10.0, size.height * 0.5);
        banner.on_event(&Event::MouseDown {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });
        banner.on_event(&Event::MouseUp {
            pos,
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
        });

        assert!(!state.has_target.get(), "tap should drop the target");
        assert!(state.no_target.get(), "tap should return to input mode");
        assert!(state.target.borrow().is_none());
    }

    /// A tap below the populated rows (empty space) selects nothing.
    #[test]
    fn tapping_empty_space_selects_nothing() {
        let font = crate::load_default_font();
        let state = make_state("sir");
        let mut widget = SearchResultsWidget::new(font, state.clone());
        widget.layout(Size::new(400.0, 600.0));
        // y = 0 is the bottom edge, below row 0 which sits at the top.
        tap(&mut widget, Point::new(20.0, -5.0));
        assert!(!state.has_target.get());
        assert!(state.target.borrow().is_none());
    }
}
