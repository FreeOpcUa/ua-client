use cursive::Printer;
use cursive::direction::Direction;
use cursive::event::{Event, EventResult};
use cursive::style::{BaseColor, Color, ColorStyle};
use cursive::view::{CannotFocus, View, ViewWrapper};

pub struct FocusGate<V> {
    inner: V,
    enabled: bool,
}

impl<V> FocusGate<V> {
    pub fn new(inner: V) -> Self {
        Self {
            inner,
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl<V: View> ViewWrapper for FocusGate<V> {
    cursive::wrap_impl!(self.inner: V);

    fn wrap_take_focus(&mut self, source: Direction) -> Result<EventResult, CannotFocus> {
        if self.enabled {
            self.inner.take_focus(source)
        } else {
            Err(CannotFocus)
        }
    }

    fn wrap_on_event(&mut self, event: Event) -> EventResult {
        if self.enabled {
            self.inner.on_event(event)
        } else {
            EventResult::Ignored
        }
    }

    fn wrap_draw(&self, printer: &Printer) {
        if self.enabled {
            self.inner.draw(printer);
        } else {
            let dimmed = ColorStyle::front(Color::Light(BaseColor::Black));
            printer.with_color(dimmed, |p| self.inner.draw(p));
        }
    }
}
