//! Contraseñas, tokens, cifrado de secretos y TOTP.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::TryRng;
use sha2::{Digest, Sha256};
use totp_rs::{Algorithm, Builder, Secret, Totp};

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
    Secret::generate().as_bytes().to_vec()
}

/// TOTP de la RFC 6238 con los parámetros de siempre (SHA-1, 6 dígitos, 30 s y
/// un paso de margen): si cambian, dejan de valer las apps ya configuradas.
pub fn totp(secret: &[u8], account: &str) -> Result<Totp> {
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(secret)
        .with_issuer(Some("Buzón"))
        .with_account_name(account)
        .build()
        .map_err(|_| Error::Crypto("semilla TOTP no válida"))
}

/// URL `otpauth://` para dar de alta la cuenta en la app (código QR).
pub fn totp_url(totp: &Totp) -> Result<String> {
    totp.to_url().map_err(|_| Error::Crypto("no se pudo crear la URL TOTP"))
}

/// `true` si el código vale ahora (con un paso de margen).
pub fn totp_check_current(totp: &Totp, code: &str) -> bool {
    totp.check_current(code).is_some()
}

pub fn totp_secret_base32(secret: &[u8]) -> String {
    Secret::from(secret).to_base32()
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
        assert_eq!(s.len(), 20);
        let t = totp(&s, "javier").unwrap();
        let code = t.generate_current().to_string();
        assert!(totp_check_current(&t, &code));
    }

    /// Lo que ve quien ya tiene el TOTP en el móvil (semilla en base32, URL del QR
    /// y códigos) tiene que salir igual que con totp-rs 5.7. Valores sacados de
    /// esa versión con la semilla de los vectores de prueba de la RFC 6238.
    #[test]
    fn totp_matches_previous_version() {
        let s = b"12345678901234567890";
        let t = totp(s, "javier").unwrap();
        assert_eq!(totp_secret_base32(s), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
        assert_eq!(totp_secret_base32(b"0123456789"), "GAYTEMZUGU3DOOBZ");
        assert_eq!(
            totp_url(&t).unwrap(),
            "otpauth://totp/Buz%C3%B3n:javier?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Buz%C3%B3n"
        );
        // RFC 6238, apéndice B (SHA-1), con 6 dígitos.
        assert_eq!(t.generate(59).to_string(), "287082");
        assert_eq!(t.generate(1111111109).to_string(), "081804");
        // Un paso de margen: vale en el paso siguiente, no en el de después.
        assert!(t.check("287082", 59).is_some());
        assert!(t.check("287082", 89).is_some());
        assert!(t.check("287082", 119).is_none());
    }
}
