use super::bep_state_reference::BepStateRef;
use ring::digest;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek};
use std::path::PathBuf;
use std::time::SystemTime;

/// A block of a file
#[derive(Clone, Debug)]
pub struct SyncBlock {
    pub offset: i64,
    pub size: i32,
    pub hash: Vec<u8>,
}

/// A file that we should be syncing
#[derive(Clone, Debug)]
pub struct SyncFile {
    pub id: Option<i32>,
    /// Relative to the directory
    pub path: PathBuf,
    pub hash: Vec<u8>,
    pub modified_by: u64,
    pub synced_version: u64,
    /// A version consists of the device ID of the version author,
    /// and an incrementing counter, starting at zero. I don't know
    /// why we have the counter, but it's part of the BEP spec.
    pub versions: Vec<(u64, u64)>,
    pub blocks: Vec<SyncBlock>,
}

/// A directory that we should be syncing
#[derive(Clone, Debug)]
pub struct SyncDirectory {
    pub id: String,
    /// Human-readable name. By default, just the name of the directory.
    pub label: String,
    pub path: Option<PathBuf>,
}

fn comp_hashes(h1: &[u8], h2: &[u8]) -> bool {
    if h1.len() != h2.len() {
        return false;
    }
    for i in 0..h1.len() {
        if h1[i] != h2[i] {
            return false;
        }
    }
    true
}

impl SyncDirectory {
    /// Generates an index of the directory, based on the files on disk.
    /// Updates the database with new versions.
    pub async fn generate_index(&self, state: &BepStateRef) -> Vec<SyncFile> {
        // TODO: Handle errors in some manner
        log::info!("Generating index of directory {}", self.label);
        let short_id = state.get_short_id().await;
        let mut sync_files = state.get_sync_files(&self.id).await;
        let mut out_files: Vec<SyncFile> = Vec::new();
        let path = self.path.as_ref().unwrap();
        let mut changed = false;
        for file in path.read_dir().unwrap().flatten() {
            let mut buf_reader = BufReader::new(File::open(file.path()).unwrap());
            let mut data = Vec::new();
            buf_reader.read_to_end(&mut data).unwrap();
            let hash = digest::digest(&digest::SHA256, &data).as_ref().to_vec();

            let mut abs_path = path.clone();
            abs_path.push(file.path());

            match sync_files.iter_mut().find(|x| x.path == file.file_name()) {
                Some(index_file) => {
                    assert!(!index_file.versions.is_empty());

                    for v in &index_file.versions {
                        log::info!("Version {} was authored by {}", v.1, v.0);
                    }
                    log::info!("We are on version {}", index_file.synced_version);
                    // If we do not have the latest version of a file, we ignore any changes made locally
                    if index_file.versions.last().unwrap().1 > index_file.synced_version {
                        out_files.push(index_file.clone());
                        continue;
                    }

                    if !comp_hashes(&hash, &index_file.hash) {
                        changed = true;
                        log::info!("We have a new version of the file.");
                        log::info!(
                            "The previous hash was {:x?} while the current hash is {:x?}",
                            index_file.hash,
                            hash
                        );
                        let vnumber = index_file.versions.last().unwrap().1 + 1;
                        index_file.synced_version = vnumber;
                        index_file.versions.push((short_id, vnumber));
                        index_file.hash = hash;
                        index_file.blocks = SyncFile::gen_blocks(&file.path(), &self).unwrap();
                        state.update_sync_file(self, index_file).await;
                    }
                    out_files.push(index_file.clone());
                }
                None => {
                    let path = PathBuf::from(file.file_name());
                    out_files.push(SyncFile {
                        id: None,
                        path: path.clone(),
                        hash: hash.clone(),
                        modified_by: short_id,
                        synced_version: 0,
                        versions: vec![(short_id, 0)],
                        blocks: SyncFile::gen_blocks(&path, &self).unwrap(),
                    });
                    state
                        .update_sync_file(self, out_files.last().unwrap())
                        .await;
                    changed = true;
                }
            }
        }

        for file in &out_files {
            assert!(!file.versions.is_empty());
        }

        if changed {
            state.clone().directory_changed(self).await;
        }

        out_files
    }

    pub async fn get_index(&self, state: BepStateRef) -> Vec<SyncFile> {
        state.get_sync_files(&self.id).await
    }
}

