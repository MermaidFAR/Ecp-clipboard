use std::ops::Range;

use crate::theme as colors;
use gpui::{
    App, Bounds, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    FocusHandle, Focusable, GlobalElementId, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    PaintQuad, Pixels, ShapedLine, Style, TextRun, UTF16Selection, Window, div, fill, point,
    prelude::*, px, relative, rgb, rgba, size,
};

pub struct SearchInput {
    pub value: String,
    focus: FocusHandle,
    selection: Range<usize>,
    marked: Option<Range<usize>>,
    layout: Option<ShapedLine>,
    bounds: Option<Bounds<Pixels>>,
}

impl SearchInput {
    fn utf16_prefix_bytes(text: &str, offset: usize) -> usize {
        let mut units = 0;
        for (index, ch) in text.char_indices() {
            if units >= offset || units + ch.len_utf16() > offset {
                return index;
            }
            units += ch.len_utf16();
        }
        text.len()
    }

    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            value: String::new(),
            focus: cx.focus_handle(),
            selection: 0..0,
            marked: None,
            layout: None,
            bounds: None,
        }
    }

    fn utf16_to_utf8(&self, offset: usize) -> usize {
        let mut utf16 = 0;
        for (index, ch) in self.value.char_indices() {
            if utf16 >= offset {
                return index;
            }
            utf16 += ch.len_utf16();
        }
        self.value.len()
    }

    fn utf8_to_utf16(&self, offset: usize) -> usize {
        self.value[..offset].encode_utf16().count()
    }

    fn utf16_range_to_utf8(&self, range: Range<usize>) -> Range<usize> {
        self.utf16_to_utf8(range.start)..self.utf16_to_utf8(range.end)
    }

    fn to_utf16(&self, range: Range<usize>) -> Range<usize> {
        self.utf8_to_utf16(range.start)..self.utf8_to_utf16(range.end)
    }

    fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.modifiers.control {
            match event.keystroke.key.as_str() {
                "a" => {
                    self.selection = 0..self.value.len();
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
                    }
                    cx.stop_propagation();
                    return;
                }
                _ => {}
            }
        }
        match event.keystroke.key.as_str() {
            "backspace" => {
                let start = if self.selection.is_empty() {
                    self.value[..self.selection.start]
                        .char_indices()
                        .last()
                        .map_or(0, |(i, _)| i)
                } else {
                    self.selection.start
                };
                self.replace_text_in_range(
                    Some(self.to_utf16(start..self.selection.end)),
                    "",
                    window,
                    cx,
                );
            }
            "escape" => window.remove_window(),
            "left" => {
                let cursor = self.value[..self.selection.start]
                    .char_indices()
                    .last()
                    .map_or(0, |(i, _)| i);
                self.selection = cursor..cursor;
                cx.notify();
            }
            "right" => {
                let cursor = self.value[self.selection.end..]
                    .char_indices()
                    .nth(1)
                    .map_or(self.value.len(), |(i, _)| self.selection.end + i);
                self.selection = cursor..cursor;
                cx.notify();
            }
            _ => {}
        }
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if let (Some(layout), Some(bounds)) = (&self.layout, self.bounds) {
            let mut cursor = layout
                .closest_index_for_x(event.position.x - bounds.left())
                .min(self.value.len());
            while cursor > 0 && !self.value.is_char_boundary(cursor) {
                cursor -= 1;
            }
            self.selection = cursor..cursor;
            cx.notify();
        }
    }
}

