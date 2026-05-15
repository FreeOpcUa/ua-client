# ua-client

A small native OPC UA browser/inspector in Rust. Connect to a server, browse the address space, and inspect node attributes and references.

Built on [`async-opcua`](https://crates.io/crates/async-opcua) and [`egui`](https://github.com/emilk/egui)/`eframe`.

## Features

- Endpoint URL bar with Connect/Disconnect, history dropdown of past successful connections (persisted across restarts).
- Server endpoint picker (Connect → discovery dialog with all advertised security policies / modes / supported identity tokens).
- Authentication: Anonymous, Username/Password, X.509 Certificate. Username and certificate paths are persisted; password is not.
- Address-space tree on the left with lazy-load expansion. Root is expanded automatically on connect.
- Upper-right panel showing the selected node's identity (NodeId, BrowseName, DisplayName, NodeClass, Description) and — for Variables — its current Value.
- Lower-right tabbed panel: **References** (forward + inverse, all reference types), with placeholders for Attributes, Events, and Data Changes.
- Bottom log panel fed by `tracing`.

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

## Encrypted connections

For any policy other than `None`, the first connection attempt typically fails with `BadSecurityChecksFailed` / `BadCertificateUntrusted`. This is expected: the server has no reason to trust the freshly generated client certificate yet.

1. Run `cargo run` once. A self-signed client keypair is created at `pki/own/cert.der` + `pki/private/private.pem` next to the binary's working directory.
2. Try Connect with the desired encrypted endpoint. It will fail.
3. Find your client cert in the server's "rejected certificates" folder (server-specific path — e.g. for Prosys UA Simulation Server it's `<server>/USERS_PKI/rejected/certs/`) and move it into the corresponding "trusted certs" folder.
4. Connect again — it should succeed.

The log panel prints the cert path on every encrypted connection attempt and a hint when the server rejects it.

## Status

Early/experimental. Anonymous, Username/Password and X.509 identity tokens work; subscriptions, Attributes/Events/Data Changes tabs, and write/method calls are not implemented yet.

## License

MIT — see [LICENSE](LICENSE).
