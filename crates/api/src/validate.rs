use crate::error::{ApiError, ApiResult};

pub const LANGUAGES: &[&str] = &["es", "en"];

pub fn username(u: &str) -> ApiResult<()> {
    let ok = (3..=32).contains(&u.chars().count())
        && u.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "El usuario debe tener de 3 a 32 caracteres: letras, números, punto, guion o guion bajo.",
        ))
    }
}

pub fn password(p: &str) -> ApiResult<()> {
    if p.chars().count() >= 10 {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "La contraseña debe tener al menos 10 caracteres.",
        ))
    }
}

pub fn language(l: &str) -> ApiResult<()> {
    if LANGUAGES.contains(&l) {
        Ok(())
    } else {
        Err(ApiError::bad_request("Idioma no disponible. Usa «es» o «en»."))
    }
}

pub fn non_empty(field: &str, v: &str) -> ApiResult<()> {
    if v.trim().is_empty() {
        Err(ApiError::bad_request(format!("Falta el campo «{field}».")))
    } else {
        Ok(())
    }
}
