-- Drop event_payload column from trigger_queue
ALTER TABLE trigger_queue DROP COLUMN event_payload;
