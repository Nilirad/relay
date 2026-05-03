-- Add branch_id and new_hash columns to trigger_queue
ALTER TABLE trigger_queue ADD COLUMN branch_id INTEGER;
ALTER TABLE trigger_queue ADD COLUMN new_hash TEXT;
