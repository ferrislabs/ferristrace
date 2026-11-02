CREATE TABLE postmortems (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    incident_id uuid NOT NULL UNIQUE REFERENCES incidents (id) ON DELETE CASCADE,
    status text NOT NULL CHECK (status IN ('draft', 'published')),
    body text NOT NULL,
    author_id uuid REFERENCES accounts (id) ON DELETE SET NULL,
    edited boolean NOT NULL DEFAULT false,
    created_ms bigint NOT NULL,
    updated_ms bigint NOT NULL,
    published_ms bigint,
    CONSTRAINT postmortems_published_once_published CHECK ((status = 'published') = (published_ms IS NOT NULL))
);

CREATE INDEX postmortems_by_org ON postmortems (org_id);