impl SyncFile {
    /// For a file of a given size, get the block size
    fn get_blocksize(file_size: usize) -> usize {
        if file_size < 250 * 1024_usize.pow(2) {
            128 * 1024
        } else if file_size < 500 * 1024_usize.pow(2) {
            256 * 1024
        } else if file_size < 1024_usize.pow(3) {
            512 * 1024
        } else if file_size < 2 * 1024_usize.pow(3) {
            1024_usize.pow(2)
        } else if file_size < 4 * 1024_usize.pow(3) {
            2 * 1024_usize.pow(2)
        } else if file_size < 8 * 1024_usize.pow(3) {
            4 * 1024_usize.pow(2)
        } else if file_size < 16 * 1024_usize.pow(3) {
            8 * 1024_usize.pow(2)
        } else {
            16 * 1024_usize.pow(2)
        }
    }

    /// For a file on disk, generate a set of SyncBlocks
    pub fn gen_blocks(
        file_path: &PathBuf,
        dir: &SyncDirectory,
    ) -> Result<Vec<SyncBlock>, io::Error> {
        let mut path = dir
            .path
            .as_ref()
            .ok_or(io::Error::other("Non-synced directory"))?
            .clone();
        path.push(file_path);
        let mut h = File::open(path)?;
        let len = h.metadata()?.len() as u32;

        let block_len = SyncFile::get_blocksize(len as usize);
        let num_blocks = len.div_ceil(block_len as u32);
        let mut blocks = Vec::new();
        for _ in 0..(num_blocks - 1) {
            let offset = h.stream_position()? as i64;
            let mut buf = vec![0; block_len];
            h.read_exact(&mut buf)?;
            blocks.push(SyncBlock {
                offset,
                size: block_len as i32,
                hash: digest::digest(&digest::SHA256, &buf).as_ref().to_vec(),
            });
        }

        let mut buf = vec![];
        h.read_to_end(&mut buf)?;
        blocks.push(SyncBlock {
            offset: (block_len as i64) * (num_blocks - 1) as i64,
            size: (len as i32) % block_len as i32,
            hash: digest::digest(&digest::SHA256, &buf).as_ref().to_vec(),
        });
        Ok(blocks)
    }

    pub fn get_index_version(&self) -> u64 {
        match self.versions.last() {
            Some(v) => v.1,
            None => 1,
        }
    }

    pub fn modified_s(&self) -> i64 {
        if self.path.is_file() {
            self.path
                .metadata()
                .unwrap()
                .modified()
                .map(|x| x.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs() as i64)
                .unwrap_or(-1)
        } else {
            -1
        }
    }

    pub fn get_size(&self) -> u64 {
        self.blocks.iter().fold(0, |acc, b| acc + (b.size as u64))
    }

