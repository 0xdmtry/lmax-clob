# Solana CLOB: LMAX-inspired and in Rust

A Central Limit Order Book backend built in Rust for performance.
Ingests a simulated Solana transaction stream of [Triton Yellowstone gRPC](https://github.com/rpcpool/yellowstone-grpc),
matches orders, persists fills, and exposes
a REST + WebSocket API — with full observability via Prometheus, Grafana, and Jaeger.

### Related Projects:

- [Solana CLOB: 3.6M orders/sec](https://github.com/0xdmtry/sol-clob)
- [Solana Indexer with Kafka and ClickHouse](https://github.com/0xdmtry/dex-indexer)
- [Eigen Graph Streamer with Full Infrastructure Stack](https://github.com/0xdmtry/eigen-graph)
- [ClickHouse Indexer Infrastructure with Kubernetes](https://github.com/0xdmtry/indexer-with-clickhouse)

---

## Table of Contents

- [1: What Is Built](#1-what-is-built)
- [2: Architecture](#2-architecture)
- [3: Data Flow](#3-data-flow)
- [4: Kubernetes — Leader-Follower Cluster](#4-kubernetes--leader-follower-cluster) 🚀
- [5: Observability](#5-observability) 📈
- [6: Performance Benchmarks](#6-performance-benchmarks) 🔥
- [7: Project Structure](#7-project-structure)
- [8: Prerequisites](#8-prerequisites)
- [9: Running](#9-running)
- [10: Environment Variables](#10-environment-variables)
- [11: API](#11-api)
- [12: Debugging](#12-debugging)
- [13: Linting and Formatting](#13-linting-and-formatting)
- [14: Makefile Targets](#14-makefile-targets)

---

## 1: What Is Built

### 1.1: Crates

| Crate              | Role                                                                       |
|--------------------|----------------------------------------------------------------------------|
| `clob-engine`      | Pure domain library: order book, matching engine, snapshots, benchmarks    |
| `clob-api`         | Async HTTP/WS service: ingest, Kafka, persistence, cache, metrics, tracing |
| `yellowstone-mock` | gRPC server that simulates a Solana Yellowstone stream                     |

### 1.2: Infrastructure (Docker)

| Service          | Purpose                                | Port  |
|------------------|----------------------------------------|-------|
| Redpanda         | Kafka-compatible broker                | 19092 |
| Redpanda Console | Topic browser UI                       | 8080  |
| PostgreSQL       | Fill + order persistence               | 5432  |
| Redis            | Best bid/ask cache + recent fills ring | 6379  |
| Prometheus       | Metrics scrape + storage               | 9091  |
| Grafana          | Dashboards                             | 3000  |
| Jaeger           | Distributed traces                     | 16686 |
| Node Exporter    | Host metrics                           | 9100  |
| pgAdmin          | Postgres UI                            | 5050  |
| yellowstone-mock | Simulated Solana gRPC feed             | 10000 |

---

## 2: Architecture

```
yellowstone-mock (gRPC)
  └─► run_ingestor()                    [async, tokio]
        └─► KafkaProducer → orders-in   [Redpanda topic]
              └─► KafkaConsumer         [async, tokio]
                    └─► crossbeam Sender<Order>
                          └─► matching-engine thread  [std::thread]
                                └─► crossbeam Receiver<Fill>
                                      ├─► FillPublisher → fills-out [Redpanda]
                                      ├─► FillsCache   → Redis
                                      ├─► FillWriter   → Postgres (batch 500 / 50ms)
                                      └─► FillBroadcast → WebSocket clients
                                └─► crossbeam Receiver<BookUpdate>
                                      └─► OrderBookCache → Redis
```

### 2.1: Why This Architecture

**Sync matching engine on a dedicated thread.**
The matching engine is single-threaded by design. Order book mutation is inherently
sequential — no benefit from parallelism, and locking overhead would dominate.
A `std::thread` with `crossbeam::bounded` channels gives maximum throughput with
predictable latency.

**crossbeam over tokio channels for the engine boundary.**
The matching thread blocks on `crossbeam_channel::recv()` — a true OS-level block,
not async. This keeps the matching loop off the Tokio executor and prevents head-of-line
blocking on the async runtime.

**Kafka (Redpanda) as the order bus.**
Kafka provides durability and replay. On restart, the consumer seeks to
`snapshot_offset + 1` and replays only the delta — bounded recovery time regardless
of total history.

**Postgres snapshots with Kafka offset.**
The snapshot table stores the serialized order book state and the Kafka offset at the
time of snapshot. This is the recovery anchor point.

**Redis for low-latency reads.**
Best bid/ask is written to Redis on every book update. HTTP handlers read Redis directly
— no DB hit on the hot path.

**postcard for binary serialization.**
Used for Kafka message encoding and snapshots. Faster and smaller than JSON; no schema
overhead unlike protobuf for internal use.

---

### 2.2: LMAX-Inspired Matching Loop

The matching engine follows the LMAX Disruptor pattern:

- **Single writer, single thread.** Only one thread ever mutates the order book.
  No locks, no CAS loops, no contention. Throughput scales with CPU clock speed,
  not core count.
- **Pre-allocated bounded channel.** `crossbeam::bounded(8192)` acts as the ring
  buffer. Back-pressure is explicit: if the engine falls behind, the producer blocks
  rather than allocating unbounded memory.
- **Mechanical sympathy.** The engine thread spins on `recv()` — no async overhead,
  no scheduler interference, no context switches on the hot path.
- **Separation of concerns.** The engine thread only matches. All side effects
  (Kafka publish, Redis write, Postgres persist, WebSocket broadcast) happen on the
  Tokio runtime via separate async tasks reading from a second crossbeam channel.
  The engine never waits on I/O.

This is why `matching_latency_ns` p50 is in the microsecond range while p99 spikes
reflect OS scheduling jitter, not engine logic.

----

## 3: Data Flow

1. `yellowstone-mock` emits a gRPC `SubscribeUpdate`
2. `run_ingestor()` maps it to an `Order` and publishes to `orders-in` via Kafka
3. `KafkaConsumer` deserializes, overwrites `sequence` with Kafka offset, sends to engine
4. Matching engine processes: produces `Fill`s or parks as resting order
5. Fills fan out to: Redpanda (`fills-out`), Redis, Postgres, WebSocket clients
6. Every 10,000 orders: book is serialized and upserted to Postgres `snapshots`

---

## 4: Kubernetes — Leader-Follower Cluster

---

### 4.1: Overview

The Kubernetes setup runs two replicas of `clob-api` as a StatefulSet Kubernetes cluster.
One pod acts as **leader** (full pipeline: ingestor + matching engine + DB + Redis + WS + fills-out), the other as *
*follower** (Kafka replay into in-memory book only — no side effects).

---

### 4.2: Why StatefulSet

StatefulSet gives stable pod names (`clob-0`, `clob-1`) and stable DNS entries via the headless service (
`clob-0.clob-headless`, `clob-1.clob-headless`). This matters because each pod injects its own name as the lease
identity via `POD_NAME` — stable names make failover logs and lease state unambiguous.

A Deployment would work functionally but produces random pod names, making lease holder identification harder to trace.

---

### 4.3: Why Leader Election via Kubernetes Lease

The matching engine is single-threaded and stateful. Two pods running the full pipeline simultaneously would produce
duplicate fills, double DB writes, and split Redis state. Leader election ensures exactly one pod owns the write path at
any time.

Kubernetes `coordination.k8s.io/v1` Leases are the standard primitive for this. The lease has a 15-second TTL. If the
leader stops renewing (pod deleted, crash, OOM), the lease expires and the follower acquires it within one TTL window.

This is the same mechanism used by `kube-controller-manager` and `kube-scheduler` internally.

---

### 4.4: Architecture

```
┌─────────────────────────────────────────────────────┐
│ Kubernetes (Docker Desktop)                         │
│                                                     │
│  StatefulSet: clob (2 replicas)                     │
│                                                     │
│  ┌─────────────┐        ┌─────────────┐             │
│  │   clob-0    │        │   clob-1    │             │
│  │ role=leader │        │role=follower│             │
│  │             │        │             │             │
│  │ ingestor    │        │ kafka replay│             │
│  │ engine      │        │ in-memory   │             │
│  │ DB writes   │        │ book only   │             │
│  │ Redis       │        │             │             │
│  │ WS/fills-out│        │             │             │
│  └──────┬──────┘        └─────────────┘             │
│         │                                           │
│  Service: clob (selector: role=leader)              │
│  → routes HTTP traffic to leader pod only           │
│                                                     │
│  Lease: clob-leader (coordination.k8s.io/v1)        │
│  → held by clob-0, TTL=15s, renewed every 5s        │
└─────────────────────────────────────────────────────┘
```

---

### 4.5: File Structure

```
k8s/
├── 00-rbac.yaml          # ServiceAccount + Role + RoleBinding
├── 01-configmap.yaml     # All env vars
├── 02-service.yaml       # ClusterIP (leader selector) + headless
└── 03-statefulset.yaml   # 2 replicas, probes, resource limits
```

Files are prefixed numerically because `kubectl apply -f k8s/` applies them alphabetically — RBAC must exist before the
StatefulSet or pods fail to start with permission errors.

---

### 4.6: Leader/Follower Mode

**Leader** (pod that holds the Lease):

- Runs full pipeline: gRPC ingestor → Kafka producer → consumer → matching engine → fills fan-out → DB → Redis →
  WebSocket
- Writes snapshots to Postgres every 10,000 orders
- Patches own pod label to `role=leader` → Service routes traffic to it

**Follower** (pod that does not hold the Lease):

- Consumes same Kafka `orders-in` topic
- Replays orders into in-memory matching engine
- No DB writes, no Redis writes, no WS, no fills-out publish
- Exposes `/health`, `/healthz`, `/markets/:id/book`, `/markets/:id/trades` (read-only)
- Patches own pod label to `role=follower`
- On promotion: exits with code 0 → Kubernetes restarts pod → pod starts fresh as leader

---

### 4.7: Health Probes

`/healthz` returns 200 only when all four conditions are true:

| Check           | What it verifies                                 |
|-----------------|--------------------------------------------------|
| `db`            | Postgres `SELECT 1` succeeds                     |
| `engine_alive`  | Matching engine thread is running                |
| `lease_healthy` | Pod holds or has seen a valid lease              |
| `kafka_ready`   | At least one Kafka message successfully consumed |

Kubernetes readiness probe uses `/healthz`. Pod is removed from Service endpoints until all checks pass — prevents
traffic routing to a pod still loading its snapshot or replaying Kafka.

Liveness probe also uses `/healthz`. If the engine thread dies, pod is restarted.

---

### 4.8: Failover Flow

```
1. clob-0 (leader) deleted
2. Lease TTL expires (≤15s)
3. clob-1 acquires lease via PATCH coordination.k8s.io/v1/leases/clob-leader
4. clob-1 patches own pod label: role=follower → role=leader
5. Service selector role=leader now matches clob-1
6. clob-1 exits follower loop, process.exit(0)
7. Kubernetes restarts clob-1 as fresh leader
8. On restart: loads latest snapshot from Postgres, seeks Kafka to kafka_offset+1
9. Replays delta, engine catches up, /healthz returns 200
10. Traffic resumes
```

Recovery gap = lease TTL (≤15s) + Kafka replay time (bounded by snapshot interval).

---

### 4.9: Prerequisites

```bash
kubectl config use-context docker-desktop
```

- Infrastructure running via docker-compose (Postgres, Redis, Redpanda, Jaeger):

```bash
docker compose up -d
make topics
make migrate
```

---

### 4.10: Build and Deploy

**Build image** (must be done before apply — Docker Desktop shares the local Docker daemon):

```bash
docker build -f crates/clob-api/Dockerfile -t clob-api:latest .
```

**Apply manifests in order:**

```bash
kubectl apply -f k8s/
```

**Verify:**

```bash
kubectl get pods -l app=clob --show-labels
kubectl get lease clob-leader -n default
kubectl get endpoints clob
```

---

### 4.11: Manual Failover Test

```bash
# 1. Confirm current leader
kubectl get pods -l app=clob --show-labels

# 2. Delete leader pod
kubectl delete pod clob-0

# 3. Watch failover
kubectl get pods -l app=clob -w

# 4. Verify new leader label
kubectl get pods -l app=clob --show-labels

# 5. Verify service rerouted
kubectl get endpoints clob

# 6. Verify snapshot replay in logs
kubectl logs clob-1 | grep -E "snapshot|replaying|leader"

# 7. Verify no fill gaps
psql postgres://clob:clob@localhost:5432/clob \
  -c "SELECT sequence FROM fills ORDER BY sequence" | awk 'NR>2{if($1!=prev+1) print "GAP at "prev" → "$1; prev=$1}'
```

---

### 4.12: Teardown

```bash
kubectl delete -f k8s/
```

---

## 5: Observability

| Tool             | URL                    | What to look for                                                                       |
|------------------|------------------------|----------------------------------------------------------------------------------------|
| Prometheus       | http://localhost:9091  | Raw metrics, PromQL queries                                                            |
| Grafana          | http://localhost:3000  | CLOB folder → 4 dashboards                                                             |
| Jaeger           | http://localhost:16686 | Service: `clob-api`, spans: `place_order`, `send_order`, `match_order`, `insert_batch` |
| Redpanda Console | http://localhost:8080  | Topic lag, message browser                                                             |
| pgAdmin          | http://localhost:5050  | Query `fills`, `orders`, `snapshots` tables                                            |

### 5.1: Key metrics

| Metric                     | Type    | Description                 |
|----------------------------|---------|-----------------------------|
| `fills_total`              | counter | Total fills produced        |
| `matching_latency_ns`      | summary | Per-order matching time     |
| `kafka_consumer_lag`       | gauge   | Per-partition consumer lag  |
| `snapshot_size_bytes`      | summary | Serialized book size        |
| `db_write_batch_size`      | summary | Fills per Postgres batch    |
| `ws_client_count`          | gauge   | Connected WebSocket clients |
| `http_request_duration_ns` | summary | Per-endpoint HTTP latency   |

### 5.2: Useful PromQL

```
rate(fills_total[1m])
matching_latency_ns{quantile="0.99"}
sum(kafka_consumer_lag)
rate(db_write_batch_size_count[1m])
```

---

## 6: Performance Benchmarks

```bash
cargo bench -p clob-engine
```

Three benchmark suites:

- `order_matching` — end-to-end match cycles at varying book depths
- `orderbook_ops` — insert/cancel/best_bid operations
- `serialization` — postcard encode/decode throughput

Results are written to `target/criterion/`. Compare baselines with:

```bash
cargo bench -p clob-engine -- --save-baseline before
# make changes
cargo bench -p clob-engine -- --baseline before
```

### 6.1: Allocator Baseline

**Platform:** aarch64-apple-darwin (Apple Silicon)

#### Heap allocations per benchmark (dhat, system allocator)

| Benchmark               | Total blocks | Total bytes | Live at peak             |
|-------------------------|--------------|-------------|--------------------------|
| `alloc_single_match`    | 563,682      | 99,488,172  | 20 blocks / 2,023 bytes  |
| `alloc_market_sweep_50` | 563,140      | 141,584,357 | 54 blocks / 15,752 bytes |

The block counts are per profiling run (~5s), not per iteration. Live-at-peak is the relevant number for hot-path
pressure: **20 blocks for a single match, 54 for a 50-level sweep**. `slab` is working — no per-match order allocation
on the hot path; allocations are dominated by `Uuid::new_v4()`, `String` market_id, and `Vec<Fill>` construction in the
benchmark harness itself.

#### Allocator comparison — `single_limit_match` (p50)

| Allocator | time     |
|-----------|----------|
| system    | ~3.37 µs |
| mimalloc  | ~3.21 µs |
| jemalloc  | ~3.21 µs |

#### Allocator comparison — `market_sweep/100` (p50)

| Allocator | time        |
|-----------|-------------|
| system    | ~177–178 µs |
| mimalloc  | ~175 µs     |
| jemalloc  | ~175 µs     |

**Decision: keep system allocator.**

Mimalloc and jemalloc each save ~150–160 ns on `single_limit_match` (~4–5%). At sweep depth 100 the gain is ~2 µs (~1%).
The improvement is real but marginal — the benchmark harness allocates heavily per iteration (fresh `MatchingEngine`,
`String`, `Uuid`) which inflates the allocator signal. The production matching thread reuses the engine across calls;
allocator pressure is lower in practice. Switching allocators at this stage is premature — the bottleneck is not the
allocator.

**Revisit if:** matching latency target is sub-microsecond or allocation profiling shows growth in production heap
pressure.

### 6.2: BTreeMap vs Vec+binary search

**Platform:** aarch64-apple-darwin (Apple Silicon)

#### Insert (N orders at distinct price levels)

| Depth | BTreeMap    | Vec+bisect    |
|-------|-------------|---------------|
| 50    | ~82–86 µs   | ~80–81 µs     |
| 200   | ~330–332 µs | ~325–331 µs   |
| 1000  | ~1.71 ms    | ~1.68–1.73 ms |

**Verdict: no meaningful difference.** Insert is dominated by `Uuid::new_v4()` and `String` allocation in the benchmark
harness, not the data structure traversal.

#### Cancel (N orders, one per distinct price level)

| Depth | BTreeMap    | Vec+bisect  |
|-------|-------------|-------------|
| 50    | ~6–7 µs     | ~6.6–7 µs   |
| 200   | ~28–35 µs   | ~39–57 µs   |
| 1000  | ~150–157 µs | ~492–860 µs |

**Verdict: BTreeMap wins decisively on cancel at depth.** Vec cancel is O(n) due to `retain` shifting elements after
removal. At depth 1000, Vec is 3–5× slower with high variance. BTreeMap `remove` is O(log n).

#### best_bid / best_ask

| Depth | BTreeMap | Vec+bisect |
|-------|----------|------------|
| 50    | ~2.7 ns  | ~1.49 ns   |
| 200   | ~3.5 ns  | ~1.49 ns   |
| 1000  | ~4.5 ns  | ~1.48 ns   |

**Verdict: Vec wins on reads — O(1) last/first vs O(log n) BTreeMap key traversal.** Gap grows with depth. However, the
absolute difference is ~3 ns at depth 1000 — negligible against the µs-range match cycle cost.

#### Match cycle (single fill against N resting orders)

| Depth | BTreeMap    |
|-------|-------------|
| 50    | ~3.5–7.9 µs |
| 200   | ~8.8–12 µs  |
| 1000  | ~60–80 µs   |

Match cycle only measured for BTreeMap (production engine). Cost at depth 1000 is dominated by `iter_batched` setup (
re-inserting 1000 resting orders), not the single match itself.

**Decision: keep BTreeMap.**

The cancel advantage at depth (3–5× at 1000 levels) is the deciding factor. Cancel is a first-class operation in a live
book. The Vec `best_bid/ask` read advantage (~3 ns) does not justify the cancel regression. BTreeMap provides balanced
O(log n) across all operations with low variance.

**Revisit if:** profiling shows `best_bid/ask` in the hot path at >10M ops/sec, at which point a hybrid (BTreeMap for
structure, cached best price as a scalar) is the right approach — not replacing BTreeMap with Vec.

### 6.3: Channel Depth Tuning

**Tested capacities:** 256, 1024, 8192, 32768  
**Mock rate:** 1000 msg/s

| Capacity | fills/sec steady | p50 (ns) steady | p99 (ns) steady  |
|----------|------------------|-----------------|------------------|
| 256      | ~9.44            | ~39,000–41,000  | ~230,000–320,000 |
| 1024     | ~9.48            | ~40,000–42,000  | ~141,000–185,000 |
| 8192     | ~9.48            | ~38,000–39,000  | ~134,000–205,000 |
| 32768    | ~9.48            | ~39,000–42,000  | ~150,000–226,000 |

**Revised finding:**

Fill rate is identical across all capacities (~9.48/s) — throughput is not affected by channel depth.

p50 is also nearly identical across all capacities (~38–42µs). The previous conclusion that p50 was ~32µs was from a
different run context (Prometheus scrape timing, different Kafka batch config).

**p99 does show a real signal:**

- depth=256: p99 ~230–320µs — highest and most variable. Back-pressure from the small buffer introduces scheduling
  jitter.
- depth=1024: p99 ~141–185µs — meaningful improvement over 256.
- depth=8192: p99 ~134–205µs — similar to 1024, slightly lower floor.
- depth=32768: p99 ~150–226µs — no further improvement; slightly worse variance than 8192.

**Revised decision: keep default of 8192.**

It provides the best p99 floor with acceptable variance. 1024 is viable. 256 is too small — p99 spikes confirm
back-pressure. 32768 adds memory overhead without p99 benefit.

**The previous claim that "capacity is not the bottleneck" holds for throughput and p50. It does not hold for p99 —
depth=256 measurably degrades tail latency.**

### 6.4: Kafka Batch Size Tuning

**Tested:** `linger_ms` ∈ {0, 5, 50, 100}, `batch.num.messages` ∈ {1, 1000, 5000, 10000}  
**Mock rate:** 1000 msg/s, 4 partitions

#### Consumer lag behaviour

| linger_ms | batch.num.messages | Total lag trend                 | Drains to zero?               |
|-----------|--------------------|---------------------------------|-------------------------------|
| 0         | 1                  | Grows continuously: 2.5k → 37k+ | No                            |
| 5         | 1000               | Stable ~14k–20k, slow drain     | Partial                       |
| 50        | 5000               | Starts ~30k, drains to ~1.7k    | Yes (~4 min)                  |
| 100       | 10000              | Starts ~2.7k, drains to ~1.8k   | Nearly (2 partitions reach 0) |

**Finding: linger_ms=0 (no batching) causes unbounded lag growth.** At 1000 msg/s with per-message sends, the producer
overhead saturates the pipeline — the consumer cannot keep up. Lag grows at ~600 msg/15s.

**linger_ms=5 stabilises lag** but never fully drains — consumer matches produce rate but cannot recover the initial
backlog within the observation window.

**linger_ms=50 is the first configuration that fully drains lag** — batching amortises producer overhead enough that the
consumer recovers. Drain rate ~1.5k msgs/15s.

**linger_ms=100 starts with the lowest observed lag (~2.7k)** and two partitions drain to zero within the window. Higher
linger allows larger batches, reducing per-message overhead further.

**Decision: set default to `linger_ms=50`, `batch.num.messages=5000`.**

This is the knee: lag drains, consumer keeps up, and per-message latency remains acceptable for a 1000 msg/s workload.
`linger_ms=100` is viable at higher throughput where batch fill rate justifies the added latency.

**Revisit if:** produce rate exceeds 5k msg/s — at that rate `linger_ms=5` batches will fill naturally and higher linger
adds latency without throughput benefit.

**Update `KAFKA_LINGER_MS` default in `producer.rs` from `5` to `50` and `batch.num.messages` from `1000` to `5000`.**

### 6.5: Postgres Batch Write Tuning

**Tested:** `batch_size` ∈ {10, 100, 500, 1000}, corresponding `flush_ms` ∈ {10, 25, 50, 100}  
**Fill rate:** ~9.5 fills/sec (from mock at 1000 msg/s)

| batch_size | flush_ms | actual p50 batch | actual p99 batch | batches/sec steady | rows/sec |
|------------|----------|------------------|------------------|--------------------|----------|
| 10         | 10       | 1                | 1                | ~87                | ~1,280   |
| 100        | 25       | 2                | 3                | ~40                | ~1,280   |
| 500        | 50       | 4                | 5–6              | ~20                | ~1,230   |
| 1000       | 100      | 8                | 9                | ~10                | ~1,230   |

**Key finding: batch size threshold is never reached.** At ~9.5 fills/sec, no configuration triggers a size-based
flush — all flushes are timer-driven. The configured `batch_size` is irrelevant at this fill rate; only `flush_ms`
matters.

**`flush_ms` directly controls batches/sec:** 10ms→87/s, 25ms→40/s, 50ms→20/s, 100ms→10/s. Rows/sec is identical across
all configs (~1,230–1,280) — throughput is fill-rate-bound, not DB-bound.

**Decision: keep defaults of `batch_size=500`, `flush_ms=50`.**

At current fill rates, `flush_ms=50` gives 20 DB round-trips/sec with batches of 4–6 rows. Lower `flush_ms` increases DB
round-trips with no throughput benefit. Higher `flush_ms` increases fill persistence latency unnecessarily.

**Batch size tuning only becomes relevant when fill rate exceeds `batch_size / flush_ms_in_seconds`.** At 500 rows /
0.05s = 10,000 fills/sec would be needed to trigger size-based flushes with current defaults. At that rate, revisit both
parameters together.

### 6.6: Snapshot Interval Tuning

**Tested:** `SNAPSHOT_EVERY` ∈ {100, 1000, 10000}  
**Mock rate:** ~9.5–80 fills/sec depending on run context

| SNAPSHOT_EVERY | snapshot_size p99 (bytes) | matching p50 steady (ns) | matching p99 steady (ns) | recovery gap observed          |
|----------------|---------------------------|--------------------------|--------------------------|--------------------------------|
| 100            | 1,692–7,934               | 120k–11M (noisy)         | 11M–592M (spiky)         | ~80s fill gap after restart    |
| 1000           | 733–5,052                 | 60k–11M (noisy)          | 13M–47M (spiky)          | ~30–45s fill gap after restart |
| 10000          | 0–1,692                   | 20k–34k (stable)         | 143k–232k (stable)       | ~30s fill gap after restart    |

**Key findings:**

**SNAPSHOT_EVERY=100 and SNAPSHOT_EVERY=1000 both cause severe matching latency spikes.** p50 oscillates between ~60µs
and ~11ms — snapshot serialization + Postgres write is blocking the engine thread on every 100th or 1000th message. p99
reaches 592ms at SNAPSHOT_EVERY=100. This is a direct result of `rt.block_on()` inside the engine thread.

**SNAPSHOT_EVERY=10000 is the only configuration with stable latency.** p50 steady ~33µs, p99 ~150–230µs — consistent
with the no-snapshot baseline from 7.3. Snapshot writes are infrequent enough that they do not visibly perturb matching
latency at ~9.5 fills/sec.

**Snapshot size is small in all cases** (under 8KB). Size is not a concern at current book depth.

**Recovery gap:** All configurations showed a ~30–80s window of reduced/zero fills after restart before returning to
steady-state. The gap is dominated by Kafka offset replay, not snapshot load time. The snapshot itself loads in <1s in
all cases.

**Decision: keep default `SNAPSHOT_EVERY=10_000`.**

Lower values add unacceptable latency spikes without meaningfully reducing recovery time — recovery is replay-bound, not
snapshot-load-bound. Revisit only if the engine thread is refactored to perform snapshot saves off-thread (async
hand-off), at which point lower intervals become viable.

**Root cause of latency spikes at low intervals:** `rt.block_on()` in the engine thread is synchronous. Every snapshot
save blocks matching for the duration of the Postgres round-trip (~10–50ms). This is architectural — the fix is to send
snapshot bytes to an async task via channel, not to tune the interval.

---

## 7: Project Structure

```
.
├── Cargo.toml                          # Workspace root
├── Makefile
├── rust-toolchain.toml
├── crates/
│   ├── clob-engine/                    # Domain library
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── order.rs                # Order, Side, OrderStatus
│   │   │   ├── orderbook.rs            # BTreeMap-based price level book
│   │   │   ├── matching.rs             # MatchingEngine
│   │   │   ├── events.rs               # Fill, OrderEvent
│   │   │   ├── snapshot.rs             # postcard serialize/deserialize
│   │   │   ├── types.rs                # Price, Quantity aliases
│   │   │   └── error.rs
│   │   └── benches/
│   │       ├── order_matching.rs
│   │       ├── orderbook_ops.rs
│   │       └── serialization.rs
│   ├── clob-api/                       # Main service
│   │   └── src/
│   │       ├── main.rs                 # Startup, wiring
│   │       ├── state.rs                # AppState (pool, producer, caches)
│   │       ├── metrics.rs              # Metric names + describe_*
│   │       ├── tracing_setup.rs        # OTLP pipeline init
│   │       ├── middleware/
│   │       │   └── metrics.rs          # Per-endpoint HTTP latency
│   │       ├── handlers/
│   │       │   ├── health.rs           # GET /health
│   │       │   ├── orders.rs           # POST /orders, GET /orders/:id
│   │       │   ├── markets.rs          # GET /markets/:id/book, /trades
│   │       │   └── ws.rs               # GET /ws/markets/:id
│   │       ├── ingest/
│   │       │   ├── yellowstone.rs      # gRPC stream consumer
│   │       │   └── processor.rs        # Order mapping + engine thread
│   │       ├── kafka/
│   │       │   ├── producer.rs         # KafkaProducer (orders-in)
│   │       │   ├── consumer.rs         # KafkaConsumer (orders-in)
│   │       │   └── fill_publisher.rs   # FillPublisher (fills-out)
│   │       ├── persistence/
│   │       │   ├── fills.rs            # FillWriter (batch insert)
│   │       │   ├── orders.rs           # Order queries
│   │       │   └── snapshot.rs         # SnapshotStore (upsert + load)
│   │       ├── cache/
│   │       │   ├── orderbook.rs        # Redis best bid/ask
│   │       │   └── fills.rs            # Redis recent fills ring
│   │       └── ws/
│   │           └── broadcast.rs        # tokio::broadcast fill fan-out
│   └── yellowstone-mock/               # Simulated gRPC feed
│       └── src/
│           ├── main.rs
│           ├── server.rs               # tonic gRPC server
│           ├── generator.rs            # Random SubscribeUpdate emission
│           └── config.rs
├── infra/
│   ├── postgres/init.sql
│   ├── prometheus/prometheus.yml
│   ├── grafana/
│   │   ├── provisioning/
│   │   │   ├── dashboards/dashboards.yml
│   │   │   └── datasources/datasources.yml
│   │   └── dashboards/
│   │       ├── clob-overview.json
│   │       ├── clob-matching-engine.json
│   │       ├── clob-kafka.json
│   │       └── clob-postgres.json
│   └── redpanda/console-config.yaml
├── migrations/
│   ├── 20260308000000_initial.sql
│   └── 20260309000001_snapshots_kafka_offset.sql
└── docker-compose.yml
```

---

## 8: Prerequisites

- Rust (see `rust-toolchain.toml`)
- Docker + Docker Compose
- `sqlx-cli`: `cargo install sqlx-cli --no-default-features --features postgres`
- `websocat` (optional, for WebSocket testing): `brew install websocat`

---

## 9: Running

### 9.1: Start infrastructure

```bash
docker compose up -d
```

### 9.2: Start yellowstone-mock

```bash
docker compose up -d yellowstone-mock
```

### 9.3: Create Kafka topics

```bash
make topics
```

### 9.4: Run migrations

```bash
make migrate
```

### 9.5: Run clob-api

```bash
RUST_LOG=info \
YELLOWSTONE_ENDPOINT=http://localhost:10000 \
KAFKA_BROKERS=localhost:19092 \
DATABASE_URL=postgres://clob:clob@localhost:5432/clob \
REDIS_URL=redis://localhost:6379 \
HTTP_BIND=0.0.0.0:8000 \
OTLP_ENDPOINT=http://localhost:4317 \
cargo run -p clob-api
```

---

## 10: Environment Variables

| Variable               | Default                                    | Description                                    |
|------------------------|--------------------------------------------|------------------------------------------------|
| `YELLOWSTONE_ENDPOINT` | `http://localhost:10000`                   | gRPC feed address                              |
| `KAFKA_BROKERS`        | `localhost:19092`                          | Redpanda broker                                |
| `DATABASE_URL`         | `postgres://clob:clob@localhost:5432/clob` | Postgres DSN                                   |
| `REDIS_URL`            | `redis://localhost:6379`                   | Redis URL                                      |
| `HTTP_BIND`            | `0.0.0.0:8000`                             | HTTP listen address                            |
| `OTLP_ENDPOINT`        | `http://localhost:4317`                    | Jaeger OTLP gRPC endpoint                      |
| `RUST_LOG`             | —                                          | Tracing filter (e.g. `info`, `clob_api=debug`) |

---

## 11: API

### 11.1: Health

```
GET /health
→ {"status":"ok","db":true}
```

### 11.2:Place order

```
POST /orders
Content-Type: application/json

{"market_id":"SOL/USDC","side":"bid","order_type":"limit","price":101,"quantity":1}
→ {"id":"<uuid>"}
```

### 11.3: Get order

```
GET /orders/<uuid>
→ {"id":…,"market_id":…,"side":…,"price":…,"quantity":…,"status":…,"sequence":…}
```

### 11.4: Order book (best bid/ask from Redis)

```
GET /markets/SOL%2FUSDC/book
→ {"market_id":"SOL/USDC","best_bid":"101","best_ask":"100"}
```

### 11.5: Recent trades

```
GET /markets/SOL%2FUSDC/trades
→ [{"maker_order_id":…,"taker_order_id":…,"price":…,"quantity":…,"sequence":…}]
```

### 11.6: WebSocket fills stream

```
ws://localhost:8000/ws/markets/SOL%2FUSDC
← {"maker_order_id":…,"taker_order_id":…,"price":…,"quantity":…,"sequence":…}
```

---

## 12: Debugging

### 12.1: Check Kafka topic contents

```bash
docker exec clob-redpanda rpk topic consume orders-in --brokers localhost:9092 -n 5
```

### 12.2: Check consumer lag

```bash
docker exec clob-redpanda rpk group describe clob-engine --brokers localhost:9092
```

### 12.3: Check Postgres fills

```bash
psql postgres://clob:clob@localhost:5432/clob -c "SELECT count(*) FROM fills;"
```

### 12.4: Check Redis book state

```bash
redis-cli GET book:SOL/USDC:best_bid
redis-cli GET book:SOL/USDC:best_ask
```

### 12.5: Check recent fills in Redis

```bash
redis-cli LRANGE fills:recent 0 9
```

### 12.6: Reset everything

```bash
make reset-db
docker compose down -v
docker compose up -d
make topics
make migrate
```

### 12.7: Grafana not loading dashboards

```bash
docker exec clob-grafana ls /var/lib/grafana/dashboards
docker logs clob-grafana 2>&1 | grep -i "provision\|dashboard\|error"
```

### 12.8: Jaeger not receiving traces

Confirm `OTLP_ENDPOINT=http://localhost:4317` is set and Jaeger container is running:

```bash
docker logs clob-jaeger 2>&1 | tail -20
```

---

## 13: Linting and Formatting

```bash
cargo check
```

```bash
cargo fmt -- --check
```

```bash 
cargo clippy --all-targets --all-features -- -D warnings
```

Allows unused code

```bash
cargo clippy --all-targets --all-features -- -D warnings -A unused -A dead_code
```

---

## 14: Makefile Targets

| Target          | Description                                  |
|-----------------|----------------------------------------------|
| `make up`       | `docker compose up -d`                       |
| `make down`     | `docker compose down`                        |
| `make build`    | `cargo build --workspace`                    |
| `make bench`    | `cargo bench -p clob-engine`                 |
| `make migrate`  | Run sqlx migrations                          |
| `make reset-db` | Drop and recreate DB + re-migrate            |
| `make topics`   | Create `orders-in` + `fills-out` in Redpanda |
| `make mock`     | Start yellowstone-mock container             |
| `make logs`     | Follow all container logs                    |
| `make console`  | Open Redpanda console                        |
| `make db-stats` | Print fill/order/snapshot counts             |

---


