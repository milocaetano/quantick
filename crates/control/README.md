# quantick-control

`quantick-control` is the transport-neutral contract crate for Quantick's local
control plane. It gives application modules, gateways, MCP adapters, and future
SDKs one set of owned DTOs and validation rules without exposing UI or trading
internals.

The crate provides:

- extensible validated string IDs and opaque runtime identities;
- version negotiation, authenticated handshake DTOs, and authority downscoping;
- request, response, actor, revision, cursor, and structured-error contracts;
- dynamic module, permission, profile, effect-policy, and capability registries;
- Draft 2020-12 schema validation and conservative compatibility checks;
- Quantick Canonical JSON v1 and SHA-256 digests;
- a bounded four-byte big-endian length-prefixed JSON codec;
- an in-memory fake host, client, connection, and two fake modules.

It intentionally has no socket, async runtime, persistence, MCP implementation,
application dependency, domain dependency, UI type, or trading behavior. Those
belong to later adapters and hosts. The crate is cold-path infrastructure and
must never enter market-data or rendering loops.

Module docking is deliberately two-phase: modules contribute permission
descriptors first, the host finalizes profile ceilings, and modules then
register their effects and capabilities. This lets an extension add a scoped
permission without editing a central profile or the contract crate.

Public wire schemas and the reference capability catalog are generated from the
Rust contracts and committed under `schemas/control/`. After an intentional
contract edit, review the output of:

```sh
cargo run -p quantick-control --example export_schemas -- --patch
```

Apply the emitted patch, then run the repository verification loop. Snapshot
tests reject an unreviewed schema drift, and breaking fixture tests require a
version bump.
