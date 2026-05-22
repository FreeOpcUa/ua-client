use cursive::Cursive;
use cursive::direction::Orientation;
use cursive::event::Key;
use cursive::view::{Nameable, Resizable, Scrollable};
use cursive::views::{Dialog, DummyView, EditView, LinearLayout, OnEventView, TextView};

use crate::messages::UiAction;
use crate::model::AttributeEditState;

use super::{TuiState, dispatch_action};

const ID_DIALOG: &str = "attr_edit_dialog";
const ID_VALUE_INPUT: &str = "attr_edit_value";
const ID_FIELD_ERROR: &str = "attr_edit_field_error";
const ID_WRITE_ERROR: &str = "attr_edit_write_error";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttrEditPhase {
    Loading,
    Failed,
    Inputs,
    Writing,
}

fn phase_of(state: &AttributeEditState) -> AttrEditPhase {
    match state {
        AttributeEditState::Loading { .. } => AttrEditPhase::Loading,
        AttributeEditState::Failed { .. } => AttrEditPhase::Failed,
        AttributeEditState::Inputs { .. } => AttrEditPhase::Inputs,
        AttributeEditState::Writing { .. } => AttrEditPhase::Writing,
    }
}

pub fn show(siv: &mut Cursive) {
    let dialog = Dialog::around(TextView::new(""))
        .title("Edit Attribute")
        .with_name(ID_DIALOG);
    let wrapped = OnEventView::new(dialog).on_pre_event(Key::Esc, |s| {
        dispatch_action(s, UiAction::CloseAttributeEdit);
    });
    siv.add_layer(wrapped);
    if let Some(st) = siv.user_data::<TuiState>() {
        st.attr_edit_dialog_open = true;
        st.attr_edit_dialog_phase = None;
    }
    refresh(siv);
}

pub fn close(siv: &mut Cursive) {
    siv.pop_layer();
    if let Some(st) = siv.user_data::<TuiState>() {
        st.attr_edit_dialog_open = false;
        st.attr_edit_dialog_phase = None;
    }
}

pub fn refresh(siv: &mut Cursive) {
    let snap = siv
        .user_data::<TuiState>()
        .and_then(|st| st.engine.model.attr_edit.clone());
    let Some(state) = snap else {
        return;
    };
    let new_phase = phase_of(&state);
    let old_phase = siv
        .user_data::<TuiState>()
        .and_then(|st| st.attr_edit_dialog_phase);

    if old_phase != Some(new_phase) {
        rebuild_dialog(siv, &state);
        if let Some(st) = siv.user_data::<TuiState>() {
            st.attr_edit_dialog_phase = Some(new_phase);
        }
    } else if let AttributeEditState::Inputs {
        field_error,
        write_error,
        ..
    } = &state
    {
        update_errors(siv, field_error.as_deref(), write_error.as_deref());
    }
}

fn rebuild_dialog(siv: &mut Cursive, state: &AttributeEditState) {
    let title = format!("Edit Attribute · {}", state.attr_name());
    siv.call_on_name(ID_DIALOG, |d: &mut Dialog| {
        d.set_title(title.clone());
        d.clear_buttons();
        match state {
            AttributeEditState::Loading { .. }
            | AttributeEditState::Failed { .. }
            | AttributeEditState::Writing { .. } => {
                d.add_button("Close", |s| dispatch_action(s, UiAction::CloseAttributeEdit));
            }
            AttributeEditState::Inputs { .. } => {
                d.add_button("Write", |s| dispatch_action(s, UiAction::ConfirmAttributeEdit));
                d.add_button("Cancel", |s| dispatch_action(s, UiAction::CloseAttributeEdit));
            }
        }
    });

    let layout = build_body(state).min_size((60, 6)).max_size((100, 18)).scrollable();
    siv.call_on_name(ID_DIALOG, |d: &mut Dialog| {
        d.set_content(layout);
    });

    if matches!(state, AttributeEditState::Inputs { .. }) {
        siv.focus_name(ID_VALUE_INPUT).ok();
    }
}

fn build_body(state: &AttributeEditState) -> LinearLayout {
    match state {
        AttributeEditState::Loading { node, .. } => single_text(&format!(
            "Reading attribute on {node}…"
        )),
        AttributeEditState::Failed { error, .. } => single_text(&format!("Error: {error}")),
        AttributeEditState::Inputs {
            attr_name,
            target,
            edited,
            field_error,
            write_error,
            ..
        } => build_inputs_body(
            attr_name,
            &target.type_label,
            edited,
            field_error.as_deref(),
            write_error.as_deref(),
        ),
        AttributeEditState::Writing {
            attr_name,
            target,
            edited,
            ..
        } => build_writing_body(attr_name, &target.type_label, edited),
    }
}

fn single_text(text: &str) -> LinearLayout {
    LinearLayout::new(Orientation::Vertical).child(TextView::new(text.to_string()))
}

fn build_inputs_body(
    attr_name: &str,
    type_label: &str,
    edited: &str,
    field_error: Option<&str>,
    write_error: Option<&str>,
) -> LinearLayout {
    let mut layout = LinearLayout::new(Orientation::Vertical);
    layout.add_child(TextView::new(format!("{attr_name}  ({type_label})")));
    layout.add_child(DummyView.fixed_height(1));

    let edit = EditView::new()
        .content(edited.to_string())
        .on_edit(|s, content, _| {
            dispatch_action(s, UiAction::AttributeValueEdited(content.to_string()));
        })
        .on_submit(|s, _| dispatch_action(s, UiAction::ConfirmAttributeEdit))
        .with_name(ID_VALUE_INPUT)
        .min_width(40);
    layout.add_child(edit);

    layout.add_child(
        TextView::new(field_error.unwrap_or("").to_string()).with_name(ID_FIELD_ERROR),
    );
    layout.add_child(
        TextView::new(write_error.unwrap_or("").to_string()).with_name(ID_WRITE_ERROR),
    );
    layout
}

fn build_writing_body(attr_name: &str, type_label: &str, edited: &str) -> LinearLayout {
    LinearLayout::new(Orientation::Vertical)
        .child(TextView::new(format!("{attr_name}  ({type_label})")))
        .child(DummyView.fixed_height(1))
        .child(TextView::new(format!("Writing: {edited}")))
        .child(DummyView.fixed_height(1))
        .child(TextView::new("Sending…"))
}

fn update_errors(siv: &mut Cursive, field_error: Option<&str>, write_error: Option<&str>) {
    let f = field_error.unwrap_or("").to_string();
    siv.call_on_name(ID_FIELD_ERROR, |v: &mut TextView| v.set_content(f));
    let w = write_error.unwrap_or("").to_string();
    siv.call_on_name(ID_WRITE_ERROR, |v: &mut TextView| v.set_content(w));
}
