use cursive::Cursive;
use cursive::direction::Orientation;
use cursive::event::Key;
use cursive::view::{Nameable, Resizable, Scrollable};
use cursive::views::{
    BoxedView, Dialog, DummyView, EditView, HideableView, LinearLayout, OnEventView, Panel,
    RadioGroup, SelectView, TextView,
};

use crate::messages::UiAction;
use crate::model::AppModel;
use crate::types::{AuthMode, EndpointInfo, SecurityMode};

use super::{TuiState, dispatch_action};

const ID_ENDPOINTS: &str = "dlg_endpoints";
const ID_AUTH_USER_ROW: &str = "dlg_auth_user_row";
const ID_AUTH_CERT_ROW: &str = "dlg_auth_cert_row";
const ID_AUTH_USERNAME: &str = "dlg_auth_username";
const ID_AUTH_PASSWORD: &str = "dlg_auth_password";
const ID_AUTH_CERT_PATH: &str = "dlg_auth_cert_path";
const ID_AUTH_KEY_PATH: &str = "dlg_auth_key_path";
const ID_STATUS: &str = "dlg_status";

pub fn show(siv: &mut Cursive) {
    let snap = match siv.user_data::<TuiState>() {
        Some(st) => InitialState::from(&st.engine.model),
        None => return,
    };
    let dialog = build(snap);
    let wrapped = OnEventView::new(dialog).on_pre_event(Key::Esc, |s| {
        dispatch_action(s, UiAction::CloseEndpointPicker);
    });
    siv.add_layer(wrapped);
    if let Some(st) = siv.user_data::<TuiState>() {
        st.dialog_open = true;
    }
    refresh(siv);
}

pub fn close(siv: &mut Cursive) {
    siv.pop_layer();
    if let Some(st) = siv.user_data::<TuiState>() {
        st.dialog_open = false;
    }
}

pub fn refresh(siv: &mut Cursive) {
    let Some(snap) = siv
        .user_data::<TuiState>()
        .map(|st| ListSnap::from(&st.engine.model))
    else {
        return;
    };
    siv.call_on_name(ID_STATUS, |v: &mut TextView| {
        v.set_content(snap.status_text.clone());
    });
    siv.call_on_name(ID_ENDPOINTS, |v: &mut SelectView<EndpointInfo>| {
        let preserved = v.selection().map(|arc| (*arc).clone());
        v.clear();
        for ep in &snap.endpoints {
            v.add_item(format_endpoint(ep), ep.clone());
        }
        if let Some(ref p) = preserved
            && let Some(idx) = snap.endpoints.iter().position(|e| endpoint_eq(e, p))
        {
            v.set_selection(idx);
        }
    });
}

struct InitialState {
    auth_mode: AuthMode,
    endpoint_mode_filter: SecurityMode,
    auth_username: String,
    auth_password: String,
    auth_cert_path: String,
    auth_key_path: String,
}

impl InitialState {
    fn from(model: &AppModel) -> Self {
        Self {
            auth_mode: model.auth_mode,
            endpoint_mode_filter: model.endpoint_mode_filter,
            auth_username: model.auth_username.clone(),
            auth_password: model.auth_password.clone(),
            auth_cert_path: model.auth_cert_path.clone(),
            auth_key_path: model.auth_key_path.clone(),
        }
    }
}

struct ListSnap {
    status_text: String,
    endpoints: Vec<EndpointInfo>,
}

