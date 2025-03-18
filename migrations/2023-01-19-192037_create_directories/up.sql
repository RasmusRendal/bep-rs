CREATE TABLE device_options (
    id INTEGER PRIMARY KEY,
    device_name TEXT NOT NULL,
    cert BLOB NOT NULL,
    key BLOB NOT NULL
);

CREATE TABLE sync_folders (
  id TEXT PRIMARY KEY NOT NULL,
  label TEXT NOT NULL,
  dir_path TEXT UNIQUE
);

CREATE TABLE peers (
    id INTEGER PRIMARY KEY,
    device_id BLOB,
    name TEXT UNIQUE NOT NULL
);

CREATE TABLE folder_shares (
    id INTEGER PRIMARY KEY,
    sync_folder_id TEXT NOT NULL,
    peer_id INTEGER NOT NULL,
    FOREIGN KEY(sync_folder_id) REFERENCES sync_folders(id),
    FOREIGN KEY(peer_id) REFERENCES peers(id)
);

CREATE TABLE peer_addresses (
    id INTEGER PRIMARY KEY,
    address TEXT NOT NULL,
    peer_id INTEGER,
    FOREIGN KEY(peer_id) REFERENCES peers(id)
);

CREATE TABLE sync_files (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    modified_by BIGINT NOT NULL,
    sequence BIGINT NOT NULL,
    synced_version_id BIGINT NOT NULL,
    hash BLOB,
    folder_id TEXT NOT NULL,
    FOREIGN KEY (synced_version_id) REFERENCES sync_file_versions(id),
    FOREIGN KEY (folder_id) REFERENCES sync_folders(id)
);

CREATE TABLE sync_file_blocks (
    id INTEGER PRIMARY KEY,
    sync_file_id INTEGER NOT NULL,
    offset BIGINT NOT NULL,
    size INTEGER NOT NULL,
    hash BLOB NOT NULL,
    weak_hash BIGINT NOT NULL,
    FOREIGN KEY (sync_file_id) REFERENCES sync_files(id)
);

/* The version_id should be incrementing for each individual sync_file
   The regular id is just for the benefit of sqlite */
CREATE TABLE sync_file_versions (
    id INTEGER PRIMARY KEY,
    version_id BIGINT NOT NULL,
    user_id BIGINT NOT NULL,
    sync_file_id INTEGER NOT NULL,
    FOREIGN KEY (sync_file_id) REFERENCES sync_files(id)
);
