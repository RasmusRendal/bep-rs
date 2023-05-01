// @generated automatically by Diesel CLI.

diesel::table! {
    device_options (id) {
        id -> Nullable<Integer>,
        device_name -> Text,
        cert -> Binary,
        key -> Binary,
    }
}

diesel::table! {
    folder_shares (id) {
        id -> Nullable<Integer>,
        sync_folder_id -> Text,
        peer_id -> Integer,
    }
}

diesel::table! {
    peer_addresses (id) {
        id -> Nullable<Integer>,
        address -> Text,
        peer_id -> Nullable<Integer>,
    }
}

diesel::table! {
    peers (id) {
        id -> Nullable<Integer>,
        device_id -> Nullable<Binary>,
        name -> Text,
    }
}

diesel::table! {
    sync_file_versions (id) {
        id -> Nullable<Integer>,
        version_id -> Nullable<BigInt>,
        sync_file_id -> Nullable<BigInt>,
        user_id -> Nullable<Integer>,
    }
}

diesel::table! {
    sync_files (id) {
        id -> Nullable<Integer>,
        modified_by -> Nullable<BigInt>,
        sequence -> Nullable<BigInt>,
        synced_version_id -> Nullable<Integer>,
    }
}

diesel::table! {
    sync_folders (id) {
        id -> Nullable<Text>,
        label -> Text,
        dir_path -> Text,
    }
}

diesel::joinable!(folder_shares -> peers (peer_id));
diesel::joinable!(folder_shares -> sync_folders (sync_folder_id));
diesel::joinable!(peer_addresses -> peers (peer_id));

diesel::allow_tables_to_appear_in_same_query!(
    device_options,
    folder_shares,
    peer_addresses,
    peers,
    sync_file_versions,
    sync_files,
    sync_folders,
);
