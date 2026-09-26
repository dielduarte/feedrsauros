-- One row of app-wide settings.
CREATE TABLE settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    ai_enabled INTEGER NOT NULL DEFAULT 0,
    -- Nonce followed by ciphertext; the encryption key lives in a separate file.
    typesafe_api_key BLOB,
    -- The key's last characters, so it can be recognised without decrypting it.
    typesafe_api_key_hint TEXT,
    CHECK ((typesafe_api_key IS NULL) = (typesafe_api_key_hint IS NULL)),
    CHECK (ai_enabled = 0 OR typesafe_api_key IS NOT NULL)
);

INSERT INTO settings (id) VALUES (1);
