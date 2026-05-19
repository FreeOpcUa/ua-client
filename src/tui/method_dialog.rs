use std::fmt::Write as _;

use cursive::Cursive;
use cursive::direction::Orientation;
use cursive::event::Key;
use cursive::view::{Nameable, Resizable, Scrollable};
use cursive::views::{Dialog, DummyView, EditView, LinearLayout, OnEventView, TextView};

use crate::messages::UiAction;
use crate::model::MethodCallState;
use crate::types::{MethodArgument, MethodCallOutcome, MethodSignature, ValueTree};

use super::{TuiState, dispatch_action};

const ID_METHOD_DIALOG: &str = "method_dialog";
const ID_METHOD_BODY: &str = "method_dialog_body";
const ID_METHOD_CALL_ERROR: &str = "method_call_error";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodPhase {
    Loading,
    Failed,
    Inputs,
    Calling,
    Result,
}

pub(super) fn phase_of(state: &MethodCallState) -> MethodPhase {
    match state {
        MethodCallState::Loading { .. } => MethodPhase::Loading,
        MethodCallState::Failed { .. } => MethodPhase::Failed,
        MethodCallState::Inputs { .. } => MethodPhase::Inputs,
        MethodCallState::Calling { .. } => MethodPhase::Calling,
        MethodCallState::Result { .. } => MethodPhase::Result,
    }
}

pub fn show(siv: &mut Cursive) {
    let body = TextView::new("").with_name(ID_METHOD_BODY);
    let dialog = Dialog::around(LinearLayout::new(Orientation::Vertical).child(body))
        .title("Call Method")
        .with_name(ID_METHOD_DIALOG);
    let wrapped = OnEventView::new(dialog).on_pre_event(Key::Esc, |s| {
        dispatch_action(s, UiAction::CloseMethodCall);
    });
    siv.add_layer(wrapped);
    if let Some(st) = siv.user_data::<TuiState>() {
        st.method_dialog_open = true;
        st.method_dialog_phase = None;
    }
    refresh(siv);
}

pub fn close(siv: &mut Cursive) {
    siv.pop_layer();
    if let Some(st) = siv.user_data::<TuiState>() {
        st.method_dialog_open = false;
        st.method_dialog_phase = None;
    }
}

pub fn refresh(siv: &mut Cursive) {
    let snap = siv
        .user_data::<TuiState>()
        .and_then(|st| st.engine.model.method_call.clone());
    let Some(state) = snap else {
        return;
    };
    let new_phase = phase_of(&state);
    let old_phase = siv
        .user_data::<TuiState>()
        .and_then(|st| st.method_dialog_phase);

    if old_phase != Some(new_phase) {
        rebuild_dialog(siv, &state);
        if let Some(st) = siv.user_data::<TuiState>() {
            st.method_dialog_phase = Some(new_phase);
        }
    } else if let MethodCallState::Inputs { field_errors, call_error, .. } = &state {
        update_input_errors(siv, field_errors, call_error.as_deref());
    }
}

fn rebuild_dialog(siv: &mut Cursive, state: &MethodCallState) {
    let title = build_title(state);
    siv.call_on_name(ID_METHOD_DIALOG, |d: &mut Dialog| {
        d.set_title(title.clone());
        d.clear_buttons();
        match state {
            MethodCallState::Loading { .. } => {
                d.add_button("Close", |s| dispatch_action(s, UiAction::CloseMethodCall));
            }
            MethodCallState::Failed { .. } => {
                d.add_button("Close", |s| dispatch_action(s, UiAction::CloseMethodCall));
            }
            MethodCallState::Inputs { .. } => {
                d.add_button("Call", |s| dispatch_action(s, UiAction::CallMethodConfirmed));
                d.add_button("Close", |s| dispatch_action(s, UiAction::CloseMethodCall));
            }
            MethodCallState::Calling { .. } => {
                d.add_button("Close", |s| dispatch_action(s, UiAction::CloseMethodCall));
            }
            MethodCallState::Result { .. } => {
                d.add_button("Call again", |s| dispatch_action(s, UiAction::CallMethodConfirmed));
                d.add_button("Close", |s| dispatch_action(s, UiAction::CloseMethodCall));
            }
        }
    });

    let layout = build_body(state).min_size((60, 8)).max_size((100, 24)).scrollable();
    siv.call_on_name(ID_METHOD_DIALOG, |d: &mut Dialog| {
        d.set_content(layout);
    });

    if matches!(state, MethodCallState::Inputs { .. }) {
        siv.focus_name(&arg_input_id(0)).ok();
    }
}

