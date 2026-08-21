# simpleload-balancer-rs

A small asynchronous HTTP reverse proxy and load balancer written in Rust with Tokio and Hyper.

The project was built as a hands-on backend/infrastructure exercise focused on routing, concurrency, failure handling, backend recovery, integration testing, and load testing.

## Features

- Round Robin backend selection
- Multiple backend support
- Per-backend `healthy / unhealthy` state
- Automatic exclusion of unhealthy backends from routing
- Upstream failures mark the selected backend unhealthy
- Background health checker for unhealthy backends
- Automatic recovery: a backend returns to rotation after becoming reachable again
- Asynchronous HTTP server built with Tokio and Hyper
- Reverse proxy request forwarding
- Concurrent request handling
- Shared load-balancer state with short critical sections
- Preserves HTTP method, path, query string, request body, upstream status, and response body
- `GET /health` endpoint that bypasses backend routing
- `503 Service Unavailable` when no healthy backends are available
- `502 Bad Gateway` for unreachable or invalid upstream backends
- `504 Gateway Timeout` for slow upstreams
- Integration tests for forwarding, failures, timeouts, concurrency, health-state transitions, and recovery
- k6 load testing with direct, single-backend, and multi-backend comparisons

## Architecture

```text
                    +------------------+
                    |      Client      |
                    +--------+---------+
                             |
                             v
                    +------------------+
                    |   Load Balancer  |
                    |  Tokio + Hyper   |
                    +--------+---------+
                             |
                      Round Robin
                             |
              +--------------+--------------+
              |                             |
              v                             v
       +-------------+               +-------------+
       |  Backend A  |               |  Backend B  |
       +-------------+               +-------------+
```

The load balancer keeps a backend pool, per-backend health state, and a stateful Round Robin selector.

For each proxied request it:

1. Selects the next healthy backend.
2. Keeps the selected backend identity together with its address.
3. Releases the shared routing lock before performing network I/O.
4. Rebuilds the outbound request for the selected backend.
5. Sends the request with the Hyper HTTP client.
6. Returns the upstream response to the original client.
7. If the upstream interaction fails, marks that selected backend unhealthy.

Unhealthy backends remain in the backend pool but are skipped by normal routing.

A background health checker periodically takes a snapshot of unhealthy backends, probes them without holding the load-balancer mutex, and marks recovered backends healthy so that they return to Round Robin rotation.

`/health` is handled directly by the proxy and does not consume a Round Robin selection.

## Backend Health and Recovery

Backend health is part of the routing state.

```text
healthy backend
      |
      | upstream failure
      v
unhealthy
      |
      | excluded from normal routing
      v
background health probe
      |
      | successful recovery
      v
healthy again
      |
      v
returns to Round Robin
```

Current behavior:

- a newly configured backend starts as healthy;
- a real upstream interaction failure marks the selected backend unhealthy;
- unhealthy backends are skipped by client request routing;
- unhealthy backends are not deleted from the pool;
- the background health checker periodically probes unhealthy backends;
- a successful probe marks the backend healthy again;
- recovered backends automatically return to routing;
- multiple unhealthy backends can recover independently;
- a backend that remains unreachable stays excluded;
- an upstream timeout currently returns `504` but does not mark the backend unhealthy;
- the failed client request is not retried on another backend.

The health checker performs network probes outside the `Mutex` critical section. The lock is used only to take a snapshot of unhealthy backend metadata and later to apply health-state changes.

## Error Handling

The proxy returns:

- `503 Service Unavailable` — no healthy backend can be selected
- `502 Bad Gateway` — upstream connection/request handling fails or backend URI is invalid
- `504 Gateway Timeout` — upstream does not complete within the configured timeout
- `500 Internal Server Error` — shared load-balancer state cannot be locked

A real upstream interaction failure returns `502` for the current request and marks that selected backend unhealthy so that subsequent requests skip it.

An HTTP `502` response returned normally by a working upstream is still a valid HTTP response and is not, by itself, treated as a connection failure.

The proxy handles these cases without panicking.

## Tests

