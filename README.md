# Freenet Search Engine

A decentralized search engine for discovering web applications on the [Freenet](https://freenet.org) network. The search index is a shared Freenet contract contributed to by all participants — no central server, no crawlers.

## Architecture

The project is a Rust workspace with 5 crates:

| Crate | Description |
|-------|-------------|
| `search-common` | Shared types, CBOR serialization, tokenization, scoring, bloom filters, web container extraction |
| `contract-catalog` | Freenet contract storing the app catalog (URL metadata, contributor reputation, anti-Sybil) |
| `contract-fulltext-shard` | Freenet contract storing inverted index shards for full-text search |
| `delegate-identity` | Freenet delegate managing ed25519 keypairs for contributor identity |
| `ui` | Dioxus 0.7 WASM app — browsing, search, and contribution UI |

### How it works

1. **Catalog contract** stores metadata (title, description, snippet) for every indexed web app, with contributor attestations and reputation scores
2. **Fulltext shard contracts** store an inverted index partitioned by keyword hash, enabling search across all indexed apps
3. **Identity delegate** manages a local ed25519 keypair for signing contributions (private key never leaves the node)
4. **UI** connects to the local Freenet node via WebSocket, discovers contracts, and renders a searchable app directory

### Key design decisions

- **CBOR serialization** for all cross-node types (no JSON) — self-describing, schema-evolution friendly
- **Integer-only scoring** with x10000 scaling (no floating-point in any contract state) — deterministic across all WASM runtimes
- **CRDT merge** for contract state — grow-only maps, max-wins for scores, dedup by pubkey, deterministic finalization. All `update_state` implementations are commutative
- **Bloom filter sync** for `summarize_state` / `get_state_delta` — compact, efficient for grow-only CRDTs (k=7, SHA-256)
- **Anti-Sybil** — antiflood tokens (proof-of-work) + ed25519 signatures + temporal staking (triple cost per attack)
- **Deterministic extraction** — single pipeline in `search-common` so all contributors produce identical metadata hashes

## Development

Prerequisites: Rust with `wasm32-unknown-unknown` target, [Dioxus CLI](https://dioxuslabs.com) (`dx`), a running [Freenet](https://github.com/freenet/freenet-core) node.

```bash
# Run all tests (149 tests across 21 test files)
cargo test --workspace

# Clippy (zero warnings policy)
cargo clippy --workspace --all-targets -- -D warnings

# Format check
cargo fmt --all --check

# WASM build check
cargo check -p freenet-search-engine --target wasm32-unknown-unknown

# Dev server (connects to Freenet node at ws://127.0.0.1:7509)
dx serve

# Release build
dx build --release
```

Smoke tests are available in `tests/`:
- `tests/check-wasm-build.sh` — verifies WASM targets compile
- `tests/check-clippy.sh` — runs clippy with `-D warnings`

## Deployment

### Local (single node)

```bash
scripts/deploy-local.sh
```

Deploys both contracts and the web app to a local Freenet node.

### Network (multi-node)

```bash
scripts/network-start.sh        # Start a local Freenet network
scripts/deploy-network.sh       # Deploy contracts and web app
scripts/redeploy-webapp.sh      # Redeploy just the web app after UI changes
scripts/network-stop.sh         # Stop the network
```

Test apps for populating the index:

```bash
tests/integration/publish-test-apps.sh
```

## Design Document

Full specification: [docs/plans/2026-02-11-contract-based-architecture-design.md](docs/plans/2026-02-11-contract-based-architecture-design.md)

## License

LGPL-2.1 — see [LICENSE](LICENSE)
