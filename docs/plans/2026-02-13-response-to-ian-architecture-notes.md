# Response to Ian Clarke's Architecture Notes & Action Plan

**Date**: 2026-02-13
**Context**: Ian Clarke published [architecture notes](https://gist.github.com/sanity/dd9878de28b87525cb679c6482f2a132) for the Freenet search engine. This document compares his vision against our deployed v1, identifies gaps, and proposes concrete next steps for both our project and upstream Freenet.

---

## Part 1: Critical Analysis — Our Architecture vs Ian's Notes

### Where We Align

| Topic | Ian's Note | Our v1 Status |
|-------|-----------|---------------|
| Index contracts | Shared replicated catalogs | Deployed: 1 catalog + 16 fulltext shards, CRDT merge, bloom filter sync |
| Subscription-based updates | Subscribe to index contracts | Deployed: catalog + shard subscriptions via WebSocket |
| Reputation-weighted contributions | Weight by contributor trust | Deployed: `trust_score` incremented on confirmation, `weight = 1 + trust_score` |
| Stop scraping diagnostics | Discovery should be network-based | Partially done: consumers read catalog; contributors still poll diagnostics |

Ian acknowledges this: *"The existing prototype's data layer (catalog contracts, inverted index shards, contribution pipeline) is a solid foundation."*

### Where We Diverge

#### 1. Anti-Sybil: PoW Antiflood vs Ghost Keys

**Us**: PoW tokens (difficulty 16) + ed25519 signatures + temporal staking + reputation.
**Ian**: Ghost Keys (donation-backed blind-signed Ed25519 identities).

**Ian is right** that PoW is weak — botnets farm it cheaply. Ghost Keys impose a flat monetary cost per identity ($5-$20 donation), hardware-independent.

**But**: Our PoW + temporal staking are *complementary*, not replaced. Ghost Keys handle identity creation cost; PoW adds per-submission cost; temporal staking prevents burst attacks. The optimal defense is all three layers.

**Key technical insight**: `ghostkey_lib` produces Ed25519 keypairs (`ed25519_dalek::VerifyingKey`), same as our current delegate. Our `Attestation.contributor_pubkey: [u8; 32]` is already compatible. Migration = adding the certificate chain alongside existing pubkey, not replacing the crypto.

#### 2. Ranking: Global Attestation Count vs Personalized Web of Trust

**Us**: Global passive ranking = `w1*attestations + w2*version + w3*subscribers + status_bonus`.
**Ian**: Personalized ranking via transitive trust graphs — results filtered through *your* web of trust.

**Ian is right** that personalized WoT ranking is strictly more manipulation-resistant. An attacker must infiltrate your specific trust graph, not just accumulate global attestations.

**But**: Web of Trust does not exist. There is no WoT contract, no trust-link protocol, no social graph infrastructure in Freenet. Ian's [Proof of Trust article](https://freenet.org/news/799-proof-of-trust-a-wealth-unbiased-consensus-mechanism-for-distributed-systems/) describes a conceptual game-theoretic mechanism for establishing trust — but it has no implementation.

**Our approach**: Keep global ranking as fallback, design attestation format to carry WoT proofs when available.

#### 3. Discovery: Diagnostics Polling Privacy Leak

**Ian is right.** Our `node_api.rs:141` calls `NodeDiagnosticsConfig::full()` every 10 seconds, requesting peer addresses, topology, subscriber IDs — far more than we use. Any malicious webpage on `localhost` can do the same (no auth on WS API, see issue [#3008](https://github.com/freenet/freenet-core/issues/3008)).

We only consume `contract_states` for discovery. We should stop requesting the rest immediately, and phase out diagnostics polling for contributors when upstream provides a privacy-respecting alternative.

#### 4. AI Embeddings

**Ian proposes** semantic search via vector embeddings (WASM or delegate-based) as a stretch goal.

**Defer.** Index has <100 entries. TF-IDF is adequate. Embedding models would add 50MB+ to WASM bundle. Revisit when index > 1000 entries.

### What We Have That Ian's Notes Don't Account For

- **Full-text inverted index** with integer TF-IDF across 16 shards — this is operational, not just planned
- **Deterministic extraction pipeline** in shared `search-common` crate with property-based tests — ensures all nodes produce identical `metadata_hash`
- **Opt-in contribution pipeline** with antiflood + signing + submission already working
- **149 tests across 21 test files**, including CRDT commutativity, anti-Sybil, and bloom filter round-trip tests
- **Content-aware deduplication** for vanity-nonce redeployments
- **Deployed and live** on the Freenet network (18 contracts, vanity webapp ID)

---

## Part 2: Action Items for the Project

### Action 1: Minimize Diagnostics Exposure [HIGH priority, SMALL effort]

**File**: `ui/src/api/node_api.rs`
- Line 143: Replace `NodeDiagnosticsConfig::full()` with minimal config requesting only `contract_states` (check freenet-stdlib for builder methods; if none exist, file upstream issue)
- Line 27: Change `POLL_INTERVAL_MS` from 10,000 to 60,000 (10s → 60s)
- Add code comment documenting why we still poll and when we plan to stop

### Action 2: Extend Attestation for Ghost Keys & Signatures [HIGH priority, MEDIUM effort]

**File**: `crates/search-common/src/types.rs` — Modify `Attestation`:
```rust
pub struct Attestation {
    pub contributor_pubkey: [u8; 32],        // stays same (GhostKey is also Ed25519)
    pub antiflood_token: AntifloodToken,
    pub token_created_at: u64,
    pub weight: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Vec<u8>>,            // Ed25519 sig over metadata_hash
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ghostkey_certificate: Option<Vec<u8>>, // CBOR-serialized GhostkeyCertificateV1
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extensions: Option<BTreeMap<String, Vec<u8>>>, // future WoT proofs, etc.
}
```

**Why `serde(default)`**: Backward-compatible CBOR. Old attestations without these fields deserialize to `None`. No contract key change needed — only a WASM code update.

**Why `Vec<u8>` for certificate**: Avoids coupling `search-common` (compiled to WASM for contracts) to `ghostkey_lib`'s RSA dependencies, which may not compile to `wasm32-unknown-unknown`. Certificate is opaque bytes; verification happens where the lib is available.

### Action 3: Add Signature Verification in Catalog Contract [HIGH priority, MEDIUM effort]

**File**: `crates/contract-catalog/src/lib.rs`
- In delta validation: when `signature` is `Some`, verify Ed25519 signature over `metadata_hash` using `contributor_pubkey`. Reject invalid signatures.
- When `signature` is `None`, accept (transition period) but at base weight only.
- Add `ed25519-dalek` dependency to contract crate.

**File**: `crates/contract-catalog/Cargo.toml`
- Add `ed25519-dalek = { version = "2.1", default-features = false }` (no-std for WASM)

### Action 4: Ghost Key Certificate Verification in Contract [HIGH priority, LARGE effort]

**WASM compatibility concern**: `ghostkey_lib` depends on `blind-rsa-signatures` (RSA operations). RSA may NOT compile to `wasm32-unknown-unknown`. Two approaches:

**Option A (preferred)**: Extract verification-only logic. The contract only needs to verify the certificate chain, not generate keys. Write a minimal verification function that:
1. Deserializes `GhostkeyCertificateV1` from CBOR bytes
2. Checks Ed25519 signature (master → delegate payload) using `ed25519-dalek` (WASM-compatible)
3. Checks RSA blind signature (delegate → ghostkey) — this is the hard part for WASM

If RSA verification doesn't compile to WASM: **Option B**: Defer in-contract Ghost Key verification. Instead, the UI (native WASM with full `ghostkey_lib`) verifies certificates before display. Contracts accept attestations with or without certificates; the UI shows "Verified Ghost Key" badge for verified ones. This is weaker (contracts can't reject fake certificates) but unblocked.

**File changes** (Option A):
- `crates/contract-catalog/Cargo.toml`: Add `ghostkey_lib` with minimal features
- `crates/contract-catalog/src/lib.rs`: In delta validation, when `ghostkey_certificate` is `Some`, deserialize and verify chain. Verified Ghost Key attestations get 2x weight multiplier.

**File changes** (Option B, fallback):
- `ui/Cargo.toml`: Add `ghostkey_lib = "0.1.4"`
- `ui/src/views/app_card.rs`: Show "Ghost Key Verified" badge when certificate validates

### Action 5: Ghost Key UI Flow [MEDIUM priority, MEDIUM effort]

**File**: `ui/src/views/settings.rs`
- Add "Ghost Key" section: status display ("Linked" / "Not linked — basic identity")
- Import flow: user pastes or loads their Ghost Key certificate (received from donation service)
- Store certificate chain in localStorage alongside existing keypair

**File**: `ui/src/api/contribution.rs`
- When creating attestations, include `ghostkey_certificate` and `signature` fields if Ghost Key is linked
- Sign `metadata_hash` with Ghost Key's Ed25519 signing key

### Action 6: Prepare for WoT Ranking [MEDIUM priority, SMALL effort]

The `extensions: Option<BTreeMap<String, Vec<u8>>>` field (Action 2) handles future WoT data. Additionally:

**File**: `ui/src/search/ranking.rs`
- Add placeholder for WoT-based scoring: `wot_score: Option<u32>` in ranking computation
- When `None` (no WoT available), fall back to current global ranking
- When `Some`, use it as primary signal (WoT overrides global attestation count)

### Action 7: Contract Versioning for This Migration [SMALL effort]

All Attestation changes use `#[serde(default)]` — **no `protocol_version` bump needed**. The new WASM code understands both old and new formats. Deploy as a code-only update to existing contract keys.

**When to bump version**: Only when we want to *require* Ghost Keys (setting `require_ghostkey: true` in CatalogParameters). This creates new contract keys and loses existing data. Do this only after Ghost Key minting service is operational.

---

## Part 3: Issues for Freenet Network (Upstream)

### CRITICAL: State Streaming Reliability [BLOCKER]

Fragment assembly failures prevent remote nodes from fetching contract state. Contracts propagate (metadata spreads) but `GET` returns errors when state must be assembled from fragments.

**Evidence**: Dummy 224-byte webapp published locally, confirmed in VPS diagnostics, but `curl` from VPS hangs indefinitely. Service reports: `MEMV3Q` (local), `M2CR2E` (VPS).

**This blocks everything.** Without reliable state fetching, new users cannot load the search index. All our contract infrastructure is useless if peers can't retrieve the state.

### CRITICAL: WS API Security [#3008](https://github.com/freenet/freenet-core/issues/3008)

Any webpage can connect to `ws://127.0.0.1:7509` and extract diagnostics (peer addresses, contract subscription lists, topology). Needs delegate-based sender attestation as Ian describes.

**Our ask**: At minimum, provide fine-grained `NodeDiagnosticsConfig` so clients can request only contract key lists without peer addresses/topology. Long-term: full delegate-based auth.

### IMPORTANT: Contract Event Subscription API

No way to subscribe to "new contracts appearing on my node" without diagnostics polling. The existing subscription API only works for specific contract keys you already know.

**Our ask**: A `ContractEvent::NewContractObserved { key }` stream that emits when the node first sees a new contract, without exposing peer addresses or subscriber counts. This unblocks privacy-respecting contribution.

### FUTURE: Web of Trust Infrastructure

**Required for personalized ranking**: WoT contract specification (trust links between Ghost Key identities, transitivity rules, revocation), reference implementation, and query protocol.

### FUTURE: Ghost Key Minting Service

`ghostkey_lib` 0.1.4 exists but the donation-based minting flow needs a live service. Until operational, we support Ghost Keys optionally (verified keys get higher weight, unverified accepted at base weight).

### FUTURE: Proof of Trust Implementation

Game-theoretic mechanism from Ian's article. Conceptual stage. Would strengthen WoT by providing economic foundation for trust links.

---

## Implementation Sequence

| # | Action | Blocks On | Breaking? | Deploy |
|---|--------|-----------|-----------|--------|
| 1 | Minimize diagnostics exposure | Nothing | No | UI rebuild |
| 2 | Add `signature`, `ghostkey_certificate`, `extensions` to Attestation | Nothing | No (serde default) | WASM code update |
| 3 | Signature verification in catalog contract | #2 | No | WASM code update |
| 4 | Ghost Key certificate verification | #2, WASM compat check | No | WASM code update |
| 5 | Ghost Key UI flow (import/link) | #4 | No | UI rebuild |
| 6 | WoT ranking placeholder | #2 | No | UI rebuild |
| 7 | Replace diagnostics discovery | Upstream event API | No | UI rebuild |
| 8 | Require Ghost Keys (`protocol_version` bump) | Minting service live | **YES** | New contract keys |

Actions 1-3 can start immediately. Action 4 requires a WASM compatibility spike for `ghostkey_lib`/RSA. Actions 5-6 follow. Action 7 depends on upstream. Action 8 is a future milestone.

## Verification

- All existing 149 tests must continue passing (backward compat of serde changes)
- New tests: Ghost Key certificate round-trip, signature verification, attestation with/without optional fields
- Deploy to local network (`freenet local`) and verify old+new attestation formats coexist
- WASM compilation check: `cargo build --target wasm32-unknown-unknown -p contract-catalog` with `ghostkey_lib` dep
