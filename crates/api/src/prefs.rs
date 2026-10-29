use buzon_store::UserDb;
use serde_json::{Map, Value};

use crate::error::{ApiError, ApiResult};
use crate::models::{Preferences, PreferencesUpdate};

pub async fn load(db: &UserDb) -> ApiResult<Preferences> {
    let mut map = match serde_json::to_value(Preferences::default()) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    };
    for (k, v) in db.prefs().await? {
        if let (true, Ok(v)) = (map.contains_key(&k), serde_json::from_str::<Value>(&v)) {
            map.insert(k, v);
        }
    }
    Ok(serde_json::from_value(Value::Object(map)).unwrap_or_default())
}

pub fn valid_font_size(s: f64) -> bool {
    (87.5..=150.0).contains(&s) && ((s * 2.0) % 25.0).abs() < f64::EPSILON
}

/// Aplica los campos presentes del cambio y los guarda.
pub async fn save(db: &UserDb, update: &PreferencesUpdate) -> ApiResult<()> {
    if let Some(s) = update.font_size {
        if !valid_font_size(s) {
            return Err(ApiError::bad_request(
                "El tamaño del texto va de 87.5 a 150, en pasos de 12.5.",
            ));
        }
    }
    if update.undo_send.is_some_and(|s| ![0, 5, 10, 20, 30].contains(&s)) {
        return Err(ApiError::bad_request(
            "El margen para deshacer es de 0, 5, 10, 20 o 30 segundos.",
        ));
    }
    let pairs: [(&str, Option<Value>); 7] = [
        ("theme", update.theme.map(|v| serde_json::json!(v))),
        ("mode", update.mode.map(|v| serde_json::json!(v))),
        ("view", update.view.map(|v| serde_json::json!(v))),
        ("font", update.font.map(|v| serde_json::json!(v))),
        ("font_size", update.font_size.map(|v| serde_json::json!(v))),
        ("density", update.density.map(|v| serde_json::json!(v))),
        ("undo_send", update.undo_send.map(|v| serde_json::json!(v))),
    ];
    for (k, v) in pairs {
        if let Some(v) = v {
            db.set_pref(k, &v.to_string()).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::valid_font_size;

    #[test]
    fn font_size_steps() {
        for ok in [87.5, 100.0, 112.5, 125.0, 137.5, 150.0] {
            assert!(valid_font_size(ok), "{ok}");
        }
        for bad in [80.0, 101.0, 162.5] {
            assert!(!valid_font_size(bad), "{bad}");
        }
    }
}
