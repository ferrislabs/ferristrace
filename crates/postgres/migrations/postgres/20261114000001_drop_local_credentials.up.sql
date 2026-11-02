DROP TABLE IF EXISTS second_step_challenges;

DROP TABLE IF EXISTS recovery_codes;

DROP TABLE IF EXISTS auth_factors;

DROP TABLE IF EXISTS password_resets;

ALTER TABLE accounts DROP COLUMN IF EXISTS password_hash;
