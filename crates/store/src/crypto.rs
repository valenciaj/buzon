//! Contraseñas, tokens, cifrado de secretos y TOTP.

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, Salt, SaltString};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::TryRng;
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Secret, TOTP};

use crate::{Error, Result};

/// Bytes del generador aleatorio del sistema.
fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::rngs::SysRng
        .try_fill_bytes(&mut b)
        .expect("generador aleatorio del sistema");
    b
}

pub fn hash_password(password: &str) -> Result<String> {
    // Como SaltString::generate: 16 bytes aleatorios en B64.
    let salt = SaltString::encode_b64(&random_bytes::<{ Salt::RECOMMENDED_LENGTH }>())
        .map_err(|_| Error::Crypto("no se pudo crear la sal"))?;
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
    bs58::encode(random_bytes::<32>()).into_string()
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
        MasterKey(random_bytes())
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
        SecretBox(XChaCha20Poly1305::new(&Key::from(okm)))
    }
}

/// Cifra y descifra secretos pequeños (contraseñas IMAP, semillas TOTP).
pub struct SecretBox(XChaCha20Poly1305);

impl SecretBox {
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        // Nonce aleatorio de 24 bytes: con XChaCha20 la probabilidad de repetirlo es despreciable.
        let nonce = XNonce::from(random_bytes::<24>());
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
        let nonce = XNonce::try_from(nonce).expect("24 bytes");
        self.0
            .decrypt(&nonce, ct)
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

    /// Lo ya guardado (contraseñas IMAP/SMTP, semillas TOTP) se tiene que seguir
    /// pudiendo descifrar: dato cifrado con chacha20poly1305 0.10.
    #[test]
    fn secret_box_opens_data_sealed_by_previous_version() {
        let mk = MasterKey::from_hex(&"0123456789abcdef".repeat(4)).unwrap();
        let sealed = hex::decode(
            "4a2850df2ccff942bb64872aa4f7454fb3ca5f6a08410a8b0c1fd290390533ed\
             c71abae6ce15cba123c0d9a01b79179174f5e8f0dd275578fc215bcaa25e86",
        )
        .unwrap();
        let b = mk.derive("user:01J0000000000000000000000");
        assert_eq!(b.open(&sealed).unwrap(), "contraseña IMAP ñ €".as_bytes());
        assert!(mk.derive("user:otro").open(&sealed).is_err());
    }

    /// Hash hecho con la sal de SaltString::generate (antes de generarla aquí).
    #[test]
    fn password_hash_from_previous_version_verifies() {
        let old = "$argon2id$v=19$m=19456,t=2,p=1$VKJ+7hIc1kCdbBccD3TuuA$hO2SA15Avvp4mxcYaX4DA2195KxaoAjmu559jhA9U/o";
        assert!(verify_password("correcto caballo batería", old));
        assert!(!verify_password("otra", old));
        let new = hash_password("correcto caballo batería").unwrap();
        let salt = PasswordHash::new(&new).unwrap().salt.unwrap();
        assert_eq!(salt.as_str().len(), 22, "16 bytes en B64 sin relleno");
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
    fn opens_secrets_sealed_by_earlier_versions() {
        // Sellado con hkdf 0.12 y sha2 0.10: al actualizar dependencias, los secretos
        // ya guardados tienen que seguir abriéndose y los hashes de tokens no deben cambiar.
        let mk = MasterKey::from_hex(&"42".repeat(32)).unwrap();
        let sealed = hex::decode(
            "ee3aa0ea106b981fdd667379695115c6a399e65c8588cf5eb1425846e01f7c3493c60a02f18abdb16f13673737da701d09",
        )
        .unwrap();
        assert_eq!(mk.derive("user:kat").open(&sealed).unwrap(), b"imap-pass");
        assert_eq!(
            token_hash("tok"),
            "1a7674eb4ee78df7e1ac439a93c3fa8e3c945784d4dec9fd8e3011738b2f1d62"
        );
    }

    #[test]
    fn totp_accepts_current_code() {
        let s = new_totp_secret();
        let t = totp(&s, "javier").unwrap();
        let code = t.generate_current().unwrap();
        assert!(t.check_current(&code).unwrap());
    }
}
