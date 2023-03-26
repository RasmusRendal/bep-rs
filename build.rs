extern crate prost_build;

fn main() {
    prost_build::compile_protos(
        &["src/peer_connection/items.proto"],
        &["src/peer_connection/"],
    )
    .unwrap();
}
