//! The frontend's wire types are generated from the Rust DTOs.
//! Regenerate with `PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings`.

use std::path::Path;

const BINDINGS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../frontend/src/api/types.ts"
);

#[test]
fn ts_bindings_up_to_date() {
    let generated = plover_server::typescript_bindings();
    if std::env::var_os("PLOVER_UPDATE_BINDINGS").is_some() {
        std::fs::create_dir_all(Path::new(BINDINGS).parent().unwrap()).unwrap();
        std::fs::write(BINDINGS, &generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(BINDINGS).unwrap_or_default();
    assert!(
        committed == generated,
        "frontend/src/api/types.ts is stale; run PLOVER_UPDATE_BINDINGS=1 cargo test -p plover-server --test bindings"
    );
}
