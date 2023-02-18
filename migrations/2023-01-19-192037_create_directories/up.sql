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
