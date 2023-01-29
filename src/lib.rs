pub mod models;
pub mod schema;

use std::path::PathBuf;
use self::models::*;
use diesel::prelude::*;

use rand::distributions::{Alphanumeric, DistString};

pub struct BepState {
    pub data_directory: PathBuf,
    connection: SqliteConnection,
}

impl BepState {
    pub fn new(mut data_directory: PathBuf) -> Self {
        data_directory.push("db.sqlite");
        let connection = SqliteConnection::establish(data_directory.to_str().unwrap()).unwrap_or_else(|_| panic!("Error connecting to database"));
        BepState { data_directory: data_directory, connection: connection }
    }

    pub fn get_sync_directories(&mut self) -> Vec<Syncfolder> {
        use self::schema::syncfolders::dsl::*;
        syncfolders.load::<Syncfolder>(&mut self.connection).unwrap_or(Vec::new())
    }

    pub fn add_sync_directory(&mut self, path: PathBuf) {
        use crate::schema::syncfolders;
        use crate::schema::syncfolders::dsl::*;

        if !path.exists() {
            panic!("Folder {} not found", path.display());
        }

        let folder_id: String = Alphanumeric.sample_string(&mut rand::thread_rng(), 12);

        let new_dir = Syncfolder {
            id: Some(folder_id),
            label: path.file_name().unwrap().to_owned().into_string().unwrap(),
            dir_path: path.display().to_string()
        };

        diesel::insert_into(syncfolders::table)
            .values(&new_dir)
            .execute(&mut self.connection)
            .expect("Error adding directory");

    }

}

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
