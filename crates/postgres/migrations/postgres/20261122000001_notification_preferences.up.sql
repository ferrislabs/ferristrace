ALTER TABLE accounts ADD COLUMN notify_email_enabled boolean;
ALTER TABLE accounts ADD COLUMN notify_weekly_digest boolean;
ALTER TABLE accounts ADD COLUMN notify_channels jsonb;
