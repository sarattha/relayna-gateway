-- Optional operator-facing aliases; credentials and authentication remain unchanged.
ALTER TABLE api_keys ADD COLUMN name TEXT;
ALTER TABLE api_keys ADD CONSTRAINT api_keys_name_length
    CHECK (name IS NULL OR char_length(name) BETWEEN 1 AND 120);