Run:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```

Current test suite:

- 19 unit tests
- 15 HTTP integration tests
- 34 tests total

The integration suite covers:

- health endpoint
- empty healthy backend set
- request forwarding
- Round Robin behavior through HTTP
- healthy/unhealthy backend state
- skipping unhealthy backends in routing
- all backends unhealthy
- unhealthy backend snapshot generation
- unreachable backend handling
- invalid backend URI handling
- path and query forwarding
- POST method and body forwarding
- upstream timeout handling
- concurrent requests where a fast request is not blocked by a slow request
- upstream failure marking a backend unhealthy
- automatic backend recovery
- an unrecovered backend remaining unhealthy
- independent recovery of multiple unhealthy backends

## Running

For performance measurements, build and run the proxy in release mode:

```bash
LB_LISTEN_ADDR=127.0.0.1:8080 LB_BACKENDS=127.0.0.1:8081,127.0.0.1:8082 LB_UPSTREAM_TIMEOUT_MS=1000 LB_HEALTH_CHECK_INTERVAL_MS=5000 cargo run     
cargo run --release
```

The load balancer is intended to proxy requests to configured backend addresses such as:

```text
http://127.0.0.1:4001
http://127.0.0.1:4002
```

Example proxy endpoint:

```text
http://127.0.0.1:3000/
```

Load-balancer health endpoint:

```text
http://127.0.0.1:3000/health
```

The current production health-check interval is still configured directly in code. External configuration is planned as part of the production-hardening stage.

## Performance Testing

Load testing was performed with k6 using the `constant-arrival-rate` executor.

The most useful local result was the comparison at a target load of 4000 RPS.

| Configuration | Actual RPS | Failed | Dropped | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|---:|
| Direct Rust backend | 3999.20 | 0 | 0 | 0.147 ms | 5.89 ms | 33.39 ms |
| Proxy -> 1 backend | 3871.63 | 0 | 3838 | 0.964 ms | 107.93 ms | 266.34 ms |
| Proxy -> 2 backends | 3999.47 | 0 | 0 | 0.305 ms | 8.16 ms | 57.42 ms |

With one backend, the full proxy path began to show saturation and tail-latency growth between approximately 3500 and 3750 RPS on the local test machine.

With two backends and Round Robin routing, the proxy sustained the full 4000 RPS target with zero HTTP failures and zero dropped iterations in that run.

At a target of 5000 RPS with two backends, the proxy delivered approximately 4946 RPS with zero HTTP failures and about 1.07% dropped iterations.

See [BENCHMARKS.md](BENCHMARKS.md) for the full benchmark methodology, results, and limitations.

## Important Benchmark Note

All benchmark components ran locally on the same machine.

These results demonstrate behavior under a controlled development workload and the effect of distributing traffic across multiple backends. They are **not production capacity guarantees**.

Results depend on hardware, network topology, response sizes, connection behavior, TLS, observability overhead, backend work, and workload shape.

Release mode is essential for meaningful Rust performance measurements. Debug builds produced dramatically worse results during testing.

The benchmark results were collected during the performance-testing stage of the project. A final control benchmark can be repeated after the remaining production-hardening work is complete.

## Current Limitations

This is an educational load balancer rather than a production-ready replacement for HAProxy, Envoy, or NGINX.

Current limitations include:

- health-check interval is still hard-coded rather than configured externally
- health probes use a simple direct HTTP request rather than a configurable readiness endpoint
- no failure threshold before marking an upstream unhealthy
- no success threshold before returning a backend to healthy
- no retry/failover policy for the current failed client request
- no exponential backoff or adaptive health-check policy
- backend identity currently relies on stable pool indexes
- no dynamic backend add/remove or configuration reload
- no weighted routing
- no least-connections routing
- no TLS termination
- request/response bodies are buffered rather than fully streamed
- no production observability or metrics endpoint
- no structured logging
- no graceful shutdown / coordinated background-task shutdown
- no graceful backend draining

## What This Project Demonstrates

The project exercises several backend/core-infrastructure concepts:

- ownership and borrowing across application layers
- stateful routing algorithms
- asynchronous networking
- concurrent request processing
- synchronization with minimal lock scope
- snapshotting shared state before asynchronous work
- reverse-proxy behavior
- backend health-state transitions
- automatic failure isolation and recovery
- background Tokio tasks
- HTTP error semantics
- deterministic integration testing with ephemeral ports
- timeout handling
- performance testing
- saturation and tail-latency analysis
- horizontal scaling with multiple backends
