DROP INDEX projects_by_team;
ALTER TABLE projects DROP COLUMN team_id;
DROP INDEX team_members_by_account;
DROP TABLE team_members;
DROP TABLE teams;
