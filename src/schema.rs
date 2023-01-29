// @generated automatically by Diesel CLI.

diesel::table! {
    syncfolders (id) {
        id -> Nullable<Text>,
        label -> Text,
        dir_path -> Text,
    }
}
