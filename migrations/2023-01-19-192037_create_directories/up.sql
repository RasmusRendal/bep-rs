CREATE TABLE syncfolders (
  id TEXT PRIMARY KEY,
  label TEXT NOT NULL,
  dir_path TEXT UNIQUE NOT NULL
)
