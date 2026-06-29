//! One-shot binary to generate an Argon2id hash for a password.
//! Used by glia-test/.env.example so devs have a known-good hash for
//! password `glia`. Re-run with `cargo run -p glia-test-hashgen --
//! <password>` to regenerate.

use argon2::PasswordHasher;
use argon2::password_hash::{SaltString, rand_core::OsRng};

fn main() {
    let password = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "glia".to_string());
    let salt = SaltString::generate(&mut OsRng);
    let hash = argon2::Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .expect("hash")
        .to_string();
    println!("{hash}");
}
