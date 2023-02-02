CREATE TABLE syncfolders (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  dir_path TEXT UNIQUE NOT NULL
);

CREATE TABLE peers (
    id INTEGER PRIMARY KEY,
    name TEXT UNIQUE NOT NULL
);

CREATE TABLE peer_addresses (
    id INTEGER PRIMARY KEY,
    address TEXT NOT NULL,
    peer_id INTEGER,
    FOREIGN KEY(peer_id) REFERENCES peers(id)
);
