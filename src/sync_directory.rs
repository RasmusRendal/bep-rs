use super::bep_state::BepState;
use ring::digest;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

/// A block of a file
#[derive(Clone)]
pub struct SyncBlock {
    pub offset: u64,
    pub size: u32,
    pub hash: Vec<u8>,
}

/// A file that we should be syncing
#[derive(Clone)]
pub struct SyncFile {
    pub id: Option<i32>,
    pub path: PathBuf,
    pub hash: Vec<u8>,
    pub modified_by: u64,
    pub synced_version: u64,
    pub versions: Vec<(u64, u64)>,
}

/// A directory that we should be syncing
#[derive(Clone)]
pub struct SyncDirectory {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

fn comp_hashes(h1: &Vec<u8>, h2: &Vec<u8>) -> bool {
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
    pub fn generate_index(&self, state: &mut BepState) -> Vec<SyncFile> {
        let short_id = state.get_short_id();
        let path = self.path.clone();
        let mut files = state.get_sync_files(&self.id);
        for file in path.read_dir().unwrap().flatten() {
            let mut buf_reader = BufReader::new(File::open(file.path()).unwrap());
            let mut data = Vec::new();
            buf_reader.read_to_end(&mut data).unwrap();
            let hash = digest::digest(&digest::SHA256, &data).as_ref().to_vec();

            match files.iter_mut().find(|x| x.path == file.path()) {
                Some(index_file) => {
                    if index_file.versions.last().unwrap().1 == index_file.synced_version {
                        if !comp_hashes(&hash, &index_file.hash) {
                            let vnumber = index_file.versions.last().unwrap().1 + 1;
                            index_file.versions.push((short_id, vnumber));
                            state.update_sync_file(self, index_file);
                        }
                    }
                }
                None => {
                    files.push(SyncFile {
                        id: None,
                        path: file.path().clone(),
                        hash,
                        modified_by: short_id,
                        synced_version: 1,
                        versions: vec![(short_id, 1)],
                    });
                    state.update_sync_file(self, files.last().unwrap());
                }
            }
        }
        files
    }
}

impl SyncFile {
    pub fn get_blocks(&self) -> Vec<SyncBlock> {
        let path = self.path.clone();
        let h = File::open(path).unwrap();
        let len = h.metadata().unwrap().len() as u32;
        // Here we may use only one block.
        // TODO: Support more blocks
        assert!(len < 1024 * 1024 * 16);
        let block = SyncBlock {
            offset: 0,
            size: len,
            hash: self.hash.clone(),
        };
        vec![block]
    }

    pub fn get_size(&self) -> u64 {
        self.get_blocks()
            .iter()
            .fold(0, |acc, b| acc + (b.size as u64))
    }

    pub fn get_name(&self, directory: &SyncDirectory) -> String {
        self.path
            .components()
            .skip(directory.path.components().count())
            .fold("".to_string(), |acc, x| {
                acc + "/" + x.as_os_str().to_str().unwrap()
            })
            .trim_matches('/')
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_generate_index() {
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

        let directory = state.add_sync_directory(path.clone(), None);

        let mut index = directory.generate_index(&mut state);
        assert!(index.len() == 1);
        let fileinfo = index.pop().unwrap();
        assert!(fileinfo.path == helloworld);
        assert!(fileinfo.hash == hash);
    }

    #[test]
    fn test_get_name() {
        let path = tempfile::tempdir().unwrap().into_path();
        let directory = SyncDirectory {
            id: "someid".to_string(),
            label: "dir".to_string(),
            path: path.clone(),
        };

        let mut filepath = path.clone();
        filepath.push("somefile");
        let file = SyncFile {
            id: None,
            path: filepath,
            hash: vec![],
            modified_by: 0,
            synced_version: 0,
            versions: vec![],
        };

        assert!(file.get_name(&directory) == "somefile");
    }

    #[test]
    fn test_update_index() {
        // Tests that a new version of the file is created in the database,
        // only when we change the file
        let _ = env_logger::builder().is_test(true).try_init();
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

        let directory = state.add_sync_directory(path.clone(), None);

        let mut index = directory.generate_index(&mut state);
        assert!(index.len() == 1);
        let mut fileinfo = index.pop().unwrap();
        assert!(fileinfo.path == helloworld);
        assert!(fileinfo.hash == hash);
        assert!(fileinfo.versions.len() == 1);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert!(fileversion.0 == state.get_short_id());
        assert!(fileversion.1 == 1);
        log::info!("Successfully generated first index");

        let mut index = directory.generate_index(&mut state);
        assert!(index.len() == 1);
        let mut fileinfo = index.pop().unwrap();
        assert!(fileinfo.path == helloworld);
        assert!(fileinfo.hash == hash);
        assert!(fileinfo.versions.len() == 1);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert!(fileversion.0 == state.get_short_id());
        assert!(fileversion.1 == 1);
        log::info!("Successfully generated second index");

        {
            let mut o = File::create(helloworld.clone()).unwrap();
            o.write_all("some new contents".as_bytes()).unwrap();
        }

        let mut index = directory.generate_index(&mut state);
        assert!(index.len() == 1);
        let mut fileinfo = index.pop().unwrap();
        assert!(fileinfo.versions.len() == 2);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert!(fileversion.0 == state.get_short_id());
        assert!(fileversion.1 == 2);
        let fileversion = fileinfo.versions.pop().unwrap();
        assert!(fileversion.0 == state.get_short_id());
        assert!(fileversion.1 == 1);
        log::info!("Successfully generated third index");
    }
}
