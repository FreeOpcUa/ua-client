# ua-client

A small native OPC UA browser/inspector in Rust. Connect to a server, browse the address space, and inspect node attributes and references.

Built on [`async-opcua`](https://crates.io/crates/async-opcua) and [`egui`](https://github.com/emilk/egui)/`eframe`.

## Features

- Endpoint URL bar with Connect/Disconnect, history dropdown of past successful connections (persisted across restarts).
- Address-space tree on the left with lazy-load expansion. Root is expanded automatically on connect.
- Upper-right panel showing the selected node's identity (NodeId, BrowseName, DisplayName, NodeClass, Description) and — for Variables — its current Value.
- Lower-right tabbed panel: **References** (forward + inverse, all reference types), with placeholders for Attributes, Events, and Data Changes.
- Bottom log panel fed by `tracing`.
- Anonymous authentication with `SecurityPolicy::None` (v1).

## Build & run

```bash
cargo run
```

To exercise it end-to-end, run the `simple-server` sample from `async-opcua` and connect to `opc.tcp://localhost:4855`:

```bash
# in another checkout
git clone https://github.com/freeopcua/async-opcua
cd async-opcua && cargo run -p async-opcua-simple-server
```

Set `RUST_LOG` to control verbosity; the default is `info,opcua=info,ua_client=debug`.

## Status

Early/experimental. Anonymous + `None` security only; subscriptions, Attributes/Events/Data Changes tabs, and write/method calls are not implemented yet.

## License

MIT — see [LICENSE](LICENSE).
