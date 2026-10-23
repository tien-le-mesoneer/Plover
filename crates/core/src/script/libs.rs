//! The safe-mode libraries scripts can use; see `crates/core/js/README.md`. Each one is
//! compiled once per worker and evaluated in a context only when a script first touches it.

/// `(name, source)`; names match the keys `prelude.js` asks `__brsLoadLib` for.
pub const LIBS: &[(&str, &str)] = &[
    ("chai", include_str!("../../js/dist/chai.js")),
    ("buffer", include_str!("../../js/dist/buffer.js")),
    ("moment", include_str!("../../js/dist/moment.js")),
    ("crypto-js", include_str!("../../js/dist/crypto-js.js")),
    ("tv4", include_str!("../../js/dist/tv4.js")),
    ("ajv", include_str!("../../js/dist/ajv.js")),
    ("uuid", include_str!("../../js/dist/uuid.js")),
    ("nanoid", include_str!("../../js/dist/nanoid.js")),
    ("jsonwebtoken", include_str!("../../js/dist/jwt.js")),
];