fn build_title(state: &MethodCallState) -> String {
    let display = match state {
        MethodCallState::Loading { node } => node.to_string(),
        MethodCallState::Failed { node, .. } => node.to_string(),
        MethodCallState::Inputs { signature, .. }
        | MethodCallState::Calling { signature, .. }
        | MethodCallState::Result { signature, .. } => signature.method_display_name.clone(),
    };
    format!("Call Method · {display}")
}

fn build_body(state: &MethodCallState) -> LinearLayout {
    match state {
        MethodCallState::Loading { .. } => single_text("Reading method signature…"),
        MethodCallState::Failed { error, .. } => single_text(&format!("Error: {error}")),
        MethodCallState::Inputs {
            signature,
            edited,
            field_errors,
            call_error,
            ..
        } => build_inputs_body(signature, edited, field_errors, call_error.as_deref()),
        MethodCallState::Calling {
            signature, edited, ..
        } => build_calling_body(signature, edited),
        MethodCallState::Result {
            signature,
            edited,
            outcome,
            ..
        } => build_result_body(signature, edited, outcome),
    }
}

fn single_text(text: &str) -> LinearLayout {
    LinearLayout::new(Orientation::Vertical)
        .child(TextView::new(text.to_string()))
}

fn build_inputs_body(
    signature: &MethodSignature,
    edited: &[String],
    field_errors: &[Option<String>],
    call_error: Option<&str>,
) -> LinearLayout {
    let mut layout = LinearLayout::new(Orientation::Vertical);
    layout.add_child(TextView::new(format!(
        "Method: {}\nParent object: {}",
        signature.method_node, signature.parent_object,
    )));
    layout.add_child(DummyView.fixed_height(1));

    if signature.inputs.is_empty() {
        layout.add_child(TextView::new("(no input arguments)"));
    } else {
        layout.add_child(TextView::new("Inputs:"));
        for (i, arg) in signature.inputs.iter().enumerate() {
            layout.add_child(input_row(i, arg, edited.get(i).map(String::as_str).unwrap_or("")));
            let err_text = field_errors
                .get(i)
                .and_then(|e| e.as_deref())
                .unwrap_or("");
            layout.add_child(
                TextView::new(err_text.to_string()).with_name(arg_error_id(i)),
            );
            if !arg.description.is_empty() {
                layout.add_child(TextView::new(format!("    {}", arg.description)));
            }
        }
    }

    layout.add_child(DummyView.fixed_height(1));
    if !signature.outputs.is_empty() {
        layout.add_child(TextView::new("Output signature:"));
        for arg in &signature.outputs {
            layout.add_child(TextView::new(format!(
                "  {} ({})",
                arg.name, arg.type_label
            )));
        }
        layout.add_child(DummyView.fixed_height(1));
    }

    layout.add_child(
        TextView::new(call_error.unwrap_or("").to_string()).with_name(ID_METHOD_CALL_ERROR),
    );
    layout
}

fn build_calling_body(signature: &MethodSignature, edited: &[String]) -> LinearLayout {
    let mut layout = LinearLayout::new(Orientation::Vertical);
    layout.add_child(TextView::new(format!(
        "Method: {}",
        signature.method_display_name
    )));
    layout.add_child(DummyView.fixed_height(1));
    for (i, arg) in signature.inputs.iter().enumerate() {
        let val = edited.get(i).map(String::as_str).unwrap_or("");
        layout.add_child(TextView::new(format!(
            "  {} ({}): {}",
            arg.name, arg.type_label, val
        )));
    }
    layout.add_child(DummyView.fixed_height(1));
    layout.add_child(TextView::new("Calling…"));
    layout
}

