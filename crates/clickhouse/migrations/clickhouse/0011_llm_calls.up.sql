CREATE TABLE IF NOT EXISTS llm_calls
(
    tenant_id           UUID,
    project_id          UUID,
    trace_id            FixedString(16),
    span_id             FixedString(8),
    service_name        LowCardinality(String),
    time                DateTime64(9, 'UTC'),
    duration_ns         UInt64,
    status_error        UInt8,
    provider            LowCardinality(String),
    operation           LowCardinality(String),
    request_model       String,
    response_model      String,
    input_tokens        UInt64,
    output_tokens       UInt64,
    cached_input_tokens UInt64,
    feature             String,
    customer            String,
    user                String,
    prompt_version      String,
    environment         LowCardinality(String),
    expire_at           DateTime('UTC'),
    INDEX llm_calls_by_trace trace_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX llm_calls_by_project project_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree
PARTITION BY toDate(time)
ORDER BY (tenant_id, time, trace_id, span_id)
TTL expire_at DELETE;

INSERT INTO llm_calls
SELECT
    tenant_id,
    project_id,
    trace_id,
    span_id,
    service_name,
    start_time AS time,
    toUInt64(greatest(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time), 0)) AS duration_ns,
    toUInt8(status = 'error') AS status_error,
    coalesce(nullIf(attributes['gen_ai.provider.name'], ''), nullIf(attributes['gen_ai.system'], ''), '') AS provider,
    attributes['gen_ai.operation.name'] AS operation,
    attributes['gen_ai.request.model'] AS request_model,
    attributes['gen_ai.response.model'] AS response_model,
    toUInt64OrZero(attributes['gen_ai.usage.input_tokens']) AS input_tokens,
    toUInt64OrZero(attributes['gen_ai.usage.output_tokens']) AS output_tokens,
    toUInt64OrZero(attributes['gen_ai.usage.cache_read.input_tokens']) AS cached_input_tokens,
    coalesce(nullIf(attributes['ferristrace.feature'], ''), nullIf(attributes['gen_ai.agent.name'], ''), service_name) AS feature,
    coalesce(nullIf(attributes['ferristrace.customer'], ''), nullIf(attributes['enduser.id'], ''), '') AS customer,
    attributes['enduser.id'] AS user,
    coalesce(nullIf(attributes['ferristrace.prompt.version'], ''), nullIf(attributes['gen_ai.prompt.version'], ''), '') AS prompt_version,
    coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '') AS environment,
    expire_at
FROM spans FINAL
WHERE attributes['gen_ai.provider.name'] != ''
   OR attributes['gen_ai.system'] != ''
   OR attributes['gen_ai.request.model'] != '';

CREATE MATERIALIZED VIEW IF NOT EXISTS llm_calls_from_spans TO llm_calls AS
SELECT
    tenant_id,
    project_id,
    trace_id,
    span_id,
    service_name,
    start_time AS time,
    toUInt64(greatest(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time), 0)) AS duration_ns,
    toUInt8(status = 'error') AS status_error,
    coalesce(nullIf(attributes['gen_ai.provider.name'], ''), nullIf(attributes['gen_ai.system'], ''), '') AS provider,
    attributes['gen_ai.operation.name'] AS operation,
    attributes['gen_ai.request.model'] AS request_model,
    attributes['gen_ai.response.model'] AS response_model,
    toUInt64OrZero(attributes['gen_ai.usage.input_tokens']) AS input_tokens,
    toUInt64OrZero(attributes['gen_ai.usage.output_tokens']) AS output_tokens,
    toUInt64OrZero(attributes['gen_ai.usage.cache_read.input_tokens']) AS cached_input_tokens,
    coalesce(nullIf(attributes['ferristrace.feature'], ''), nullIf(attributes['gen_ai.agent.name'], ''), service_name) AS feature,
    coalesce(nullIf(attributes['ferristrace.customer'], ''), nullIf(attributes['enduser.id'], ''), '') AS customer,
    attributes['enduser.id'] AS user,
    coalesce(nullIf(attributes['ferristrace.prompt.version'], ''), nullIf(attributes['gen_ai.prompt.version'], ''), '') AS prompt_version,
    coalesce(nullIf(attributes['deployment.environment.name'], ''), nullIf(attributes['deployment.environment'], ''), '') AS environment,
    expire_at
FROM spans
WHERE attributes['gen_ai.provider.name'] != ''
   OR attributes['gen_ai.system'] != ''
   OR attributes['gen_ai.request.model'] != ''
