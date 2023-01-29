use std::path::PathBuf;
use self::models::*;
use diesel::prelude::*;

pub mod models;
pub mod schema;

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
