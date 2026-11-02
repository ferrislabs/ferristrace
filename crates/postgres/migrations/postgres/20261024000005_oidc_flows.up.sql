CREATE TABLE oidc_flows (
    state text PRIMARY KEY,
    nonce text NOT NULL,
    code_verifier text NOT NULL,
    redirect_uri text NOT NULL,
    created_at bigint NOT NULL,
    expires_at bigint NOT NULL
);

CREATE INDEX oidc_flows_by_expiry ON oidc_flows (expires_at);
