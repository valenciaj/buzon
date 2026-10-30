//! Contraseñas, tokens, cifrado de secretos y TOTP.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::TryRng;
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};

use crate::{Error, Result};

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| Error::Crypto("no se pudo calcular el hash"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| Argon2::default().verify_password(password.as_bytes(), &h).is_ok())
        .unwrap_or(false)
}

/// Token aleatorio de 32 bytes en Base58, para sesiones e invitaciones.
pub fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::rngs::SysRng
        .try_fill_bytes(&mut b)
        .expect("generador aleatorio del sistema");
    bs58::encode(b).into_string()
}

/// Solo se guarda el hash de los tokens, nunca el token.
pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Clave maestra del servidor (32 bytes). Cada uso deriva su propia subclave.
#[derive(Clone)]
pub struct MasterKey([u8; 32]);

impl MasterKey {
    pub fn generate() -> Self {
        let mut b = [0u8; 32];
        rand::rngs::SysRng
            .try_fill_bytes(&mut b)
            .expect("generador aleatorio del sistema");
        MasterKey(b)
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        let v = hex::decode(s.trim()).map_err(|_| Error::Crypto("clave maestra no válida"))?;
        let b: [u8; 32] = v
            .try_into()
            .map_err(|_| Error::Crypto("la clave maestra debe tener 32 bytes"))?;
        Ok(MasterKey(b))
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Subclave para un contexto, por ejemplo `user:<id>` o `app:totp`.
    pub fn derive(&self, context: &str) -> SecretBox {
        let hk = Hkdf::<Sha256>::new(Some(b"buzon-v1"), &self.0);
        let mut okm = [0u8; 32];
        hk.expand(context.as_bytes(), &mut okm)
            .expect("32 bytes es una longitud válida");
        SecretBox(XChaCha20Poly1305::new(Key::from_slice(&okm)))
    }
}

/// Cifra y descifra secretos pequeños (contraseñas IMAP, semillas TOTP).
pub struct SecretBox(XChaCha20Poly1305);

impl SecretBox {
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let mut out = nonce.to_vec();
        out.extend(
            self.0
                .encrypt(&nonce, plain)
                .map_err(|_| Error::Crypto("no se pudo cifrar"))?,
        );
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        if sealed.len() < 24 {
            return Err(Error::Crypto("dato cifrado demasiado corto"));
        }
        let (nonce, ct) = sealed.split_at(24);
        self.0
            .decrypt(XNonce::from_slice(nonce), ct)
            .map_err(|_| Error::Crypto("no se pudo descifrar"))
    }
}

pub fn new_totp_secret() -> Vec<u8> {
    Secret::generate_secret().to_bytes().expect("secreto generado válido")
}

pub fn totp(secret: &[u8], account: &str) -> Result<TOTP> {
    TOTP::new(
        Algorithm::SHA1,
        6,
        1,
        30,
        secret.to_vec(),
        Some("Buzón".into()),
        account.into(),
    )
    .map_err(|_| Error::Crypto("semilla TOTP no válida"))
}

pub fn totp_secret_base32(secret: &[u8]) -> String {
    Secret::Raw(secret.to_vec()).to_encoded().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let h = hash_password("correcto caballo batería").unwrap();
        assert!(verify_password("correcto caballo batería", &h));
        assert!(!verify_password("otra", &h));
    }

    #[test]
    fn secret_box_roundtrip_and_isolation() {
        let mk = MasterKey::generate();
        let a = mk.derive("user:a");
        let sealed = a.seal(b"imap-pass").unwrap();
        assert_eq!(a.open(&sealed).unwrap(), b"imap-pass");
        assert!(mk.derive("user:b").open(&sealed).is_err());
    }

    #[test]
    fn master_key_hex_roundtrip() {
        let mk = MasterKey::generate();
        assert_eq!(MasterKey::from_hex(&mk.to_hex()).unwrap().to_hex(), mk.to_hex());
    }

    #[test]
    fn totp_accepts_current_code() {
        let s = new_totp_secret();
        let t = totp(&s, "javier").unwrap();
        let code = t.generate_current().unwrap();
        assert!(t.check_current(&code).unwrap());
    }
}
