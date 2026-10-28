use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use buzon_api::AppState;
use buzon_store::crypto::MasterKey;
use buzon_store::AppDb;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

struct T {
    _dir: tempfile::TempDir,
    app: Router,
}

async fn app() -> T {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(&dir.path().join("app.db")).await.unwrap();
    let state = AppState::new(
        db,
        dir.path().to_path_buf(),
        MasterKey::generate(),
        true,
        Some("https://correo.test".into()),
    );
    T {
        app: buzon_api::app(Arc::new(state), None),
        _dir: dir,
    }
}

struct Res {
    status: StatusCode,
    cookie: Option<String>,
    body: Value,
}

impl T {
    async fn call(&self, method: &str, uri: &str, cookie: Option<&str>, body: Option<Value>) -> Res {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(c) = cookie {
            b = b.header(header::COOKIE, c);
        }
        let req = match body {
            Some(v) => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(v.to_string())),
            None if method == "GET" => b.body(Body::empty()),
            None => b
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}")),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let cookie = res
            .headers()
            .get(header::SET_COOKIE)
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string());
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Res { status, cookie, body }
    }

    async fn setup(&self) -> String {
        let r = self
            .call(
                "POST",
                "/api/v1/setup",
                None,
                Some(json!({
                    "admin": {"username": "javier", "password": "una-contraseña-larga"},
                    "server": {"language": "es", "timezone": "Europe/Madrid"},
                    "preferences": {"theme": "dark", "font_size": 112.5},
                    "account": {
                        "name": "Personal", "email": "yo@ejemplo.es",
                        "imap_host": "imap.ejemplo.es", "imap_port": 993,
                        "smtp_host": "smtp.ejemplo.es", "smtp_port": 465,
                        "login": "yo@ejemplo.es", "password": "clave-del-buzon"
                    }
                })),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.cookie.expect("cookie de sesión")
    }

    async fn invite_user(&self, admin: &str, username: &str) -> String {
        let inv = self
            .call("POST", "/api/v1/invitations", Some(admin), Some(json!({})))
            .await;
        let token = inv.body["url"]
            .as_str()
            .unwrap()
            .rsplit('/')
            .next()
            .unwrap()
            .to_string();
        let r = self
            .call(
                "POST",
                &format!("/api/v1/invite/{token}"),
                None,
                Some(json!({"username": username, "password": "otra-contraseña-larga"})),
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.cookie.unwrap()
    }
}

#[tokio::test]
async fn setup_runs_once_and_logs_in_the_admin() {
    let t = app().await;
    assert_eq!(
        t.call("GET", "/api/v1/setup", None, None).await.body["needs_setup"],
        true
    );
    let cookie = t.setup().await;
    assert_eq!(
        t.call("GET", "/api/v1/setup", None, None).await.body["needs_setup"],
        false
    );

    let me = t.call("GET", "/api/v1/me", Some(&cookie), None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "javier");
    assert_eq!(me.body["is_admin"], true);
    assert_eq!(me.body["preferences"]["theme"], "dark");
    assert_eq!(
        me.body["preferences"]["mode"], "focus",
        "Enfoque es el modo por defecto"
    );
    assert_eq!(me.body["preferences"]["font"], "atkinson");

    let accounts = t.call("GET", "/api/v1/accounts", Some(&cookie), None).await;
    assert_eq!(accounts.body.as_array().unwrap().len(), 1);
    assert!(
        !accounts.body.to_string().contains("clave-del-buzon"),
        "la contraseña del buzón no se devuelve nunca"
    );

    let again = t
        .call(
            "POST",
            "/api/v1/setup",
            None,
            Some(json!({
                "admin": {"username": "otro", "password": "una-contraseña-larga"},
                "server": {"language": "es"}
            })),
        )
        .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn ids_are_base58_uuidv7() {
    let t = app().await;
    let cookie = t.setup().await;
    let me = t.call("GET", "/api/v1/me", Some(&cookie), None).await;
    let id = me.body["id"].as_str().unwrap();
    assert!(id.parse::<buzon_store::Id>().is_ok());
    assert!(!id.contains('-'));
}

#[tokio::test]
async fn mutating_requests_require_json() {
    let t = app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("username=a&password=b"))
        .unwrap();
    let res = t.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn login_logout_and_lockout() {
    let t = app().await;
    t.setup().await;
    let ok = t
        .call(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({"username": "JAVIER", "password": "una-contraseña-larga"})),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    let cookie = ok.cookie.unwrap();
    assert_eq!(
        t.call("POST", "/api/v1/auth/logout", Some(&cookie), None).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        t.call("GET", "/api/v1/me", Some(&cookie), None).await.status,
        StatusCode::UNAUTHORIZED
    );

    for _ in 0..5 {
        let r = t
            .call(
                "POST",
                "/api/v1/auth/login",
                None,
                Some(json!({"username": "javier", "password": "mala"})),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
        assert_eq!(r.body["error"], "invalid_credentials");
    }
    let locked = t
        .call(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({"username": "javier", "password": "una-contraseña-larga"})),
        )
        .await;
    assert_eq!(locked.status, StatusCode::LOCKED);
}

#[tokio::test]
async fn invitations_are_single_use_and_admin_only() {
    let t = app().await;
    let admin = t.setup().await;

    let inv = t
        .call(
            "POST",
            "/api/v1/invitations",
            Some(&admin),
            Some(json!({"note": "Para Ana"})),
        )
        .await;
    assert_eq!(inv.status, StatusCode::OK);
    let url = inv.body["url"].as_str().unwrap();
    assert!(url.starts_with("https://correo.test/invite/"));
    let token = url.rsplit('/').next().unwrap();
    let ttl = inv.body["expires_at"].as_i64().unwrap() - buzon_store::now();
    assert!((72 * 3600 - 5..=72 * 3600).contains(&ttl), "caduca a las 72 h");

    assert_eq!(
        t.call("GET", &format!("/api/v1/invite/{token}"), None, None).await.body["valid"],
        true
    );
    let body = json!({"username": "ana", "password": "otra-contraseña-larga"});
    let accepted = t
        .call("POST", &format!("/api/v1/invite/{token}"), None, Some(body.clone()))
        .await;
    assert_eq!(accepted.status, StatusCode::OK);
    assert_eq!(accepted.body["is_admin"], false);
    let ana = accepted.cookie.unwrap();

    let reused = t
        .call(
            "POST",
            &format!("/api/v1/invite/{token}"),
            None,
            Some(json!({"username": "eva", "password": "otra-contraseña-larga"})),
        )
        .await;
    assert_eq!(reused.status, StatusCode::GONE);
    assert_eq!(
        t.call("GET", &format!("/api/v1/invite/{token}"), None, None).await.body["valid"],
        false
    );

    assert_eq!(
        t.call("POST", "/api/v1/invitations", Some(&ana), Some(json!({})))
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    let list = t.call("GET", "/api/v1/invitations", Some(&admin), None).await;
    assert_eq!(list.body[0]["status"], "used");
    assert!(
        !list.body.to_string().contains(token),
        "el token no se guarda ni se lista"
    );
}

#[tokio::test]
async fn invited_users_get_their_own_database() {
    let t = app().await;
    let admin = t.setup().await;
    let ana = t.invite_user(&admin, "ana").await;
    assert_eq!(
        t.call("GET", "/api/v1/accounts", Some(&ana), None)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        t.call("GET", "/api/v1/accounts", Some(&admin), None)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn totp_is_required_once_enabled() {
    let t = app().await;
    let cookie = t.setup().await;
    let setup = t.call("POST", "/api/v1/me/totp/setup", Some(&cookie), None).await;
    assert_eq!(setup.status, StatusCode::OK);
    let secret = totp_rs::Secret::Encoded(setup.body["secret"].as_str().unwrap().into())
        .to_bytes()
        .unwrap();
    let totp = totp_rs::TOTP::new(
        totp_rs::Algorithm::SHA1,
        6,
        1,
        30,
        secret,
        Some("Buzón".into()),
        "javier".into(),
    )
    .unwrap();

    let bad = t
        .call(
            "POST",
            "/api/v1/me/totp/enable",
            Some(&cookie),
            Some(json!({"code": "000000"})),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let ok = t
        .call(
            "POST",
            "/api/v1/me/totp/enable",
            Some(&cookie),
            Some(json!({"code": totp.generate_current().unwrap()})),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.body["totp_enabled"], true);

    let creds = json!({"username": "javier", "password": "una-contraseña-larga"});
    let need = t.call("POST", "/api/v1/auth/login", None, Some(creds)).await;
    assert_eq!(need.body["error"], "totp_required");
    let with = t
        .call(
            "POST",
            "/api/v1/auth/login",
            None,
            Some(json!({
                "username": "javier", "password": "una-contraseña-larga", "totp": totp.generate_current().unwrap()
            })),
        )
        .await;
    assert_eq!(with.status, StatusCode::OK);
}

#[tokio::test]
async fn preferences_are_validated() {
    let t = app().await;
    let cookie = t.setup().await;
    let ok = t
        .call(
            "PUT",
            "/api/v1/me/preferences",
            Some(&cookie),
            Some(json!({"mode": "classic", "view": "per_account", "language": "en"})),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.body["preferences"]["mode"], "classic");
    assert_eq!(ok.body["language"], "en");
    assert_eq!(ok.body["preferences"]["theme"], "dark", "lo no enviado no cambia");

    let bad = t
        .call(
            "PUT",
            "/api/v1/me/preferences",
            Some(&cookie),
            Some(json!({"font_size": 101})),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let bad_lang = t
        .call(
            "PUT",
            "/api/v1/me/preferences",
            Some(&cookie),
            Some(json!({"language": "fr"})),
        )
        .await;
    assert_eq!(bad_lang.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn openapi_and_viewer_are_public() {
    let t = app().await;
    let spec = t.call("GET", "/api/openapi.json", None, None).await;
    assert_eq!(spec.status, StatusCode::OK);
    assert!(spec.body["paths"]["/api/v1/auth/login"].is_object());
    assert!(spec.body["paths"]["/api/v1/invite/{token}"].is_object());

    let req = Request::builder().uri("/api/docs").body(Body::empty()).unwrap();
    let res = t.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
}