impl ListSnap {
    fn from(model: &AppModel) -> Self {
        let status_text = if model.endpoints_loading {
            "Querying endpoints…".to_string()
        } else {
            match model.discovered_endpoints.as_ref() {
                None => "Click Refresh to query endpoints.".to_string(),
                Some(list) => {
                    let filtered = list
                        .iter()
                        .filter(|e| e.security_mode == model.endpoint_mode_filter)
                        .count();
                    if list.is_empty() {
                        "No endpoints returned by the server.".to_string()
                    } else if filtered == 0 {
                        format!(
                            "{} endpoint(s) returned; none match mode '{}'.",
                            list.len(),
                            model.endpoint_mode_filter.label()
                        )
                    } else {
                        format!(
                            "{} endpoint(s) matching '{}'.",
                            filtered,
                            model.endpoint_mode_filter.label()
                        )
                    }
                }
            }
        };
        let endpoints = model
            .discovered_endpoints
            .as_ref()
            .map(|eps| {
                let mut filtered: Vec<EndpointInfo> = eps
                    .iter()
                    .filter(|e| e.security_mode == model.endpoint_mode_filter)
                    .cloned()
                    .collect();
                filtered.sort_by(|a, b| b.security_level.cmp(&a.security_level));
                filtered
            })
            .unwrap_or_default();
        Self {
            status_text,
            endpoints,
        }
    }
}

fn build(snap: InitialState) -> Dialog {
    let mode_row = build_mode_row(snap.endpoint_mode_filter);
    let auth_row = build_auth_row(snap.auth_mode);
    let user_hide = build_user_fields(&snap);
    let cert_hide = build_cert_fields(&snap);
    let endpoints_panel = build_endpoints_panel();
    let status = TextView::new("").with_name(ID_STATUS);

    let content = LinearLayout::new(Orientation::Vertical)
        .child(mode_row)
        .child(DummyView.fixed_height(1))
        .child(auth_row)
        .child(user_hide)
        .child(cert_hide)
        .child(DummyView.fixed_height(1))
        .child(endpoints_panel)
        .child(DummyView.fixed_height(1))
        .child(status);

    Dialog::around(content.min_size((78, 18)))
        .title("Connect to OPC UA server")
        .button("Refresh", |s| {
            dispatch_action(s, UiAction::ForceRefreshEndpoints)
        })
        .button("Cancel", |s| dispatch_action(s, UiAction::CloseEndpointPicker))
        .button("Connect", |s| dispatch_action(s, UiAction::ConfirmConnect))
}

fn build_mode_row(current: SecurityMode) -> LinearLayout {
    let mut group: RadioGroup<SecurityMode> = RadioGroup::new();
    group.set_on_change(|s, m: &SecurityMode| {
        dispatch_action(s, UiAction::SetEndpointModeFilter(*m));
    });
    let none_btn = group.button(SecurityMode::None, "None");
    let sign_btn = group.button(SecurityMode::Sign, "Sign");
    let se_btn = group.button(SecurityMode::SignAndEncrypt, "SignAndEncrypt");
    let (none_btn, sign_btn, se_btn) = match current {
        SecurityMode::None => (none_btn.selected(), sign_btn, se_btn),
        SecurityMode::Sign => (none_btn, sign_btn.selected(), se_btn),
        SecurityMode::SignAndEncrypt => (none_btn, sign_btn, se_btn.selected()),
    };
    LinearLayout::new(Orientation::Horizontal)
        .child(TextView::new("Mode: "))
        .child(none_btn)
        .child(DummyView.fixed_width(2))
        .child(sign_btn)
        .child(DummyView.fixed_width(2))
        .child(se_btn)
}

fn build_auth_row(current: AuthMode) -> LinearLayout {
    let mut group: RadioGroup<AuthMode> = RadioGroup::new();
    group.set_on_change(|s, m: &AuthMode| {
        dispatch_action(s, UiAction::SetAuthMode(*m));
        update_auth_visibility(s, *m);
    });
    let anon = group.button(AuthMode::Anonymous, "Anonymous");
    let user = group.button(AuthMode::UserName, "UserName");
    let cert = group.button(AuthMode::Certificate, "Certificate");
    let (anon, user, cert) = match current {
        AuthMode::Anonymous => (anon.selected(), user, cert),
        AuthMode::UserName => (anon, user.selected(), cert),
        AuthMode::Certificate => (anon, user, cert.selected()),
    };
    LinearLayout::new(Orientation::Horizontal)
        .child(TextView::new("Auth: "))
        .child(anon)
        .child(DummyView.fixed_width(2))
        .child(user)
        .child(DummyView.fixed_width(2))
        .child(cert)
}

