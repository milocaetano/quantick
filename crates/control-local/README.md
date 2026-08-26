# quantick-control-local

Local transport of the Quantick control plane, per
`docs/control-plane/adr-0001-local-transport-and-instance-discovery.md`:

- `discovery` — the private instance-descriptor directory: a running instance
  publishes `<instance_id>.json` there; a client lists, verifies and reads
  those files. One implementation of the ownership and permission checks serves
  both sides.
- `client` — the blocking loopback client: connect, authenticate with the
  descriptor's bearer token, then exchange framed request/response envelopes.

Depends only on `quantick-control`. It never launches the application and
never binds a socket of its own.
