CREATE TABLE model_prices (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    provider text NOT NULL,
    model text NOT NULL,
    input_micros bigint NOT NULL CHECK (input_micros >= 0),
    output_micros bigint NOT NULL CHECK (output_micros >= 0),
    cached_input_micros bigint CHECK (cached_input_micros IS NULL OR cached_input_micros >= 0),
    effective_from_ms bigint NOT NULL CHECK (effective_from_ms >= 0),
    created_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    UNIQUE (org_id, provider, model, effective_from_ms)
);
