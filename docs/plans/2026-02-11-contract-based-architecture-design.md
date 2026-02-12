# Freenet Search Engine — Contract-Based Architecture Design

**Date**: 2026-02-11
**Status**: Reviewed — post-review v2
**Review**: 3-agent design review completed (architecture, security, UX/performance)

## Context

The Freenet Search Engine is currently a client-side WASM app (Dioxus 0.7) that connects to a local Freenet node, discovers contracts via polling diagnostics, classifies them, extracts metadata, and caches everything in browser localStorage. Every user repeats the full discovery/indexation work independently.

This design transitions the architecture to a fully decentralized model where:
- The search index is a shared Freenet contract, contributed to by all participants
- Full-text search is supported via a sharded inverted index on Freenet
- Ranking uses passive metrics + contributor reputation (v1), with active community voting deferred to v2
- The app itself is served as a Freenet WebApp contract

**Prerequisites**: A running local Freenet node + a web browser. Nothing else.

## Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope v1 | Shared index + full-text + passive ranking + reputation | Maximum impact, community voting deferred |
| Active ranking (votes) | Deferred to v2 | Sybil/identity problems are a project in themselves |
| Contract structure | Separated by domain, sharded | Independent evolution, granular updates |
| Full-text sharding | Hash-based, configurable shard count via Parameters | Uniform distribution, O(1) lookup |
| Contribution model | Opt-in with reputation system | Quality incentivized, contributor trust earned over time |
| Quorum matching | ContractKey + metadata_hash, weighted by contributor trust | Prevents poisoning, rewards reliable contributors |
| Anti-Sybil | Antiflood tokens + crypto signatures + temporal staking | Triple cost per attack (computation + identity + time) |
| App serving | SearchEngine is a Freenet WebApp contract | Dogfooding, single distribution channel |
| Indexed content | Title + description + visible HTML text (snippet) | Google-style: rich search without index bloat |
| Extraction pipeline | Single deterministic pipeline in shared crate | Mandatory for quorum hash convergence |
| Merge strategy | CRDT grow-only map (all hash variants kept, client resolves) | Provably commutative, Freenet-compliant |
| Entry lifecycle | Pending → Confirmed → Disputed → Expired (TTL + contestation) | Defense in depth, natural cleanup |
| Sync protocol | Bloom filter summaries for `get_state_delta` | Compact, efficient for grow-only CRDTs |
| Serialization | CBOR | Self-describing, schema-evolution friendly |
| Caching | localStorage L1 + Freenet node L2 + network L3 | Instant page loads + persistent cache |
| Cold start | Catalog first, shards pre-fetched progressively in background | Immediate browsing, search available quickly |
| Dev/test environment | Isolated local network (`freenet local` + `local-network.mk`) | Zero network pollution |

## 1. Contract Architecture

### 1.1 Freenet ContractInterface Compliance

All contracts implement the 4 methods of `ContractInterface`:
- **`validate_state`**: Validates the complete state after any modification
- **`update_state`**: Receives `Vec<UpdateData>`, applies deltas, performs ALL input validation (rejects invalid deltas by returning error). There is no separate `validate_delta` method in Freenet.
- **`summarize_state`**: Returns a compact `StateSummary` (bloom filter) for P2P sync
- **`get_state_delta`**: Given the local state and a peer's `StateSummary`, computes the minimal `StateDelta` to send

**Commutativity requirement**: All `update_state` implementations MUST be commutative — applying deltas in any order produces the same final state. This is achieved through CRDT semantics (grow-only maps, max-counters).

**Parameters**: Configurable values set at contract deployment time, part of the contract key derivation:
- `protocol_version: u16`
- `shard_count: u8` (for FullTextShard)
- `confirmation_weight_threshold: u32` (weighted quorum threshold)
- `entry_ttl_days: u16` (TTL for re-confirmation)

### 1.2 SearchCatalog (single contract)

The app catalog. One contract, not sharded (monitoring for growth; sharding planned if > 10K entries, ~15MB threshold).