    /// Gets the `name` as formatted in BEP. That means the path of the file,
    /// relative to the sync directory, separated by / regardless of OS
    pub fn get_name(&self) -> String {
        self.path
            .components()
            .fold("".to_string(), |acc, x| {
                acc + "/" + x.as_os_str().to_str().unwrap()
            })
            .trim_matches('/')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;
    use crate::bep_state::BepState;
    use std::fs::File;
    use std::io::Write;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_generate_index() {
        let statedir = tempfile::tempdir().unwrap().into_path();
        let mut state = BepState::new(statedir);

        let file_contents = "hello world";
        let filename = "testfile";
        let hash: Vec<u8> = b"\xb9\x4d\x27\xb9\x93\x4d\x3e\x08\xa5\x2e\x52\xd7\xda\x7d\xab\xfa\xc4\x84\xef\xe3\x7a\x53\x80\xee\x90\x88\xf7\xac\xe2\xef\xcd\xe9".to_vec();

        let path = tempfile::tempdir().unwrap().into_path();
        let mut helloworld = path.clone();
        helloworld.push(filename);
        {
            let mut o = File::create(helloworld.clone()).unwrap();
            o.write_all(file_contents.as_bytes()).unwrap();
        }

        let directory = state.add_sync_directory(Some(path.clone()), "testdir".to_string(), None);

        let mut index = directory
            .generate_index(&BepStateRef::from_bepstate(state))
            .await;
        assert!(index.len() == 1);
        let fileinfo = index.pop().unwrap();
        assert!(fileinfo.path == PathBuf::from(filename));
        assert!(fileinfo.hash == hash);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_get_blocks() {
        let statedir = tempfile::tempdir().unwrap().into_path();
        let mut state = BepState::new(statedir);

        let mut file_contents = vec![0u8; 192 * 1024];
        rand::rng().fill(&mut file_contents[..]);
        let filename = "testfile";

        let path = tempfile::tempdir().unwrap().into_path();
        let mut helloworld = path.clone();
        helloworld.push(filename);
        {
            let mut o = File::create(helloworld.clone()).unwrap();
            o.write_all(&file_contents).unwrap();
        }

        let directory = state.add_sync_directory(Some(path.clone()), "testdir".to_string(), None);

        let mut index = directory
            .generate_index(&BepStateRef::from_bepstate(state))
            .await;
        assert!(index.len() == 1);
        let fileinfo = index.pop().unwrap();
        assert_eq!(fileinfo.path, PathBuf::from(filename));
        assert_eq!(fileinfo.blocks.len(), 2);

        assert_eq!(fileinfo.blocks[0].offset, 0);
        assert_eq!(fileinfo.blocks[0].size, 128 * 1024);
        assert_eq!(fileinfo.blocks[1].offset, 128 * 1024);
        assert_eq!(fileinfo.blocks[1].size, 64 * 1024);
    }

    #[test]
    fn test_get_name() {
        let mut filepath = PathBuf::new();
        filepath.push("somefile");
        let file = SyncFile {
            id: None,
            path: filepath,
            hash: vec![],
            modified_by: 0,
            synced_version: 0,
            versions: vec![(1, 1)],
            blocks: vec![SyncBlock {
                offset: 0,
                size: 0,
                hash: vec![],
            }],
        };

        assert_eq!(file.get_name(), "somefile");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_update_index() {
        // Tests that a new version of the file is created in the database,
        // only when we change the file
        let _ = env_logger::builder().is_test(true).try_init();
        let statedir = tempfile::tempdir().unwrap().into_path();
        let state = BepStateRef::new(statedir);

        let file_contents1 = "hello world";
        let file_contents2 = "some other contents";
        let filename = "testfile";
        let hash1: Vec<u8> = b"\xb9\x4d\x27\xb9\x93\x4d\x3e\x08\xa5\x2e\x52\xd7\xda\x7d\xab\xfa\xc4\x84\xef\xe3\x7a\x53\x80\xee\x90\x88\xf7\xac\xe2\xef\xcd\xe9".to_vec();
        let hash2: Vec<u8> = b"\xb9\x59\x57\xc2\x9f\xf5\xe0\xea\xb4\x0c\x20\xcb\xc4\x41\xe8\x6d\x81\xb5\xad\x1a\x59\x19\x99\x36\x7b\xa1\x8c\x9b\x4d\xa7\xab\xae".to_vec();
        let mut relpath = PathBuf::new();
        relpath.push(filename);

        let path = tempfile::tempdir().unwrap().into_path();
        let mut helloworld = path.clone();
        helloworld.push(filename);
        {
            let mut o = File::create(helloworld.clone()).unwrap();
            o.write_all(file_contents1.as_bytes()).unwrap();
        }

        let directory = state
            .add_sync_directory(Some(path.clone()), "testdir".to_string(), None)
            .await;

        // Generate the first index
        let mut index = directory.generate_index(&state).await;
        assert_eq!(index.len(), 1);
        let mut fileinfo = index.pop().unwrap();
        assert_eq!(fileinfo.path, relpath);
        assert_eq!(fileinfo.hash, hash1);
        assert_eq!(fileinfo.versions.len(), 1);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert_eq!(fileversion.0, state.get_short_id().await);
        assert_eq!(fileversion.1, 0);
        log::info!("Successfully generated first index");

        // Call the generate index again. Since we have not changed the file,
        // nothing should have changed.
        let mut index = directory.generate_index(&state).await;
        assert_eq!(index.len(), 1);
        let mut fileinfo = index.pop().unwrap();
        assert_eq!(fileinfo.path, relpath);
        assert_eq!(fileinfo.hash, hash1);
        assert_eq!(fileinfo.versions.len(), 1);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert_eq!(fileversion.0, state.get_short_id().await);
        assert_eq!(fileversion.1, 0);
        log::info!("Successfully generated second index");

        // Change the file, and create a third index. This one should have
        // two versions.
        {
            let mut o = File::create(helloworld.clone()).unwrap();
            o.write_all(file_contents2.as_bytes()).unwrap();
        }

        let mut index = directory.generate_index(&state).await;
        assert_eq!(index.len(), 1);
        let mut fileinfo = index.pop().unwrap();
        assert_eq!(fileinfo.path, relpath);
        assert_eq!(fileinfo.hash, hash2);
        assert_eq!(fileinfo.synced_version, 1);

        assert_eq!(fileinfo.versions.len(), 2);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert_eq!(fileversion.0, state.get_short_id().await);
        assert_eq!(fileversion.1, 1);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert_eq!(fileversion.0, state.get_short_id().await);
        assert_eq!(fileversion.1, 0);

        log::info!("Successfully generated third index");
    }
}
