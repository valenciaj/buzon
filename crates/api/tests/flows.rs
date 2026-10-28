use std::sync::Arc;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use buzon_api::AppState;
use buzon_store::crypto::MasterKey;
use buzon_store::mail::Flags;
use buzon_store::user::user_raw_dir;
use buzon_store::{AppDb, Id, UserDb};
use buzon_sync::parse::Envelope;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

struct T {
    dir: tempfile::TempDir,
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
        dir,
    }
}

struct Res {
    status: StatusCode,
    cookie: Option<String>,
    body: Value,
    headers: axum::http::HeaderMap,
    bytes: Vec<u8>,
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
        let headers = res.headers().clone();
        let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Res {
            status,
            cookie,
            body,
            headers,
            bytes,
        }
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

const RAW: &str = "From: Ana <ana@ejemplo.es>\r\nTo: yo@ejemplo.es\r\nSubject: Fotos\r\nMessage-ID: <f1@ejemplo.es>\r\n\
Date: Tue, 27 Oct 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"B\"\r\n\r\n\
--B\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nTe paso las fotos.\r\n\
--B\r\nContent-Type: text/html; name=\"pagina.html\"\r\nContent-Disposition: attachment; filename=\"página.html\"\r\n\r\n\
<script>alert(1)</script>\r\n--B--\r\n";

/// Mete correo en la base de datos del usuario como lo haría la sincronización.
async fn seed_mail(t: &T, cookie: &str) -> (Id, Id) {
    let me = t.call("GET", "/api/v1/me", Some(cookie), None).await;
    let user: Id = me.body["id"].as_str().unwrap().parse().unwrap();
    let accounts = t.call("GET", "/api/v1/accounts", Some(cookie), None).await;
    let account: Id = accounts.body[0]["id"].as_str().unwrap().parse().unwrap();
    assert_eq!(accounts.body[0]["state"], "off");
    assert_eq!(accounts.body[0]["imap_security"], "tls");

    let udb = UserDb::open(t.dir.path(), user).await.unwrap();
    let inbox = udb
        .upsert_mailbox(account, "INBOX", Some("/"), Some("inbox"))
        .await
        .unwrap();
    let msg_id = Id::new();
    let raw_dir = user_raw_dir(t.dir.path(), user);
    std::fs::create_dir_all(&raw_dir).unwrap();
    std::fs::write(raw_dir.join(format!("{msg_id}.eml")), RAW).unwrap();
    let m = buzon_sync::parse::parse(
        RAW.as_bytes(),
        Envelope {
            id: msg_id,
            account_id: account,
            mailbox_id: inbox.id,
            uid: 1,
            size: RAW.len() as u32,
            flags: Flags::default(),
            fallback_date: 0,
        },
    );
    let thread = udb.insert_message(&m).await.unwrap();
    (thread, msg_id)
}

#[tokio::test]
async fn reads_threads_messages_and_attachments() {
    let t = app().await;
    let cookie = t.setup().await;
    let (thread, msg) = seed_mail(&t, &cookie).await;

    let mbs = t.call("GET", "/api/v1/mailboxes", Some(&cookie), None).await;
    assert_eq!(mbs.body[0]["role"], "inbox");
    assert_eq!(mbs.body[0]["unread"], 1);

    let page = t.call("GET", "/api/v1/threads?role=inbox", Some(&cookie), None).await;
    assert_eq!(page.status, StatusCode::OK, "{}", page.body);
    assert_eq!(page.body["threads"][0]["id"], thread.to_base58());
    assert_eq!(page.body["threads"][0]["subject"], "Fotos");
    assert_eq!(page.body["threads"][0]["has_attachments"], true);
    assert_eq!(page.body["next_cursor"], Value::Null);
    let bad = t.call("GET", "/api/v1/threads?role=nada", Some(&cookie), None).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    let detail = t
        .call("GET", &format!("/api/v1/threads/{thread}"), Some(&cookie), None)
        .await;
    let m = &detail.body["messages"][0];
    assert_eq!(m["from"]["name"], "Ana");
    assert_eq!(m["seen"], false);
    let url = m["attachments"][0]["url"].as_str().unwrap().to_string();
    assert_eq!(url, format!("/api/v1/messages/{msg}/attachments/0"));

    // Un adjunto HTML se descarga como binario, nunca se muestra.
    let file = t.call("GET", &url, Some(&cookie), None).await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.headers["content-type"], "application/octet-stream");
    assert_eq!(file.headers["x-content-type-options"], "nosniff");
    assert!(file.headers["content-disposition"]
        .to_str()
        .unwrap()
        .contains("p%C3%A1gina.html"));
    assert!(String::from_utf8_lossy(&file.bytes).contains("<script>"));
    let none = t
        .call(
            "GET",
            &format!("/api/v1/messages/{msg}/attachments/9"),
            Some(&cookie),
            None,
        )
        .await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);

    let r = t
        .call(
            "PATCH",
            &format!("/api/v1/threads/{thread}"),
            Some(&cookie),
            Some(json!({"seen": true, "flagged": true})),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let detail = t
        .call("GET", &format!("/api/v1/threads/{thread}"), Some(&cookie), None)
        .await;
    assert_eq!(detail.body["messages"][0]["seen"], true);
    assert_eq!(detail.body["messages"][0]["flagged"], true);

    // Otra persona no ve nada de esto.
    let ana = t.invite_user(&cookie, "ana").await;
    let other = t
        .call("GET", &format!("/api/v1/threads/{thread}"), Some(&ana), None)
        .await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);
    assert_eq!(
        t.call("GET", &url, Some(&ana), None).await.status,
        StatusCode::NOT_FOUND
    );
    let anon = t.call("GET", "/api/v1/threads", None, None).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);

    // Al borrar la cuenta se va su correo, también el original en disco.
    let account = t.call("GET", "/api/v1/accounts", Some(&cookie), None).await.body[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let del = t
        .call("DELETE", &format!("/api/v1/accounts/{account}"), Some(&cookie), None)
        .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    let page = t.call("GET", "/api/v1/threads", Some(&cookie), None).await;
    assert_eq!(page.body["threads"].as_array().unwrap().len(), 0);
    assert_eq!(
        t.call("GET", &url, Some(&cookie), None).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn new_accounts_are_checked_against_the_server() {
    let t = app().await;
    let cookie = t.setup().await;
    let account = json!({
        "name": "Trabajo", "email": "yo@trabajo.es",
        "imap_host": "127.0.0.1", "imap_port": 1,
        "smtp_host": "127.0.0.1", "smtp_port": 1,
        "login": "yo", "password": "x"
    });
    let r = t
        .call("POST", "/api/v1/accounts", Some(&cookie), Some(account.clone()))
        .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{}", r.body);
    assert_eq!(r.body["error"], "mail_login_failed");

    let mut insecure = account.clone();
    insecure["imap_security"] = json!("none");
    insecure["verify"] = json!(false);
    let r = t.call("POST", "/api/v1/accounts", Some(&cookie), Some(insecure)).await;
    assert_eq!(
        r.status,
        StatusCode::BAD_REQUEST,
        "sin cifrar no se permite por defecto"
    );

    let mut unchecked = account;
    unchecked["verify"] = json!(false);
    let r = t.call("POST", "/api/v1/accounts", Some(&cookie), Some(unchecked)).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let list = t.call("GET", "/api/v1/accounts", Some(&cookie), None).await;
    assert_eq!(list.body.as_array().unwrap().len(), 2);
}
