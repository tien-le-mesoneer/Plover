//! Host operations a worker answers itself: random bytes for `uuid`/`nanoid`
//! (`crypto.getRandomValues`) and JWT signatures for `jsonwebtoken`. Everything else
//! (HTTP, cookies) goes to the runner.

use base64::Engine as _;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey};
use serde_json::{Value, json};

/// Web Crypto's `getRandomValues` limit.
const MAX_RANDOM_BYTES: usize = 65_536;

/// `Some` when `op` is a built-in operation.
pub(super) fn builtin(op: &str, args: &Value) -> Option<Result<Value, String>> {
    Some(match op {
        "random" => random(args),
        "jwt.sign" => jwt_sign(args),
        "jwt.verify" => jwt_verify(args),
        _ => return None,
    })
}

fn random(args: &Value) -> Result<Value, String> {
    let n = args.get("n").and_then(Value::as_u64).unwrap_or(0) as usize;
    if n > MAX_RANDOM_BYTES {
        return Err(format!(
            "The ArrayBufferView's byte length ({n}) exceeds the number of bytes of entropy available via this API ({MAX_RANDOM_BYTES})"
        ));
    }
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).map_err(|e| format!("no randomness available: {e}"))?;
    Ok(json!(bytes))
}

fn text<'a>(args: &'a Value, key: &str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn algorithm(args: &Value) -> Result<Algorithm, String> {
    let name = text(args, "alg");
    name.parse()
        .map_err(|_| format!("\"{name}\" is not a valid algorithm."))
}

/// The key bytes: a string as UTF-8, or a `Buffer` sent as base64.
fn key_bytes(args: &Value) -> Result<Vec<u8>, String> {
    let key = text(args, "key");
    if text(args, "keyEncoding") == "base64" {
        base64::engine::general_purpose::STANDARD
            .decode(key)
            .map_err(|e| format!("invalid key: {e}"))
    } else {
        Ok(key.as_bytes().to_vec())
    }
}

fn is_hmac(alg: Algorithm) -> bool {
    matches!(alg, Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512)
}

fn encoding_key(alg: Algorithm, key: &[u8]) -> Result<EncodingKey, String> {
    let pem = |r: jsonwebtoken::errors::Result<EncodingKey>| {
        r.map_err(|e| format!("secretOrPrivateKey must be an asymmetric key when using {alg:?}: {e}"))
    };
    match alg {
        a if is_hmac(a) => Ok(EncodingKey::from_secret(key)),
        Algorithm::ES256 | Algorithm::ES384 => pem(EncodingKey::from_ec_pem(key)),
        Algorithm::EdDSA => pem(EncodingKey::from_ed_pem(key)),
        _ => pem(EncodingKey::from_rsa_pem(key)),
    }
}

fn decoding_key(alg: Algorithm, key: &[u8]) -> Result<DecodingKey, String> {
    let pem = |r: jsonwebtoken::errors::Result<DecodingKey>| {
        r.map_err(|e| format!("secretOrPublicKey must be an asymmetric key when using {alg:?}: {e}"))
    };
    match alg {
        a if is_hmac(a) => Ok(DecodingKey::from_secret(key)),
        Algorithm::ES256 | Algorithm::ES384 => pem(DecodingKey::from_ec_pem(key)),
        Algorithm::EdDSA => pem(DecodingKey::from_ed_pem(key)),
        _ => pem(DecodingKey::from_rsa_pem(key)),
    }
}

/// Signs `input` (`base64url(header).base64url(payload)`, built in JS so the bytes match
/// node's `jsonwebtoken`) and returns the base64url signature.
fn jwt_sign(args: &Value) -> Result<Value, String> {
    let alg = algorithm(args)?;
    let key = encoding_key(alg, &key_bytes(args)?)?;
    jsonwebtoken::crypto::sign(text(args, "input").as_bytes(), &key, alg)
        .map(Value::String)
        .map_err(|e| e.to_string())
}

fn jwt_verify(args: &Value) -> Result<Value, String> {
    let alg = algorithm(args)?;
    let key = decoding_key(alg, &key_bytes(args)?)?;
    let valid = jsonwebtoken::crypto::verify(
        text(args, "signature"),
        text(args, "input").as_bytes(),
        &key,
        alg,
    )
    .unwrap_or(false);
    Ok(Value::Bool(valid))
}
