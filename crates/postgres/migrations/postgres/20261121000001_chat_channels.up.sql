ALTER TABLE alert_channels DROP CONSTRAINT alert_channels_kind_check;

ALTER TABLE alert_channels
    ADD CONSTRAINT alert_channels_kind_check CHECK (kind IN ('email', 'webhook', 'slack', 'discord'));