impl EntityInputHandler for SearchInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let utf8 = self.utf16_range_to_utf8(range);
        *adjusted = Some(self.to_utf16(utf8.clone()));
        Some(self.value.get(utf8)?.to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.to_utf16(self.selection.clone()),
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.clone().map(|range| self.to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|range| self.utf16_range_to_utf8(range))
            .or_else(|| self.marked.clone())
            .unwrap_or(self.selection.clone());
        self.value.replace_range(range.clone(), text);
        let cursor = range.start + text.len();
        self.selection = cursor..cursor;
        self.marked = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        new_selection: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = range
            .as_ref()
            .map(|range| self.utf16_to_utf8(range.start))
            .or_else(|| self.marked.as_ref().map(|range| range.start))
            .unwrap_or(self.selection.start);
        self.replace_text_in_range(range, text, window, cx);
        self.marked = (!text.is_empty()).then_some(start..start + text.len());
        if let Some(new_selection) = new_selection {
            let marked_start = self.marked.as_ref().map_or(start, |range| range.start);
            let begin = marked_start + Self::utf16_prefix_bytes(text, new_selection.start);
            let end = marked_start + Self::utf16_prefix_bytes(text, new_selection.end);
            self.selection = begin..end;
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let range = self.utf16_range_to_utf8(range);
        Some(Bounds::from_corners(
            point(
                bounds.left() + layout.x_for_index(range.start),
                bounds.top(),
            ),
            point(
                bounds.left() + layout.x_for_index(range.end),
                bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.bounds?;
        let layout = self.layout.as_ref()?;
        let utf8 = layout.index_for_x(point.x - bounds.left())?;
        Some(self.utf8_to_utf16(utf8))
    }

    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selection = self.utf16_range_to_utf8(range);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.value.encode_utf16().count())
    }
}

struct InputElement {
    input: Entity<SearchInput>,
}

struct Prepaint {
    line: ShapedLine,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for InputElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for InputElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let input = self.input.read(cx);
        let display = if input.value.is_empty() {
            "搜索文字、网址或文件…"
        } else {
            &input.value
        };
        let style = window.text_style();
        let run = TextRun {
            len: display.len(),
            font: style.font(),
            color: if input.value.is_empty() {
                rgb(colors::FAINT).into()
            } else {
                style.color
            },
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            display.into(),
            style.font_size.to_pixels(window.rem_size()),
            &[run],
            None,
        );
        let selection = if !input.selection.is_empty() {
            Some(fill(
                Bounds::from_corners(
                    point(
                        bounds.left() + line.x_for_index(input.selection.start),
                        bounds.top(),
                    ),
                    point(
                        bounds.left() + line.x_for_index(input.selection.end),
                        bounds.bottom(),
                    ),
                ),
                rgba(0x365bd744),
            ))
        } else {
            None
        };
        let cursor = if input.focus.is_focused(window) && selection.is_none() {
            let offset = input.selection.end.min(input.value.len());
            Some(fill(
                Bounds::new(
                    point(bounds.left() + line.x_for_index(offset), bounds.top()),
                    size(px(2.), bounds.bottom() - bounds.top()),
                ),
                rgb(colors::ACCENT),
            ))
        } else {
            None
        };
        Prepaint {
            line,
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        prepaint
            .line
            .paint(
                bounds.origin,
                window.line_height(),
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            )
            .ok();
        if let Some(cursor) = prepaint.cursor.take() {
            window.paint_quad(cursor);
        }
        self.input.update(cx, |input, _| {
            input.layout = Some(prepaint.line.clone());
            input.bounds = Some(bounds);
        });
    }
}

impl Render for SearchInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(42.))
            .w_full()
            .px_3()
            .rounded_md()
            .border_1()
            .border_color(rgb(colors::STROKE))
            .bg(rgb(colors::CANVAS))
            .text_color(rgb(colors::INK))
            .flex()
            .items_center()
            .gap_2()
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_key_down(cx.listener(Self::handle_key))
            .child(
                div()
                    .text_color(rgb(colors::FAINT))
                    .text_size(px(18.))
                    .child("⌕"),
            )
            .child(InputElement { input: cx.entity() })
    }
}

impl Focusable for SearchInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::SearchInput;

    #[test]
    fn utf16_offsets_follow_chinese_ime_surrogate_boundaries() {
        assert_eq!(SearchInput::utf16_prefix_bytes("中😀文", 0), 0);
        assert_eq!(SearchInput::utf16_prefix_bytes("中😀文", 1), 3);
        assert_eq!(SearchInput::utf16_prefix_bytes("中😀文", 3), 7);
        assert_eq!(SearchInput::utf16_prefix_bytes("中😀文", 4), 10);
    }
}
