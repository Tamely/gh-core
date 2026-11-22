-- Design doc §5 notification_dispatches, with CHECKs and the escalation index.
CREATE TABLE notification_dispatches (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_type VARCHAR(48) NOT NULL,
    priority SMALLINT NOT NULL CHECK (priority >= 1), -- 1 = critical (push -> SMS -> email)
    push_sent_at TIMESTAMPTZ,
    push_acked_at TIMESTAMPTZ,
    sms_triggered_at TIMESTAMPTZ,
    sms_status VARCHAR(24) CHECK (sms_status IN ('DELIVERED', 'FAILED')),
    email_triggered_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER notification_dispatches_updated_at BEFORE UPDATE ON notification_dispatches
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE INDEX notification_dispatches_user_id ON notification_dispatches (user_id);
-- The escalation worker: critical pushes not acknowledged and not yet sent by SMS.
CREATE INDEX notification_dispatches_unacked_critical ON notification_dispatches (push_sent_at)
    WHERE priority = 1 AND push_acked_at IS NULL AND sms_triggered_at IS NULL;
