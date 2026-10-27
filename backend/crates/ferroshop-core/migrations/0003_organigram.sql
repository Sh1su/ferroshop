-- Reporting lines for the organigram ------------------------------------------

ALTER TABLE admin_user
    ADD COLUMN manager_id uuid REFERENCES admin_user (id) ON DELETE SET NULL,
    ADD COLUMN job_title  text,
    ADD COLUMN department text;

CREATE INDEX admin_user_manager_idx ON admin_user (manager_id);