**State structure** (per indexed contract):
```
CatalogEntry {
    contract_key: String,               // key of the indexed WebApp contract
    hash_variants: BTreeMap<[u8; 32], HashVariant>,  // all submitted metadata variants
    size_bytes: u64,                    // full contract state size
    version: Option<u64>,              // from CBOR metadata
    status: Status,                     // derived from best variant's state
    first_seen: u64,                   // min across all attestations
    last_seen: u64,                    // max across all attestations
}

HashVariant {
    title: String,                      // extracted from HTML (max 256 chars)
    description: String,                // from meta tags (max 1024 chars)
    mini_snippet: String,               // visible HTML text (max 300 chars, for browsing display)
    attestations: Vec<Attestation>,     // all attestations for this variant
    total_weight: u32,                  // sum of attestation weights (cached)
}

Attestation {
    contributor_pubkey: [u8; 32],       // ed25519 public key from delegate
    antiflood_token: AntifloodToken,    // proof-of-work token
    token_created_at: u64,             // timestamp of token generation (temporal staking)
    weight: u32,                       // 1 + contributor's trust_score at submission time
}

enum Status {
    Pending,      // total_weight < confirmation_threshold
    Confirmed,    // total_weight >= confirmation_threshold
    Disputed,     // competing variant has significant weight
    Expired,      // TTL exceeded without re-confirmation
}
```

**Contributor Reputation** (stored in catalog state):
```
ContributorScore {
    pubkey: [u8; 32],
    trust_score: u32,    // starts at 0, incremented when contributed entries reach Confirmed
    total_contributions: u32,
}
```

When a contributor's entry reaches `Confirmed`, their `trust_score` is incremented. Future attestations from this contributor carry `weight = 1 + trust_score`, accelerating confirmation of entries from trusted sources.

**CRDT Merge semantics** (commutative):
- `CatalogEntry` for a given `contract_key`: union of all `hash_variants`
- `HashVariant` for a given hash: union of all `attestations` (deduplicated by `contributor_pubkey`)
- `total_weight`: recomputed as sum of attestation weights (deterministic from attestation set)
- `status`: derived deterministically from `total_weight` vs threshold and competing variants
- `first_seen`: min across all attestations — `last_seen`: max
- `ContributorScore.trust_score`: max across all replicas (monotonically increasing)

**Contract methods**:
- **`validate_state`**: Verifies CBOR deserialization, all entries have non-empty `contract_key`, `metadata_hash` matches recomputed hash from fields, attestation weights are consistent with contributor scores, no duplicate pubkeys per hash variant
- **`update_state`**: Receives `Vec<UpdateData>`, iterates batch. For each delta:
  - Validates: `contract_key` format, field size limits, `metadata_hash` matches submitted data, antiflood token is valid, temporal staking minimum met (token age >= configured minimum), contributor signature is valid
  - Rejects invalid deltas with `ContractError::InvalidUpdate`
  - New entry (unknown key): creates `CatalogEntry` with single `HashVariant`, `attestations: [submitted]`
  - Existing entry, matching hash: adds attestation to existing `HashVariant` (dedup by pubkey)
  - Existing entry, different hash: adds new `HashVariant` to `hash_variants` map
  - Recomputes `status` and `total_weight` for affected entry
  - Updates `ContributorScore` if any variant crosses confirmation threshold
- **`summarize_state`**: Returns bloom filter encoding `(contract_key, best_hash, total_weight)` tuples
- **`get_state_delta`**: Tests each local entry against peer's bloom filter, returns entries likely missing from peer

**Contestation mechanism**: When two `HashVariant`s for the same `contract_key` both have significant weight (e.g., secondary variant weight > 30% of primary), the entry status becomes `Disputed`. The UI displays both variants with their respective attestation counts.

**TTL / Expiry**: Entries whose `last_seen` is older than `entry_ttl_days` (from Parameters) transition to `Expired`. Expired entries are excluded from search results but kept in state for historical reference. Re-attestation resets the TTL.

> **Open question (time trust)**: Freenet contracts have no trusted clock. The `token_created_at` and `last_seen` values come from submitting nodes and can be spoofed. Mitigation options to investigate during implementation: contract-update sequence numbers, Freenet-provided timestamps in `UpdateData` context, or relative time (delta between token creation and submission).

### 1.3 FullTextShard[0..N-1] (N contracts, e.g., 16)

The inverted index, partitioned into N shards by keyword hash.

**State structure** (per shard):
```
ShardState {
    shard_id: u8,
    index: BTreeMap<String, Vec<TermEntry>>,  // word → list of matching contracts
}

TermEntry {
    contract_key: String,
    snippet: String,        // full visible HTML text (max 2000 chars, for search result display)
    tf_idf_score: u32,     // integer-scaled TF-IDF (×10000) — deterministic, no floats
}
```

