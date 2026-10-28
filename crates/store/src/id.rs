use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::Error;

/// Clave primaria: UUIDv7 guardado como BLOB de 16 bytes y expuesto en Base58.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(Uuid);

impl Id {
    pub fn new() -> Self {
        Id(Uuid::now_v7())
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }

    pub fn from_slice(b: &[u8]) -> Result<Self, Error> {
        Uuid::from_slice(b).map(Id).map_err(|_| Error::BadId)
    }

    pub fn to_base58(&self) -> String {
        bs58::encode(self.0.as_bytes()).into_string()
    }
}

impl Default for Id {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_base58())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.to_base58())
    }
}

impl FromStr for Id {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let mut buf = [0u8; 16];
        let n = bs58::decode(s).onto(&mut buf).map_err(|_| Error::BadId)?;
        if n != 16 {
            return Err(Error::BadId);
        }
        Ok(Id(Uuid::from_bytes(buf)))
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_base58())
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base58_roundtrip() {
        let id = Id::new();
        let s = id.to_base58();
        assert!(s.len() <= 22);
        assert_eq!(s.parse::<Id>().unwrap(), id);
    }

    #[test]
    fn ids_sort_by_creation_time() {
        let a = Id::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = Id::new();
        assert!(a.as_bytes() < b.as_bytes());
    }

    #[test]
    fn rejects_bad_input() {
        assert!("0OIl".parse::<Id>().is_err());
        assert!("abc".parse::<Id>().is_err());
    }
}
