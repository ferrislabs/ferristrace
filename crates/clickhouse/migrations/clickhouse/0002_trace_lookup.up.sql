ALTER TABLE spans ADD INDEX IF NOT EXISTS spans_by_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4
