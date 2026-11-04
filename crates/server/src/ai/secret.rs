//! Keeping users' API keys encrypted in the database, with the server's own key from
//! `NEXTTODO_SECRET_KEY`, which is never stored with the data: a copy of the database alone does not
//! give them away.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use chacha20poly1305::aead::{Aead, Generate, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};

const NONCE: usize = 24;

pub struct SecretKey(XChaCha20Poly1305);

impl SecretKey {
    /// From 32 bytes in base64, as `openssl rand -base64 32` gives them.
    pub fn from_base64(s: &str) -> anyhow::Result<SecretKey> {
        let bytes = STANDARD.decode(s.trim()).map_err(|_| anyhow::anyhow!("NEXTTODO_SECRET_KEY 不是 base64"))?;
        let key = Key::try_from(bytes.as_slice()).map_err(|_| anyhow::anyhow!("NEXTTODO_SECRET_KEY 要是 32 字节"))?;
        Ok(SecretKey(XChaCha20Poly1305::new(&key)))
    }

    /// `secret` encrypted for `user`: a random nonce, then the ciphertext. Bound to the user, so that
    /// it does not open as anyone else's.
    pub fn seal(&self, user: &str, secret: &str) -> Vec<u8> {
        let nonce = XNonce::generate();
        let sealed =
            self.0.encrypt(&nonce, Payload { msg: secret.as_bytes(), aad: user.as_bytes() }).expect("encrypting in memory succeeds");
        [nonce.as_slice(), &sealed].concat()
    }

    /// What `seal` gave for `user`; nothing when it was sealed with another key or for someone else.
    pub fn open(&self, user: &str, sealed: &[u8]) -> Option<String> {
        let (nonce, sealed) = sealed.split_at_checked(NONCE)?;
        let plain = self.0.decrypt(&XNonce::try_from(nonce).ok()?, Payload { msg: sealed, aad: user.as_bytes() }).ok()?;
        String::from_utf8(plain).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    #[test]
    fn seals_for_one_user() {
        let key = SecretKey::from_base64(KEY).unwrap();
        let sealed = key.seal("alice", "sk-123");
        assert_ne!(sealed, key.seal("alice", "sk-123"), "a new nonce each time");
        assert_eq!(key.open("alice", &sealed).as_deref(), Some("sk-123"));
        assert_eq!(key.open("bob", &sealed), None);
        let other = SecretKey::from_base64(&STANDARD.encode([7u8; 32])).unwrap();
        assert_eq!(other.open("alice", &sealed), None);
        assert!(SecretKey::from_base64("AAEC").is_err());
    }
}
