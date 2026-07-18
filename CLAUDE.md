# ua-client

OPC UA browser/inspector in Rust with a terminal UI (`cursive`, default) and a desktop GUI (`egui`/`eframe`). Uses `async-opcua` from crates.io.

## Build & run

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo run
```

To exercise it end-to-end, run `cd ../async-opcua && cargo run -p async-opcua-simple-server` in another terminal, then connect to `opc.tcp://localhost:4855`.

## Architecture — Model–View–Update

The app is structured as MVU on top of egui + tokio. **Do not break this layering** when adding features.

```
View (ui/*.rs, pure)  ──reads──▶  Model (model.rs, plain data)
       │                                ▲
       │ pushes UiAction                │ &mut, apply UiUpdate
       ▼                                │
   Update (app.rs)  ──spawns──▶  UaClient (client.rs, async)
                       result via mpsc       │
                                             ▼
                                   async-opcua Session
```

Three invariants:

1. **Model is plain data.** `AppModel` in `src/model.rs` holds no `egui::*`, no `async-opcua` `Session`, no `tokio::*`. Anything beyond plain types/data goes elsewhere.
2. **Views are pure.** Each `src/ui/*.rs` exposes `fn draw(model: &AppModel, ui: &mut egui::Ui, actions: &mut Vec<UiAction>)`. Views render the model and append user intents — they do not mutate, do not call the client, do not spawn tasks.
3. **Update is the only mutator.** `UaApp::update()` in `src/app.rs` (a) drains `update_rx` and applies each `UiUpdate` to the model, (b) calls `ui::draw`, (c) dispatches each `UiAction` — either mutates the model directly (local action) or spawns a tokio task on `self.rt` that sends a `UiUpdate` back when done.

`UaClient` (`src/client.rs`) is the **only** surface that touches `async-opcua`. **No async-opcua types must leak past it** — it always returns plain types from `src/types.rs` (`TreeChild`, `NodeSummary`, `ReferenceRow`). Exception: `NodeId` and `NodeClass` are re-used directly because they are cheap, `Hash+Eq+Clone`, and used as map keys.

## Modules

| File                     | Role                                                                                            |
| ------------------------ | ----------------------------------------------------------------------------------------------- |
| `src/main.rs`            | eframe entry, builds tokio Runtime and tracing layer                                            |
| `src/app.rs`             | `UaApp: eframe::App` — Update step, channel plumbing, `save()` for persistence                  |
| `src/model.rs`           | `AppModel`, `TreeModel`, `ConnectionState`, `DetailTab`                                         |
| `src/messages.rs`        | `UiAction` (View→Update), `UiUpdate` (async→Update)                                             |
| `src/client.rs`          | `UaClient` async OPC UA bridge                                                                  |
| `src/types.rs`           | Plain data crossing the channel boundary                                                        |
| `src/logger.rs`          | tracing `Layer` that emits `UiUpdate::Log`                                                      |
| `src/ui/mod.rs`          | Panel layout (top URI bar, left tree, bottom log, central split)                                |
| `src/ui/connect_bar.rs`  | URI textbox + history dropdown + Connect/Disconnect                                             |
| `src/ui/tree.rs`         | Lazy-loaded address-space tree                                                                  |
| `src/ui/node_summary.rs` | Upper-right summary widget                                                                      |
| `src/ui/tabs.rs`         | Lower-right tabs: Attributes / Events / Data Changes / References (only References implemented) |
| `src/ui/log_panel.rs`    | Bottom log                                                                                      |

## Adding async work

When a user interaction triggers an OPC UA call:

1. Add a variant to `UiAction` (view-side).
2. Add a variant to `UiUpdate` (result-side).
3. Implement the async method on `UaClient` — it must return plain types only.
4. In `UaApp::dispatch`, add a `spawn_*` helper that clones `self.client`, `self.update_tx`, `ctx`; spawns on `self.rt`; sends a `UiUpdate` and calls `ctx.request_repaint()` from the task.
5. In `UaApp::apply_update`, handle the new `UiUpdate` variant — typically guarded by checking `self.model.selected == Some(&node)` so late responses for stale selections are dropped.

## async-opcua specifics

- Entry point is `opcua::client::{ClientBuilder, IdentityToken}`, `opcua::types::*`.
- `Client::connect_to_matching_endpoint(endpoint, IdentityToken)` returns `(Arc<Session>, SessionEventLoop)`. Always `event_loop.spawn()` and `session.wait_for_connection().await` before using the session. Always `session.disconnect().await` then await the event-loop handle on disconnect.
- Browse: `session.browse(&[BrowseDescription], max_refs, view)`. Hierarchical-only: `reference_type_id = ReferenceTypeId::HierarchicalReferences`, `include_subtypes = true`, `BrowseDirection::Forward`. All references (for the References tab): `ReferenceTypeId::References`, `BrowseDirection::Both`.
- Read attributes: `session.read(&[ReadValueId], TimestampsToReturn, max_age)`. Construct via `ReadValueId::new(node_id, AttributeId::DisplayName)`.
- `ReferenceDescription.node_id` is an `ExpandedNodeId` — pull `.node_id` (a `NodeId`) for local use.
- Reference-type display names are resolved via a follow-up `Read` for `AttributeId::DisplayName` (see `resolve_reference_type_name` in `client.rs`).

## Persistence

`eframe` is built with the `persistence` feature. `UaApp::save` persists `endpoint_url` and `endpoint_history` via `eframe::set_value`; `UaApp::new` reads them back from `cc.storage`. egui also persists panel sizes (e.g. the resizable splits) automatically through this mechanism.

If you add new persisted state, put `pub const STORAGE_*` keys near the top of `app.rs` and add matching `set_value` / `get_value` calls.

## Conventions

- `cargo clippy --all-targets -- -D warnings` must stay clean.
- Edition 2021.
- Plain types in `types.rs` use `derive(Debug, Clone)`. Persisted types add `serde::{Serialize, Deserialize}` only on the fields actually saved (currently `endpoint_url: String` and `endpoint_history: Vec<String>` — both stdlib types, no custom derive needed).
- Tracing: targets are crate-scoped; default filter is `info,opcua=info,ua_client=debug`. Override via `RUST_LOG`.
- The address-space root is `NodeId::new(0, ObjectId::RootFolder as u32)` — set on `AppModel::default()`.
- Default to writing no comments. Only add one when the _why_ is non-obvious — a hidden constraint, a subtle invariant, a workaround for a specific bug, or behavior that would surprise a reader. Well-named identifiers should already convey _what_ the code does. Never reference the current task, the last commit, a recent fix, or which caller motivated the line ("// CLI override beats saved selection", "// added for the X flow", "// fixes #123") — that context belongs in the commit message, not the source.