fn build_user_fields(snap: &InitialState) -> cursive::views::NamedView<HideableView<BoxedView>> {
    let user_edit = EditView::new()
        .content(snap.auth_username.clone())
        .on_edit(|s, c, _| dispatch_action(s, UiAction::AuthUsernameEdited(c.to_owned())))
        .with_name(ID_AUTH_USERNAME)
        .min_width(20);
    let pass_edit = EditView::new()
        .secret()
        .content(snap.auth_password.clone())
        .on_edit(|s, c, _| dispatch_action(s, UiAction::AuthPasswordEdited(c.to_owned())))
        .with_name(ID_AUTH_PASSWORD)
        .min_width(20);
    let row = LinearLayout::new(Orientation::Horizontal)
        .child(TextView::new("User: "))
        .child(user_edit)
        .child(DummyView.fixed_width(2))
        .child(TextView::new("Pass: "))
        .child(pass_edit);
    HideableView::new(BoxedView::boxed(row))
        .visible(matches!(snap.auth_mode, AuthMode::UserName))
        .with_name(ID_AUTH_USER_ROW)
}

fn build_cert_fields(snap: &InitialState) -> cursive::views::NamedView<HideableView<BoxedView>> {
    let cert_edit = EditView::new()
        .content(snap.auth_cert_path.clone())
        .on_edit(|s, c, _| dispatch_action(s, UiAction::AuthCertPathEdited(c.to_owned())))
        .with_name(ID_AUTH_CERT_PATH)
        .min_width(40);
    let key_edit = EditView::new()
        .content(snap.auth_key_path.clone())
        .on_edit(|s, c, _| dispatch_action(s, UiAction::AuthKeyPathEdited(c.to_owned())))
        .with_name(ID_AUTH_KEY_PATH)
        .min_width(40);
    let row = LinearLayout::new(Orientation::Vertical)
        .child(
            LinearLayout::new(Orientation::Horizontal)
                .child(TextView::new("Cert: "))
                .child(cert_edit),
        )
        .child(
            LinearLayout::new(Orientation::Horizontal)
                .child(TextView::new("Key:  "))
                .child(key_edit),
        );
    HideableView::new(BoxedView::boxed(row))
        .visible(matches!(snap.auth_mode, AuthMode::Certificate))
        .with_name(ID_AUTH_CERT_ROW)
}

fn build_endpoints_panel() -> Panel<BoxedView> {
    let mut select = SelectView::<EndpointInfo>::new();
    select.set_on_submit(|s, ep: &EndpointInfo| {
        dispatch_action(s, UiAction::SelectEndpoint(ep.clone()));
    });
    let scrollable = select.with_name(ID_ENDPOINTS).scrollable();
    Panel::new(BoxedView::boxed(scrollable.fixed_height(8))).title("Endpoints")
}

fn update_auth_visibility(siv: &mut Cursive, mode: AuthMode) {
    siv.call_on_name(ID_AUTH_USER_ROW, |v: &mut HideableView<BoxedView>| {
        v.set_visible(matches!(mode, AuthMode::UserName));
    });
    siv.call_on_name(ID_AUTH_CERT_ROW, |v: &mut HideableView<BoxedView>| {
        v.set_visible(matches!(mode, AuthMode::Certificate));
    });
}

fn format_endpoint(ep: &EndpointInfo) -> String {
    format!(
        "{} L{} [{}]   {}",
        ep.security_policy,
        ep.security_level,
        token_label(ep),
        ep.endpoint_url
    )
}

fn token_label(ep: &EndpointInfo) -> String {
    let mut parts = Vec::new();
    if ep.supports_anonymous {
        parts.push("Anon");
    }
    if ep.supports_username {
        parts.push("User");
    }
    if ep.supports_certificate {
        parts.push("Cert");
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join(", ")
    }
}

fn endpoint_eq(a: &EndpointInfo, b: &EndpointInfo) -> bool {
    a.endpoint_url == b.endpoint_url
        && a.security_policy_uri == b.security_policy_uri
        && a.security_mode == b.security_mode
}
