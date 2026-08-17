# Load Balancer Benchmarks

## Goal

This benchmark evaluates the throughput and latency of the Rust/Tokio/Hyper reverse proxy and measures the effect of distributing traffic across multiple backends with Round Robin routing.

The main questions were:

- How much load can the proxy sustain in a local benchmark?
- Where does tail latency begin to degrade?
- Is the backend itself the bottleneck?
- Does adding a second backend improve behavior under the same offered load?

These results are intended as development and learning benchmarks, not as production capacity claims.

## Environment and Methodology

All endpoints were bound to `127.0.0.1`, so these are local single-host measurements.

- Load generator: k6
- Proxy: Rust + Tokio + Hyper
- Backend: minimal Rust + Tokio + Hyper HTTP server
- Proxy build: `--release`
- Backend build: `--release`
- k6 executor: `constant-arrival-rate`
- Test duration: 30 seconds
- Requests: `GET /`
- Metrics tracked:
  - target RPS
  - actual RPS
  - HTTP failure rate
  - dropped iterations
  - p50 latency
  - p95 latency
  - p99 latency

`preAllocatedVUs` was increased during testing when needed so that insufficient k6 VUs would not be confused with application saturation.

### Build mode sanity check

An early benchmark showed that Rust build mode materially changes the result.

| Proxy build | Target RPS | Actual RPS | Dropped | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|---:|
| Debug | 1800 | 1641.18 | 4697 | 19.61 ms | 397.35 ms | 740.01 ms |
| Release | 1800 | 1799.84 | 0 | 0.289 ms | 15.50 ms | 53.02 ms |

Because of this difference, debug-build measurements were excluded from the final capacity comparison below.

## Single Backend Through Proxy

The proxy routed all requests to one Rust backend.

| Target RPS | Actual RPS | Failed | Dropped | Dropped % | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|---:|---:|
| 3000 | 2997.52 | 0 | 70 | 0.08% | 0.246 ms | 1.05 ms | 3.84 ms |
| 3500 | 3499.66 | 0 | 0 | 0.00% | 0.327 ms | 22.94 ms | 87.44 ms |
| 3750 | 3687.40 | 0 | 1768 | 1.57% | 0.513 ms | 67.16 ms | 163.58 ms |
| 4000 | 3871.63 | 0 | 3838 | 3.20% | 0.964 ms | 107.93 ms | 266.34 ms |

At 3000 RPS the proxy handled essentially the full offered load with very low latency.

At 3500 RPS it still completed the full target with no dropped iterations, although tail latency had already increased.

Between 3500 and 3750 RPS, the local single-backend path entered a clear degradation region: dropped iterations appeared and p95/p99 latency increased sharply.

The exact saturation point should not be treated as a universal proxy limit because all components were running locally on one host.

## Direct Backend Comparison

To separate backend capacity from proxy-path overhead, the Rust backend was tested directly at 4000 RPS with the same 30-second constant-arrival-rate workload.

| Path | Target RPS | Actual RPS | Failed | Dropped | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Direct backend | 4000 | 3999.20 | 0 | 0 | 0.147 ms | 5.89 ms | 33.39 ms |
| Proxy → 1 backend | 4000 | 3871.63 | 0 | 3838 | 0.964 ms | 107.93 ms | 266.34 ms |

The backend itself sustained approximately 4000 RPS without dropped iterations. Under the same offered load, the one-backend proxy path showed substantially higher tail latency and dropped iterations.

This indicates that the observed degradation at this level was associated with the full proxy/forwarding path rather than the backend alone.

## Two-Backend Round Robin

The proxy was then configured with two identical Rust backends and distributed requests between them using Round Robin routing.

| Backends | Target RPS | Actual RPS | Failed | Dropped | Dropped % | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| 2 | 4000 | 3999.47 | 0 | 0 | 0.00% | 0.305 ms | 8.16 ms | 57.42 ms |
| 2 | 5000 | 4945.80 | 0 | 1612 | 1.07% | 0.328 ms | 34.37 ms | 122.09 ms |

The 4000 RPS comparison is the clearest demonstration of horizontal scaling in this benchmark:

| Configuration | Actual RPS | Dropped | p95 | p99 |
|---|---:|---:|---:|---:|
| Proxy → 1 backend | 3871.63 | 3838 | 107.93 ms | 266.34 ms |
| Proxy → 2 backends | 3999.47 | 0 | 8.16 ms | 57.42 ms |

With two backends, the proxy sustained the full 4000 RPS target with zero HTTP failures and zero dropped iterations, while tail latency was substantially lower than in the one-backend run.

At 5000 RPS, the two-backend configuration still completed approximately 4946 RPS with zero HTTP failures, but around 1.07% of planned iterations were dropped and tail latency increased. This indicates that the local test setup was beginning to experience pressure at that offered load.

## Conclusions

1. **Release builds are mandatory for meaningful Rust performance measurements.**  
   The debug proxy produced dramatically worse throughput and tail latency and created a false impression that saturation occurred near 1800 RPS.

2. **The local one-backend proxy path was stable around 3000–3500 RPS.**  
   At 3500 RPS it completed the full offered load with no failures or dropped iterations. Between 3500 and 3750 RPS, dropped iterations and tail latency began to rise sharply.

3. **The backend alone was not the limiting component at 4000 RPS.**  
   Direct testing sustained approximately 3999 RPS with no dropped iterations, while the proxy-to-one-backend path degraded at the same target.

4. **Adding a second backend materially improved the 4000 RPS result.**  
   Round Robin distribution across two backends sustained essentially the full target, eliminated dropped iterations in that run, and significantly reduced p95/p99 latency compared with the one-backend configuration.

5. **The two-backend setup approached 5000 RPS but was beginning to show pressure.**  
   It delivered approximately 4946 RPS with zero HTTP failures and about 1.07% dropped iterations.

6. **These are localhost development benchmarks, not production capacity guarantees.**  
   k6, the proxy, the operating system networking stack, and the backend all share the resources of the local machine. Production results will depend on hardware, network topology, workload shape, response sizes, connection behavior, TLS, observability overhead, and backend application work.

The benchmark therefore demonstrates the behavior and horizontal-scaling value of the Round Robin proxy under a controlled local workload, rather than claiming a fixed production RPS limit.
