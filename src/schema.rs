// @generated automatically by Diesel CLI.

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
        name -> Text,
    }
}

diesel::table! {
    syncfolders (id) {
        id -> Nullable<Text>,
        label -> Text,
        dir_path -> Text,
    }
}

diesel::joinable!(peer_addresses -> peers (peer_id));

diesel::allow_tables_to_appear_in_same_query!(
    peer_addresses,
    peers,
    syncfolders,
);
