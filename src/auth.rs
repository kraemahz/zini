use base64::Engine;
use rand::RngCore;
use scrypt::{
    password_hash::{
        errors::Result as HashResult,
        rand_core::OsRng,
        PasswordHasher, SaltString, Salt,
    },
    Scrypt
};


#[inline]
pub fn generate_random_token(size: usize) -> Vec<u8> {
    let mut rng = OsRng::default();
    let mut bytes = vec![0; size];
    rng.fill_bytes(&mut bytes);
    bytes
}


#[inline]
pub fn to_base64(bytes: &[u8]) -> String {
    let engine = base64::engine::general_purpose::URL_SAFE;
    engine.encode(bytes)
}


pub fn generate_salt() -> Vec<u8> {
    let mut buf = vec![0u8; Salt::RECOMMENDED_LENGTH];
    OsRng.fill_bytes(&mut buf);
    buf
}


pub fn salt_and_hash(target: &str, salt: &[u8]) -> HashResult<String> {
    let salt = SaltString::encode_b64(salt)?;
    let target = target.as_bytes();
    Ok(Scrypt.hash_password(target, &salt)?.to_string())
}
