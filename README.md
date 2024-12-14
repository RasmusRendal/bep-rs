# bep-rs
Implementation of the Syncthing [Block Exchange Protocol](https://docs.syncthing.net/specs/bep-v1.html) in Rust.
Currently it supports syncing folders consisting only small files, smaller than the maximum block size.

## Roadmap
 - [X] Open a connection between two peers
 - [X] Authenticated by TLS
 - [X] Send an index of files in a directory
 - [X] Synchronize single files
 - [X] Send the cluster config
 - [X] Send a continuous ping, to keep connection open
 - [ ] Split files up into blocks
 - [ ] Support compressed messages
 - [ ] Support compressed files/metadata
 - [ ] Index update messages
 - [ ] Get it working with the Go client
 - [ ] DownloadProgress messages
