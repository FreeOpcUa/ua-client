use cursive::event::{Event, EventResult};
use cursive::style::{ColorStyle, Effect, PaletteStyle};
use cursive::view::{View, ViewWrapper};
use cursive::{Printer, Rect, Vec2};

pub struct FocusFrame<V> {
    inner: V,
    title: String,
}

impl<V> FocusFrame<V> {
    pub fn new(inner: V, title: impl Into<String>) -> Self {
        Self {
            inner,
            title: title.into(),
        }
    }
}

const TITLE_SPACING: usize = 3;

impl<V: View> ViewWrapper for FocusFrame<V> {
    cursive::wrap_impl!(self.inner: V);

    fn wrap_required_size(&mut self, req: Vec2) -> Vec2 {
        let inner_req = req.saturating_sub(Vec2::new(2, 2));
        let inner = self.inner.required_size(inner_req);
        let size = inner + Vec2::new(2, 2);
        let title_w = self.title.chars().count() + 2 * TITLE_SPACING;
        size.or_max(Vec2::new(title_w, 0))
    }

    fn wrap_on_event(&mut self, event: Event) -> EventResult {
        self.inner.on_event(event.relativized((1, 1)))
    }

    fn wrap_layout(&mut self, size: Vec2) {
        self.inner.layout(size.saturating_sub(Vec2::new(2, 2)));
    }

    fn wrap_draw(&self, printer: &Printer) {
        let focused = printer.focused;
        let w = printer.size.x;
        let h = printer.size.y;
        if w < 2 || h < 2 {
            return;
        }
        let (tl, tr, bl, br, hor, ver) = if focused {
            ("┏", "┓", "┗", "┛", "━", "┃")
        } else {
            ("┌", "┐", "└", "┘", "─", "│")
        };
        let paint = |p: &Printer| {
            p.print((0, 0), tl);
            p.print((w - 1, 0), tr);
            p.print((0, h - 1), bl);
            p.print((w - 1, h - 1), br);
            p.print_hline((1, 0), w - 2, hor);
            p.print_hline((1, h - 1), w - 2, hor);
            p.print_vline((0, 1), h - 2, ver);
            p.print_vline((w - 1, 1), h - 2, ver);
        };
        if focused {
            printer.with_color(ColorStyle::title_primary(), |p| {
                p.with_effect(Effect::Bold, paint);
            });
        } else {
            paint(printer);
        }

        if !self.title.is_empty() && w >= TITLE_SPACING * 2 + 2 {
            let label = format!(" {} ", self.title);
            let label_w = label.chars().count();
            let max_w = w.saturating_sub(TITLE_SPACING * 2);
            if label_w <= max_w {
                let x = TITLE_SPACING + (max_w - label_w) / 2;
                if focused {
                    printer.with_style(PaletteStyle::Highlight, |p| {
                        p.with_effect(Effect::Bold, |q| q.print((x, 0), &label));
                    });
                } else {
                    printer.print((x, 0), &label);
                }
            }
        }

        let inner_printer = printer.offset((1, 1)).shrinked((1, 1));
        self.inner.draw(&inner_printer);
    }

    fn wrap_important_area(&self, size: Vec2) -> Rect {
        let inner_size = size.saturating_sub(Vec2::new(2, 2));
        self.inner.important_area(inner_size) + Vec2::new(1, 1)
    }
}
