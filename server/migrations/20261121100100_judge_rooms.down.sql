DROP TABLE judge_exchange_audit;
-- Room sessions can't become user sessions, so they go before user_id is required again.
DELETE FROM sessions WHERE room_id IS NOT NULL;
ALTER TABLE sessions
    DROP CONSTRAINT sessions_user_or_room,
    DROP COLUMN room_id,
    ALTER COLUMN user_id SET NOT NULL;
