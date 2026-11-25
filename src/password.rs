//! Argon2id password hashing.

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::{Algorithm, Argon2, Params, Version};

#[derive(Clone)]
pub struct Hasher {
    argon: Argon2<'static>,
    /// A valid hash of a random password, verified against when the account does not exist so
    /// that unknown-email and wrong-password logins cost about the same.
    decoy: String,
}

impl Hasher {
    /// The argon2 crate defaults (Argon2id, 19 MiB, 2 passes, 1 lane).
    pub fn standard() -> Self {
        Self::with_params(Params::default())
    }

    /// Deliberately weak parameters so the test suite does not spend seconds per hash.
    /// Never use outside tests.
    pub fn cheap_for_tests() -> Self {
        Self::with_params(Params::new(8, 1, 1, None).expect("valid argon2 parameters"))
    }

    fn with_params(params: Params) -> Self {
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let salt = SaltString::generate(&mut OsRng);
        let decoy = argon
            .hash_password(b"decoy-password-never-matched", &salt)
            .expect("hashing a constant cannot fail")
            .to_string();
        Self { argon, decoy }
    }

    pub fn hash(&self, password: &str) -> Result<String, argon2::password_hash::Error> {
        let salt = SaltString::generate(&mut OsRng);
        Ok(self
            .argon
            .hash_password(password.as_bytes(), &salt)?
            .to_string())
    }

    pub fn verify(&self, password: &str, stored_hash: &str) -> bool {
        match PasswordHash::new(stored_hash) {
            Ok(parsed) => self
                .argon
                .verify_password(password.as_bytes(), &parsed)
                .is_ok(),
            Err(_) => false,
        }
    }

    /// Spends the cost of a verification without any chance of success.
    pub fn verify_decoy(&self, password: &str) {
        let _ = self.verify(password, &self.decoy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_password_verifies_and_wrong_one_does_not() {
        let h = Hasher::cheap_for_tests();
        let stored = h.hash("correct horse").unwrap();
        assert!(h.verify("correct horse", &stored));
        assert!(!h.verify("wrong horse", &stored));
    }

    #[test]
    fn same_password_gets_different_salts() {
        let h = Hasher::cheap_for_tests();
        assert_ne!(h.hash("same").unwrap(), h.hash("same").unwrap());
    }

    #[test]
    fn a_corrupt_stored_hash_never_verifies() {
        let h = Hasher::cheap_for_tests();
        assert!(!h.verify("anything", "not-a-phc-string"));
    }
}
