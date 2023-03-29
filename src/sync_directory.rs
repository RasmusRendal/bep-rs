use ring::digest;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

/// A block of a file
#[derive(Clone)]
pub struct Block {
    pub offset: u64,
    pub size: u32,
    pub hash: Vec<u8>,
}

/// A file that we should be syncing
#[derive(Clone)]
pub struct SyncFile {
    pub name: String,
    pub hash: Vec<u8>,
}

/// A directory that we should be syncing
#[derive(Clone)]
pub struct SyncDirectory {
    pub id: String,
    pub label: String,
    pub path: PathBuf,
}

impl SyncDirectory {
    pub fn generate_index(&self) -> Vec<SyncFile> {
        let path = self.path.clone();
        let mut files: Vec<SyncFile> = Vec::new();
        for e in path.read_dir().unwrap() {
            if let Ok(file) = e {
                let mut buf_reader = BufReader::new(File::open(file.path()).unwrap());
                let mut data = Vec::new();
                buf_reader.read_to_end(&mut data).unwrap();

                let hash = digest::digest(&digest::SHA256, &data).as_ref().to_vec();
                files.push(SyncFile {
                    name: file.file_name().into_string().unwrap(),
                    hash,
                });
            }
        }
        files
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn test_generate_index() {
        let file_contents = "hello world";
        let filename = "testfile";
        let hash: Vec<u8> = b"\xb9\x4d\x27\xb9\x93\x4d\x3e\x08\xa5\x2e\x52\xd7\xda\x7d\xab\xfa\xc4\x84\xef\xe3\x7a\x53\x80\xee\x90\x88\xf7\xac\xe2\xef\xcd\xe9".to_vec();

        let path = tempfile::tempdir().unwrap().into_path();
        {
            let mut helloworld = path.clone();
            helloworld.push(filename);
            let mut o = File::create(helloworld).unwrap();
            o.write_all(file_contents.as_bytes()).unwrap();
        }

        let directory = SyncDirectory {
            id: "someid".to_string(),
            label: "dir".to_string(),
            path,
        };
        let mut index = directory.generate_index();
        assert!(index.len() == 1);
        let fileinfo = index.pop().unwrap();
        assert!(fileinfo.name == filename);
        assert!(fileinfo.hash == hash);
    }
}
