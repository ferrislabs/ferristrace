CREATE TABLE teams (
    id uuid PRIMARY KEY,
    org_id uuid NOT NULL REFERENCES orgs (id) ON DELETE CASCADE,
    name text NOT NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    UNIQUE (org_id, name),
    -- So a membership below can point at the team and the organisation in one constraint.
    UNIQUE (org_id, id)
);

-- The org_id is carried rather than read through the team because it buys both of the things
-- below: leaving an organisation takes you off its teams, and no row can ever name a team in one
-- organisation and an account that belongs to another.
CREATE TABLE team_members (
    org_id uuid NOT NULL,
    team_id uuid NOT NULL,
    account_id uuid NOT NULL,
    created_at bigint NOT NULL DEFAULT ((extract(epoch FROM clock_timestamp()) * 1000000000)::bigint),
    PRIMARY KEY (team_id, account_id),
    FOREIGN KEY (org_id, team_id) REFERENCES teams (org_id, id) ON DELETE CASCADE,
    FOREIGN KEY (org_id, account_id) REFERENCES org_members (org_id, account_id) ON DELETE CASCADE
);

CREATE INDEX team_members_by_account ON team_members (org_id, account_id);

-- A project has one owner team or none, and disbanding a team leaves its projects behind rather
-- than taking them with it.
ALTER TABLE projects ADD COLUMN team_id uuid REFERENCES teams (id) ON DELETE SET NULL;

CREATE INDEX projects_by_team ON projects (team_id) WHERE team_id IS NOT NULL;
