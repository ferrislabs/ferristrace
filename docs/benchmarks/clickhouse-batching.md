# Small versus large batches into ClickHouse

Produced by `cargo run -p ferristrace-clickhouse --example batching_benchmark --release`
in the `clickhouse` CI job, against `clickhouse/clickhouse-server:24.8` running as a
service container on the same runner.

60 000 spans, offered 100 at a time through `SpanSink::accept`, spread over 20 tenants.
Each row carries two attributes. Only `max_rows` changes between runs; the period is an
hour, so the row count is the only trigger.

| max_rows | spans | inserts | wall time | spans/s |
|---------:|------:|--------:|----------:|--------:|
| 100 | 60000 | 600 | 26.21s | 2289 |
| 1000 | 60000 | 60 | 0.28s | 211324 |
| 10000 | 60000 | 6 | 0.12s | 498864 |
| 60000 | 60000 | 1 | 0.11s | 533085 |

## Reading

One insert per 100 rows costs 233 times the wall time of a single insert for the same
60 000 spans. The cost is per statement, not per row: 600 inserts take 26 seconds while
6 inserts take 0.12. This is the reason the sink keeps one buffer for every tenant
instead of one per tenant batch.

The knee sits between 100 and 1000. Past 10 000 the curve is flat: 10 000 already reaches
94 percent of the throughput that 60 000 reaches, so raising the threshold further buys
little and costs memory and latency.

The default of 100 000 rows with a five second period therefore sits well past the knee.
The binding constraint on that default is not throughput but how many rows may sit in
memory, and how long a span may wait before it becomes queryable.

## What these numbers are not

A capacity figure. ClickHouse, the benchmark and the runner share one machine, with no
network between them and no other load. Treat the ratios as the finding and the absolute
rates as an upper bound.