**Note on snippets**: The full snippet (2000 chars) lives here in the shards, not in the catalog. The catalog only stores a `mini_snippet` (300 chars) for browsing display. This avoids data duplication while keeping both browsing and search UX rich.

**Note on scoring**: TF-IDF scores use integer arithmetic (score × 10000, rounded) to guarantee determinism across all WASM runtimes. No floating-point in any cross-node data.

**Shard assignment**: `shard_id = hash(normalized_word) % N`, where N comes from contract Parameters.

**CRDT Merge semantics** (commutative):
- `index` for a given word: union of all `TermEntry` (deduplicated by `contract_key`)
- For duplicate `contract_key` per word: keep entry with highest `tf_idf_score` (max-wins)

**Contract methods**:
- **`validate_state`**: Valid CBOR, all words hash to this shard, no duplicate `contract_key` per word
- **`update_state`**: Validates each delta (word hashes to correct shard, `contract_key` format valid, antiflood token valid). Merges into index with CRDT semantics. Rejects invalid deltas.
- **`summarize_state`**: Bloom filter of `(word, contract_key)` pairs
- **`get_state_delta`**: Entries present locally but absent from peer's bloom filter

### 1.4 SearchEngine (WebApp contract)

The search engine app itself, deployed as a Freenet web container.

**Contents**: Standard web container format:
```
[metadata_size: u64 BE][CBOR metadata][web_size: u64 BE][tar.xz of WASM app]
```

The tar.xz contains: `index.html`, compiled WASM, CSS, JS glue code.

**Vanity Contract ID**: The app uses a vanity nonce to produce a human-readable contract ID prefix (e.g. `FinderTZGH...`). The 8-byte nonce is appended to the 32-byte Ed25519 verifying key in `webapp.parameters` (40 bytes total). The `web_container_contract` ignores bytes after the first 32, so the nonce is transparent to the contract runtime. See `freenet-vanity-id` for the grinder tool and deploy workflow.

**Deployment**: The deploy scripts (`scripts/deploy-live.sh`, `scripts/redeploy-webapp.sh`) handle vanity nonce preservation across sign steps. The signing tool regenerates `webapp.parameters` to exactly 32 bytes each time, so the nonce must be appended *after* every sign step. The scripts use `head -c 32` to extract the verifying key before appending the nonce, preventing double-nonce corruption.

### 1.5 Delegate (user identity & keys)

The delegate manages the user's cryptographic identity for contributions:
- Generates and stores an ed25519 keypair
- Signs attestations before submission
- Tracks the user's contribution history locally
- Will be extended for voting in v2

## 2. Anti-Sybil & Spam Prevention

Three-layer defense, each raising the cost of attack:

