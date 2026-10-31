-- Sessions carry the version they were issued under; bumping it (password change, reset,
-- disabling) ends every older session.
ALTER TABLE users ADD COLUMN token_version INTEGER NOT NULL DEFAULT 0;
