-- What a person decided about an issue, and nothing else. The count, the message and the
-- timestamps live in ClickHouse and expire with the spans that produced them; this row is the
-- half that must not. An issue nobody has touched has no row here, which is why `unresolved` is
-- the default in the domain rather than a value somebody has to write.
--
-- The key is the pair that defines an issue: a project and a fingerprint. There is no foreign key
-- on project_id on purpose. The value is minted by the ingestion path, which writes the nil uuid
-- for a payload that named no project, and ClickHouse holds it happily; a foreign key here would
-- make Postgres the authority over a key it does not issue, and would refuse a decision on an
-- issue the user can plainly see. The price is that deleting a project leaves its decisions
-- behind, which costs a few rows and no correctness.
--
-- decided_at and decided_by describe the status and only the status, because a resolved issue is
-- regressed when an occurrence lands after decided_at. Assigning an owner must not move that
-- moment or a reassignment would quietly clear a regression, so `assign` never writes these two.
CREATE TABLE issue_decisions (
    project_id uuid NOT NULL,
    fingerprint bytea NOT NULL,
    status text NOT NULL,
    owner_id uuid REFERENCES accounts (id) ON DELETE SET NULL,
    decided_at bigint NOT NULL,
    decided_by uuid REFERENCES accounts (id) ON DELETE SET NULL,
    PRIMARY KEY (project_id, fingerprint)
);

CREATE INDEX issue_decisions_by_owner ON issue_decisions (owner_id);