fn build_result_body(
    signature: &MethodSignature,
    edited: &[String],
    outcome: &MethodCallOutcome,
) -> LinearLayout {
    let mut layout = LinearLayout::new(Orientation::Vertical);
    layout.add_child(TextView::new(format!(
        "Method: {}\nParent object: {}",
        signature.method_node, signature.parent_object,
    )));
    layout.add_child(DummyView.fixed_height(1));

    if signature.inputs.is_empty() {
        layout.add_child(TextView::new("(no input arguments)"));
    } else {
        layout.add_child(TextView::new("Inputs:"));
        for (i, arg) in signature.inputs.iter().enumerate() {
            layout.add_child(input_row(i, arg, edited.get(i).map(String::as_str).unwrap_or("")));
            let server_err = outcome
                .input_arg_errors
                .get(i)
                .and_then(|e| e.as_deref())
                .unwrap_or("");
            layout.add_child(
                TextView::new(server_err.to_string()).with_name(arg_error_id(i)),
            );
        }
    }

    layout.add_child(DummyView.fixed_height(1));
    layout.add_child(TextView::new(format!("Status: {}", outcome.status)));
    layout.add_child(DummyView.fixed_height(1));

    if signature.outputs.is_empty() {
        layout.add_child(TextView::new("(no output arguments)"));
    } else {
        layout.add_child(TextView::new("Outputs:"));
        for (i, arg) in signature.outputs.iter().enumerate() {
            let rendered = outcome
                .outputs
                .get(i)
                .map(render_value_inline)
                .unwrap_or_else(|| "<missing>".to_string());
            layout.add_child(TextView::new(format!(
                "  {} ({}): {}",
                arg.name, arg.type_label, rendered
            )));
        }
    }

    layout.add_child(DummyView.fixed_height(1));
    layout.add_child(
        TextView::new(String::new()).with_name(ID_METHOD_CALL_ERROR),
    );
    layout
}

fn input_row(index: usize, arg: &MethodArgument, current: &str) -> LinearLayout {
    let label = format!("  {} ({}): ", arg.name, arg.type_label);
    let edit = EditView::new()
        .content(current.to_string())
        .on_edit(move |s, content, _| {
            dispatch_action(
                s,
                UiAction::MethodArgEdited {
                    index,
                    value: content.to_string(),
                },
            );
        })
        .on_submit(|s, _| dispatch_action(s, UiAction::CallMethodConfirmed))
        .with_name(arg_input_id(index))
        .min_width(30);
    LinearLayout::new(Orientation::Horizontal)
        .child(TextView::new(label))
        .child(edit)
}

fn update_input_errors(
    siv: &mut Cursive,
    field_errors: &[Option<String>],
    call_error: Option<&str>,
) {
    for (i, err) in field_errors.iter().enumerate() {
        let text = err.clone().unwrap_or_default();
        siv.call_on_name(&arg_error_id(i), |v: &mut TextView| {
            v.set_content(text.clone());
        });
    }
    let call_text = call_error.unwrap_or("").to_string();
    siv.call_on_name(ID_METHOD_CALL_ERROR, |v: &mut TextView| {
        v.set_content(call_text);
    });
}

fn arg_input_id(index: usize) -> String {
    format!("method_arg_input_{index}")
}

fn arg_error_id(index: usize) -> String {
    format!("method_arg_err_{index}")
}

fn render_value_inline(v: &ValueTree) -> String {
    let mut out = String::new();
    render(v, &mut out);
    out
}

fn render(v: &ValueTree, out: &mut String) {
    match v {
        ValueTree::Null => {
            let _ = write!(out, "<null>");
        }
        ValueTree::Leaf(s) => {
            let _ = write!(out, "{s}");
        }
        ValueTree::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                render(item, out);
            }
            out.push(']');
        }
        ValueTree::Object(fields) => {
            out.push('{');
            for (i, (k, val)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                let _ = write!(out, "{k}: ");
                render(val, out);
            }
            out.push('}');
        }
    }
}