### 2.1 Antiflood Tokens (Layer 1: Computation cost)
Each submission to SearchCatalog or FullTextShard requires a valid antiflood token (Freenet's native proof-of-work mechanism). Cost: significant CPU time per token.

### 2.2 Cryptographic Signatures (Layer 2: Identity cost)
Each attestation is signed with the contributor's ed25519 key (managed by delegate). The contract deduplicates attestations by public key — one attestation per pubkey per `(contract_key, metadata_hash)`. An attacker must generate a new keypair per fake attestation.

### 2.3 Temporal Staking (Layer 3: Time cost)
Antiflood tokens include a creation timestamp. The contract requires a minimum age (e.g., 1 hour) between token generation and submission. An attacker must pre-generate tokens hours in advance, preventing burst attacks.

> **To validate during implementation**: Whether Freenet provides a trustworthy reference for "current time" in contract execution context. If not, temporal staking may need to rely on contract-update sequence numbers or relative ordering instead of absolute timestamps.

### 2.4 Contributor Reputation (Layer 4: History cost)
New contributors start with `trust_score = 0` (attestation weight = 1). Trust is earned only when contributed entries reach `Confirmed` status — which itself requires multiple independent attestations. Building reputation is slow and expensive for attackers but automatic for honest participants.

**Combined attack cost**: To confirm a single malicious entry, an attacker needs:
- `confirmation_threshold` antiflood tokens (CPU time each)
- `confirmation_threshold` distinct keypairs
- Each token pre-generated at least 1 hour before use
- OR fewer tokens if the attacker has built legitimate trust first (which requires genuine contributions)

## 3. Contribution Flow

Contribution is **opt-in** with reputation incentives: contributors who submit entries that later reach `Confirmed` earn trust points, making their future attestations carry more weight.

### 3.1 Discovery & Submission Pipeline

```
User enables contribution in settings
    │
    ▼
Node discovers new contract (via diagnostics polling)
    │
    ▼
Check SearchCatalog: is this contract_key already indexed?
    ├── Yes, with our hash → submit attestation only
    ├── Yes, different hash → submit our variant (may create Disputed status)
    └── No → full submission (new entry)
    │
    ▼
GET contract state (raw bytes)
    │
    ▼
Detect WebApp format (metadata_size + web_size validation)
    │
    ▼
Extract metadata via DETERMINISTIC pipeline (search-common crate):
    ├── Decompress XZ tar from raw state bytes (NEVER from HTTP fallback)
    ├── Extract index.html from tar
    ├── Parse title: <title> > og:title > application-name > <h1>
    ├── Parse description: <meta description> > og:description
    ├── Extract snippet: visible text, strip tags/scripts/styles, first 2000 chars
    ├── Extract mini_snippet: first 300 chars of snippet
    ├── Normalize all text: trim, collapse whitespace, UTF-8 NFC normalization
    └── Hash: sha256(canonical_title + "\0" + canonical_description + "\0" + canonical_snippet)
    │
    ▼
Generate antiflood token (proof-of-work, may take seconds)
    │
    ▼
Sign attestation with delegate keypair
    │
    ▼
Submit to SearchCatalog:
    ContractRequest::Update { key: catalog_key, delta: CatalogDelta { entry, attestation, token } }
    │
    ▼
Tokenize snippet for full-text indexing:
    ├── Split into words
    ├── Normalize (lowercase, strip accents, remove stop words)
    ├── Calculate integer TF-IDF scores (×10000)
    └── Group by shard: hash(word) % N
    │
    ▼
Submit to FullTextShard[i] for each relevant shard:
    ContractRequest::Update { key: shard_keys[i], delta: ShardDelta { entries, token } }
```

### 3.2 Deterministic Extraction Invariant

All nodes running the same `search-common` crate version on the same contract state bytes MUST produce the same `metadata_hash`. Enforced by:
- Raw contract state bytes as sole input (never HTTP fallback)
- Fixed parsing priority order, specified as a byte-level algorithm
- Canonical text normalization (Unicode NFC, lowercase, whitespace collapse with exact whitespace definition: U+0020 only)
- Integer-only arithmetic (no floating-point in hash-relevant computations)
- Pinned dependency versions with exact features/build flags
- Extensive property-based tests with adversarial HTML (malformed tags, mixed encodings, BOM markers, combining characters)

## 4. Search Flow

When a user types a query:

```
User types query: "freenet chat app"
    │
    ▼
Tokenize + normalize: ["freenet", "chat", "app"]
    (same normalization as indexing: lowercase, strip accents, remove stop words)
    │
    ▼
Determine shards needed:
    shard("freenet") = hash("freenet") % N → shard 3
    shard("chat")    = hash("chat") % N    → shard 7
    shard("app")     = hash("app") % N     → shard 12
    │
    ▼
Fetch shards (parallel ContractRequest::Get):
    GET FullTextShard[3], FullTextShard[7], FullTextShard[12]
    (use cached version if available + Freenet subscription for updates)
    │
    ▼
Retrieve posting lists:
    "freenet" → [{key: A, score: 8000}, {key: B, score: 3000}]
    "chat"    → [{key: A, score: 6000}, {key: C, score: 9000}]
    "app"     → [{key: A, score: 4000}, {key: B, score: 5000}, {key: D, score: 7000}]
    │
    ▼
Score + rank:
    ├── Intersect/union based on strategy (default: OR with boost for AND matches)
    ├── Combined score = sum of per-term integer scores
    ├── Tiebreaker: passive ranking (weighted attestations > version > subscribers)
    └── Status weighting: Confirmed boost, Pending neutral, Disputed penalty, Expired hidden
    │
    ▼
Enrich from SearchCatalog:
    GET SearchCatalog (cached + subscribed)
    Match contract_keys → title, description, mini_snippet, status
    Retrieve full snippet from shard results for display
    │
    ▼
Display results:
    ├── Title + snippet with highlighted matching terms
    ├── Metadata: version, subscribers, size
    ├── Trust indicator: Confirmed ✓ | Pending ⏳ | Disputed ⚠
    └── "Open" link → /v1/contract/web/{contract_key}/
```

**Partial results**: If some shards are unavailable, results are displayed with a "partial results" indicator. Available shards contribute results immediately; missing shards are retried in background.

## 5. Crate / Project Structure

```
freenet-search-engine/
├── crates/
│   ├── search-common/              # Shared deterministic library
│   │   ├── src/
│   │   │   ├── extraction.rs       # HTML parsing, metadata extraction
│   │   │   ├── normalization.rs    # Text normalization, canonicalization
│   │   │   ├── tokenization.rs     # Word splitting, stop words, stemming
│   │   │   ├── hashing.rs          # metadata_hash, shard assignment
│   │   │   ├── scoring.rs          # Integer TF-IDF / BM25 calculation
│   │   │   ├── bloom.rs            # Bloom filter for StateSummary
│   │   │   └── types.rs            # Shared types (CatalogEntry, Status, TermEntry, etc.)
│   │   └── Cargo.toml              # no_std compatible, WASM-friendly, pinned deps
│   │
│   ├── contract-catalog/           # SearchCatalog Freenet contract
│   │   ├── src/lib.rs              # validate_state, update_state, summarize_state, get_state_delta
│   │   └── Cargo.toml              # depends on search-common, freenet-stdlib
│   │
│   ├── contract-fulltext-shard/    # FullTextShard Freenet contract
│   │   ├── src/lib.rs              # validate_state, update_state, summarize_state, get_state_delta
│   │   └── Cargo.toml              # depends on search-common, freenet-stdlib
│   │
│   └── delegate-identity/          # Delegate for managing user identity/secrets
│       ├── src/lib.rs              # ed25519 keypair management, attestation signing
│       └── Cargo.toml              # depends on freenet-stdlib
│
├── ui/                             # SearchEngine WebApp (Dioxus 0.7 WASM)
│   ├── src/
│   │   ├── main.rs                 # App entry, layout
│   │   ├── state.rs                # Global signals
│   │   ├── api/
│   │   │   ├── mod.rs              # Init orchestration
│   │   │   ├── node_api.rs         # WebSocket connection
│   │   │   ├── contracts.rs        # NEW: Read/subscribe to index contracts
│   │   │   └── contribution.rs     # NEW: Submit discoveries to index (opt-in)
│   │   ├── search/                 # NEW: Search logic
│   │   │   ├── query.rs            # Query parsing, shard routing
│   │   │   └── ranking.rs          # Result scoring + passive ranking + reputation
│   │   ├── discovery/              # Contract discovery & metadata extraction
│   │   │   ├── title.rs            # HTML title/description extraction, update_catalog_entry (with extracted flag)
│   │   │   ├── cache.rs            # localStorage L1 cache (versioned, auto-clearing)
│   │   │   ├── http_fallback.rs    # HTTP fallback for title extraction when XZ decompression fails
│   │   │   └── detector.rs         # WebApp format detection (extracted from node_api.rs)
│   │   └── views/
│   │       ├── app_directory.rs    # REFACTORED: Content-aware dedup, reads from SearchCatalog
│   │       ├── search_bar.rs       # ENHANCED: Full-text search
│   │       ├── search_results.rs   # NEW: Ranked results with snippets + trust indicators
│   │       ├── app_card.rs         # ENHANCED: Trust indicator + reputation badge
│   │       └── settings.rs         # NEW: Contribution toggle, identity management
│   ├── assets/main.css
│   └── Cargo.toml                  # depends on search-common, freenet-stdlib
│
├── docs/
│   └── plans/
│       └── 2026-02-11-contract-based-architecture-design.md  # this file
│
└── Cargo.toml                      # workspace: ui, crates/*
```

## 6. Display, Deduplication & Ranking

### 6.1 Content-Aware Deduplication

Multiple contract keys may correspond to different versions of the same app (e.g. after redeployment with a new vanity nonce). The UI deduplicates entries by lowercase title, keeping the "best" entry per app.

**Ranking for dedup** (in priority order):
1. **Working content** — entries with a non-empty description (extracted from live HTML) are preferred over blank pages. This is the primary signal: if one version actually renders content and another doesn't, the working one wins regardless of other metrics.
2. **Attestation count** — total attestations across all hash variants in the catalog contract (network-wide trust signal).
3. **State size** — larger state = more complete contract content.
4. **Version** — tiebreaker from CBOR metadata.

Subscribers are NOT used for dedup — they only reflect direct peers, not network-wide popularity.

Entries without a title are not deduplicated and appear individually.

### 6.2 Passive Ranking + Reputation (v1)

No active community voting. Ranking is computed client-side from passive metrics and contributor reputation:

**Score formula**:
```
rank_score = w1 * norm(weighted_attestations)
           + w2 * norm(version)
           + w3 * norm(subscribers)
           + status_bonus

where:
  weighted_attestations = sum of attestation weights for best hash variant
  status_bonus = Confirmed: +0.3, Pending: 0.0, Disputed: -0.2, Expired: excluded
```

Weights are tunable (initial: w1=0.4, w2=0.3, w3=0.3).

**Combined search score**: `final_score = relevance_score * 0.7 + rank_score * 0.3`

The relevance score comes from integer TF-IDF. Ranking is a tiebreaker, not dominant.

**Note on ranking inputs**: `subscribers` is self-reported and spoofable. It is included as a weak signal but weighted less than attestations (which are cryptographically verified). Timestamps (`first_seen`, `last_seen`) are NOT used in ranking due to lack of trusted time source — they are displayed for informational purposes only.

## 7. Subscription & Caching Strategy

### 7.1 Cache Hierarchy

```
L1: localStorage (instant, survives page refresh)
L2: WASM in-memory (instant, lost on page refresh, updated by subscriptions)
L3: Freenet node cache (fast, persists across sessions)
L4: Network fetch (slow, only when node doesn't have the contract)
```

localStorage is preserved as L1 for instant page loads. On startup: load from localStorage immediately, then subscribe to contracts and update localStorage when fresh state arrives.

**Cache versioning**: The L1 cache includes a `CACHE_VERSION` integer. When the `AppEntry` schema changes, the version is bumped and stale caches are automatically cleared on next load.

**Stale entry clearing**: When fresh metadata extraction produces no title or description (e.g. blank-page apps), the cached values must be overwritten — not preserved. The `update_catalog_entry` function takes an `extracted: bool` flag: when `true` (fresh extraction from contract state), title and description are always overwritten (even with `None`); when `false` (cache-only update), existing values are preserved. This prevents stale metadata from a previous contract version persisting indefinitely.

### 7.2 Startup Sequence

```
Page load
    │
    ▼
Load SearchCatalog from localStorage (L1) → display immediately
    │
    ▼
Connect WebSocket to local Freenet node
    │
    ▼
GET + Subscribe to SearchCatalog contract
    │ (update display when fresh state arrives)
    ▼
Begin background pre-fetch of FullTextShard[0..N-1]:
    ├── Fetch shards one by one (staggered, not all at once)
    ├── Each fetched shard is subscribed to and cached in localStorage
    └── Search becomes progressively available as shards load
    │
    ▼
Ready: all shards cached, search fully available
    (subsequent visits: all shards loaded from localStorage instantly)
```

### 7.3 Subscription Updates

- The Freenet node pushes state updates via WebSocket when subscribed contracts change
- Updates refresh L2 (in-memory) and L1 (localStorage) caches
- UI re-renders reactively via Dioxus signals

## 8. Development & Testing Strategy

### 8.1 Local Environment

All development uses isolated local networks. Zero pollution of the public Freenet network.

**Tools**:
- `freenet local` — Single isolated node for contract development
- `local-network.mk` — Multi-node local network (configurable gateways + nodes)
- `freenet-test-network` — Programmatic Rust test harness with Docker NAT backend

### 8.2 Testing Phases

1. **Unit tests** (search-common crate): Extraction determinism, normalization, tokenization, hashing, integer scoring. Property-based tests with adversarial inputs (malformed HTML, mixed encodings, BOM markers, combining characters, bidirectional text) to verify all nodes produce identical output.

2. **Contract tests** (contract-catalog, contract-fulltext-shard):
   - `validate_state` correctness
   - `update_state` with valid and invalid deltas (rejection of bad inputs)
   - CRDT commutativity: apply same set of deltas in randomized orders, verify identical final state
   - Quorum mechanics: confirmation threshold, dispute detection, TTL expiry
   - Antiflood token validation
   - `summarize_state` + `get_state_delta` round-trip: verify sync produces convergence
   - `UpdateData` batch semantics (multiple deltas per call)

3. **Integration tests** (multi-node via `freenet-test-network`):
   - Deploy contracts on local network
   - Simulate N nodes contributing to index, verify convergence
   - Verify N nodes extracting metadata from same contract produce identical `metadata_hash`
   - Test subscription propagation: one node updates, others receive
   - Test dispute scenarios: conflicting metadata from different nodes

4. **End-to-end tests**: Full flow from app discovery → contribution → search → result display. Browser-based with the WASM app running against local network.

### 8.3 Contract Versioning

- `protocol_version` stored in contract `Parameters` (part of key derivation)
- Breaking changes = new Parameters = new contract key = clean deployment
- The app reads `protocol_version` from Parameters and only interacts with compatible contracts
- Discovery of latest contract keys: well-known Parameters derivation (same code + version N = deterministic key)

## 9. Migration Path

### Phase 1: Foundation
- Create workspace structure with `search-common` crate
- Extract deterministic pipeline from current `discovery/title.rs` into `search-common`
- Implement integer scoring (no floats)
- Implement bloom filter for `StateSummary`
- Unit test extraction determinism exhaustively (property-based tests)
- `no_std` compatibility for shared crate

### Phase 2: Contracts
- Implement `SearchCatalog` contract (CRDT merge, antiflood validation, reputation tracking)
- Implement `FullTextShard` contract (CRDT merge, shard validation)
- Implement `delegate-identity` (ed25519 key management, attestation signing)
- Test CRDT commutativity with randomized delta ordering
- Test on single local node with `fdev`
- Deploy on multi-node local network, verify sync convergence

### Phase 3a: Contract Reading
- Refactor UI to read from `SearchCatalog` for browsing (replacing local-only discovery)
- Keep local diagnostics polling as fallback / discovery source for contribution
- Implement subscription to SearchCatalog + progressive shard pre-fetch
- Implement localStorage L1 cache for contracts
- Adapt views: trust indicators (Pending/Confirmed/Disputed), reputation badges

### Phase 3b: Contribution
- Implement opt-in contribution flow (discovery → extraction → antiflood → sign → submit)
- Implement contribution settings UI (enable/disable, identity display, contribution history)
- Test multi-node contribution on local network

### Phase 4: Search
- Implement full-text query parsing and shard routing
- Implement integer scoring (TF-IDF + passive ranking + reputation weighting)
- Build search results UI with full snippets, highlighted terms, trust indicators
- Implement partial results (some shards unavailable)
- Performance testing: measure query latency on local network

### Phase 5: Deployment
- Package the app as a Freenet WebApp contract
- Final integration testing on local network (full cycle: deploy app → discover → contribute → search)
- Deploy contracts to public Freenet network
- Deploy app contract to public Freenet network

## 10. Open Questions

- **Shard count (N)**: Start with 16? Adjustable via Parameters for future versions.
- **Confirmation weight threshold**: Configurable via Parameters. Initial value TBD — depends on expected network participation.
- **Temporal staking minimum**: 1 hour? Needs validation of time trust mechanism in Freenet.
- **Stop words list**: English-only initially? Multi-language in v2.
- **Stemming**: Adds complexity but improves recall. Defer to v2 or include in v1?
- **Contract size limits**: What's the practical max state size for a Freenet contract? Determines shard sizing and catalog sharding threshold.
- **Time trust**: Can Freenet provide a trustworthy timestamp in contract execution context? Critical for temporal staking and TTL.

## 11. Future (v2+)

- **Active ranking**: Community voting with Web of Trust, building on the delegate identity system
- **Multi-language support**: Language detection, per-language stop words and stemming
- **Dynamic sharding**: Auto-split shards when they exceed size thresholds (monitor in v1)
- **Content re-indexing**: Periodic re-crawl to update snippets when apps change
- **Categories/tags**: Structured taxonomy beyond free-text search
- **Catalog sharding**: If catalog exceeds 10K entries, shard by contract_key hash
