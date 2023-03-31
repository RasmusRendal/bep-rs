CREATE TABLE device_options (
    id INTEGER PRIMARY KEY,
    device_name TEXT NOT NULL
);

CREATE TABLE sync_folders (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  dir_path TEXT UNIQUE NOT NULL
);

CREATE TABLE peers (
    id INTEGER PRIMARY KEY,
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
    modified_by BIGINT,
    sequence BIGINT,
    synced_version_id INTEGER,
    FOREIGN KEY (synced_version_id) REFERENCES sync_file_versions(id)
);

/* The version_id should be incrementing for each individual sync_file
   The regular id is just for the benefit of sqlite */
CREATE TABLE sync_file_versions (
    id INTEGER PRIMARY KEY,
    version_id BIGINT,
    sync_file_id BIGINT,
    user_id INTEGER,
    FOREIGN KEY (sync_file_id) REFERENCES sync_files(id)
);
