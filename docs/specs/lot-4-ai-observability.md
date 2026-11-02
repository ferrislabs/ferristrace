# Lot 4: AI observability

Goal
  Show what every LLM call cost and how long it took, by feature, model,
  customer and environment, tied to the request that made it, and alert
  when a feature or a customer overspends.

Context
  Milestone "4 · AI observability": #184, #185, #186, #187. Spans already
  carry their attributes and resource merged in (lot 1), so spans written
  with the OpenTelemetry GenAI conventions (`gen_ai.provider.name` or the
  older `gen_ai.system`, `gen_ai.operation.name`, `gen_ai.request.model`,
  `gen_ai.response.model`, `gen_ai.usage.input_tokens`,
  `gen_ai.usage.output_tokens`) are stored, but nothing reads them. There
  is no AI page in the web app. Alert rules take trace, log and metric
  signals (lot 2).

  One pull request per part:
    4A  LLM calls and their cost                       #184 #185
    4B  Spend view                                     #186
    4C  Spend alerts                                   #187

Glossary
  LLM call       a span with a GenAI provider or request model attribute.
  Feature        what the product calls the code path that made the call:
                 `ferristrace.feature`, else `gen_ai.agent.name`, else the
                 span's service name.
  Customer       who the call was made for: `ferristrace.customer`, else
                 `enduser.id`, else empty.
  User           `enduser.id`.
  Prompt version `ferristrace.prompt.version`, else
                 `gen_ai.prompt.version`, else empty.
  Price          for one provider and model, the price per million input,
                 output and cached input tokens from an effective date.

Invariants
  - Every read filters on the organisation and narrows project-bound read
    keys, like lots 0 to 3.  [test]
  - A call's cost uses the price effective at the call's time; changing a
    price changes the cost of the calls after its effective date only.
    [test]
  - A call without a price is counted as unpriced, never as free.  [test]
  - Prompts and completions are not stored or shown beyond what the
    application already put in span attributes.  [review]
  - Every price write is audited (lot 0).  [test]

Out of scope
  - Shipping a built-in price list: prices change too often to be right in
    a release. The organisation enters the prices of the models it uses;
    the app lists the models seen without a price.
  - Capturing prompt and completion content, evaluations, LLM-as-judge.
  - Currencies other than US dollars.

Design
  4A Calls and cost
    ClickHouse: table `llm_calls` (tenant_id, project_id, trace_id,
    span_id, service_name, time, duration_ns, status_error, provider,
    operation, request_model, response_model, input_tokens,
    output_tokens, cached_input_tokens from
    `gen_ai.usage.cache_read.input_tokens`, feature, customer, user,
    prompt_version, environment, expire_at) MergeTree ORDER BY
    (tenant_id, time), fed by a materialized view over spans where the
    provider or request model is set, backfilled from existing spans, TTL
    like spans.
    Postgres: `model_prices` (org, provider, model, input, output,
    cached_input as micro-dollars per million tokens, effective_from)
    unique on (org, provider, model, effective_from); a model entry ending
    in `*` matches by prefix, the longest match wins, exact before prefix.
    Cost is computed when reading, in Rust, from the token counts and the
    price effective at the call time; the response model is priced before
    the request model.
    Reads: `LlmReader::calls` (window, filters on feature, model,
    customer, user, prompt version, trace id, provider, project; newest
    first, cursor) and `unpriced` (models seen in a window without a
    price, with call counts).
    HTTP: `GET /api/orgs/{org}/llm/calls?from&to&feature&model&customer&user&prompt&trace&project&limit&before`
    → `{calls:[{time_ms, trace_id, span_id, service, provider, operation,
    model, input_tokens, output_tokens, cached_input_tokens, cost|null,
    duration_ms, error, feature, customer, user, prompt_version,
    environment}], next}`; `GET/POST /api/orgs/{org}/llm/prices`,
    `DELETE /api/orgs/{org}/llm/prices/{id}` (admins write, audited);
    `GET /api/orgs/{org}/llm/unpriced?from&to`.
    Web: Settings > LLM prices (table, add, delete, the unpriced models
    with "add a price"); trace view marks LLM spans with model, tokens
    and cost, and the trace header shows model time next to the rest.
  4B Spend view
    `LlmReader::spend` (window, group by feature|model|customer|
    environment|provider, same filters): per group cost, unpriced calls,
    calls, input and output tokens, p50 and p95 duration, error count,
    and a series of cost per bucket (60 points); prompt version markers
    (first time a feature's prompt version is seen in the window).
    HTTP: `GET /api/orgs/{org}/llm/spend?from&to&group_by&...` →
    `{step_ms, groups:[{key, cost, unpriced, calls, input_tokens,
    output_tokens, p50_ms, p95_ms, errors, points:[[t_ms, cost]]}],
    markers:[{time_ms, feature, prompt_version}]}`.
    Web: an "AI" page: total spend, a stacked chart by the chosen group
    with prompt version markers, the group table; clicking a group opens
    the calls list filtered on it; a call opens its trace.
  4C Spend alerts
    A fourth alert signal `llm: {measure: cost|tokens|calls|p95,
    group_by: feature|customer|model|none, filter: {feature?, customer?,
    model?, project?}}`; group_by yields one state per group, like metric
    rules; evaluated through `LlmReader::spend` over the rule window,
    channels and incidents unchanged. Alert editor gains the signal.

Acceptance
  @spec-l4-1  A span with GenAI attributes is listed as an LLM call with
              provider, model, tokens, duration, feature, customer and
              prompt version, findable by user, trace id, model and
              prompt version.
  @spec-l4-2  With a price set, each call's cost is its tokens times the
              price effective at its time; a call of an unpriced model is
              reported as unpriced and its model listed.
  @spec-l4-3  Spend grouped by feature, model, customer or environment
              sums the costs of the calls in the window, and the trace
              view shows the model time inside its request.
  @spec-l4-4  A rule "cost of feature ticket-summary > $10 over 1 h" fires
              when the calls cross it and notifies its channels.

Simulation
  No: reads over stored data; the evaluator's state machine is already
  covered by lot 2.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck && pnpm build
Amended: 2026-11-02 4A as built: `llm_calls` is a ReplacingMergeTree keyed by tenant, time, trace and span, read with FINAL, so a replayed span counts once; prices are whole micro-dollars per million tokens on the wire and costs are dollars; a price without an effective date applies from the start; providers match case-insensitively; a model is listed as unpriced when no price is in force at its last call; model time in the trace header is the union of the calls' intervals.
Amended: 2026-11-02 4B as built: costs use the price in force at each bucket start (bucket = window / 60); the model group is the response model, else the request model; at most 100 groups by call count; prompt version markers ignore customer, user and model filters so a filter never fakes a first sighting; the calls list also filters on environment; a group with an empty key opens no calls list.
