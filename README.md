# simpleload-balancer-rs

A small asynchronous HTTP reverse proxy and load balancer written in Rust with Tokio and Hyper.

The project was built as a hands-on backend/infrastructure exercise focused on routing, concurrency, failure handling, integration testing, and load testing.

## Features

- Round Robin backend selection
- Multiple backend support
- Asynchronous HTTP server built with Tokio and Hyper
- Reverse proxy request forwarding
- Concurrent request handling
- Shared load-balancer state with short critical sections
- Preserves HTTP method, path, query string, request body, upstream status, and response body
- `GET /health` endpoint that bypasses backend routing
- `503 Service Unavailable` when no backends are configured
- `502 Bad Gateway` for unreachable or invalid upstream backends
- `504 Gateway Timeout` for slow upstreams
- Integration tests for forwarding, failures, timeouts, and concurrency
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

The load balancer keeps a backend pool and a stateful Round Robin selector.

For each proxied request it:

1. Selects the next backend.
2. Releases the shared routing lock before performing network I/O.
3. Rebuilds the outbound request for the selected backend.
4. Sends the request with the Hyper HTTP client.
5. Returns the upstream response to the original client.

`/health` is handled directly by the proxy and does not consume a Round Robin selection.

## Error Handling

The proxy returns:

- `503 Service Unavailable` — backend pool is empty
- `502 Bad Gateway` — upstream connection/request fails or backend URI is invalid
- `504 Gateway Timeout` — upstream does not complete within the configured timeout

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
- 10 HTTP integration tests
- 29 tests total

The integration suite covers:

- health endpoint
- empty backend pool
- request forwarding
- Round Robin behavior through HTTP
- unreachable backend handling
- invalid backend URI handling
- path and query forwarding
- POST method and body forwarding
- upstream timeout handling
- concurrent requests where a fast request is not blocked by a slow request

## Running

For performance measurements, build and run the proxy in release mode:

```bash
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

Health endpoint:

```text
http://127.0.0.1:3000/health
```

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

## Current Limitations

This is an educational load balancer rather than a production-ready replacement for HAProxy, Envoy, or NGINX.

Current limitations include:

- no active backend health checks
- unreachable backends are not automatically removed from rotation
- no retry/failover policy
- no weighted routing
- no least-connections routing
- no TLS termination
- request/response bodies are buffered rather than fully streamed
- no production observability or metrics endpoint
- no graceful backend draining

For example, if one backend becomes unreachable, the proxy currently returns `502 Bad Gateway` for requests routed to that backend. It does not yet mark that backend unhealthy and exclude it from future Round Robin selections.

## What This Project Demonstrates

The project exercises several backend/core-infrastructure concepts:

- ownership and borrowing across application layers
- stateful routing algorithms
- asynchronous networking
- concurrent request processing
- synchronization with minimal lock scope
- reverse-proxy behavior
- HTTP error semantics
- deterministic integration testing with ephemeral ports
- timeout handling
- performance testing
- saturation and tail-latency analysis
- horizontal scaling with multiple backends
