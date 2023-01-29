use std::path::PathBuf;
use diesel::prelude::*;

pub struct BepState {
    pub data_directory: PathBuf,
    connection: SqliteConnection,
}

impl BepState {
    pub fn new(mut data_directory: PathBuf) -> Self {
        data_directory.push("dq.sqlite");
        let connection = SqliteConnection::establish(data_directory.to_str().unwrap()).unwrap_or_else(|_| panic!("Error connecting to database"));
        BepState { data_directory: data_directory, connection: connection }
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
