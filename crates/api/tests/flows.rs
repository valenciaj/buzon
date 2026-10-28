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
    app_with(false).await
}

async fn app_with(allow_insecure_mail: bool) -> T {
    let dir = tempfile::tempdir().unwrap();
    let db = AppDb::open(&dir.path().join("app.db")).await.unwrap();
    let mut state = AppState::new(
        db,
        dir.path().to_path_buf(),
        MasterKey::generate(),
        true,
        Some("https://correo.test".into()),
    );
    state.allow_insecure_mail = allow_insecure_mail;
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

    async fn upload(&self, cookie: &str, name: Option<&str>, content_type: &str, data: &[u8]) -> Res {
        let mut b = Request::builder()
            .method("POST")
            .uri("/api/v1/uploads")
            .header(header::COOKIE, cookie)
            .header(header::CONTENT_TYPE, content_type);
        if let Some(n) = name {
            b = b.header("x-buzon-filename", n);
        }
        let res = self
            .app
            .clone()
            .oneshot(b.body(Body::from(data.to_vec())).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        Res {
            status,
            cookie: None,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
            headers: Default::default(),
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

/// Servidor SMTP mínimo: acepta un envío y devuelve el sobre y el mensaje recibidos.
async fn fake_smtp() -> (u16, tokio::sync::oneshot::Receiver<(Vec<String>, String)>) {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (sock, _) = listener.accept().await.unwrap();
        let (r, mut w) = sock.into_split();
        let mut r = BufReader::new(r);
        w.write_all(b"220 falso ESMTP\r\n").await.unwrap();
        let (mut rcpt, mut data) = (Vec::new(), String::new());
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line).await.unwrap() == 0 {
                break;
            }
            let cmd = line.trim_end().to_ascii_uppercase();
            let reply: &[u8] = if cmd.starts_with("EHLO") {
                b"250-falso\r\n250 AUTH PLAIN LOGIN\r\n"
            } else if cmd.starts_with("AUTH") {
                b"235 vale\r\n"
            } else if cmd.starts_with("RCPT") {
                rcpt.push(line.trim_end()[8..].to_string());
                b"250 vale\r\n"
            } else if cmd == "DATA" {
                w.write_all(b"354 adelante\r\n").await.unwrap();
                loop {
                    line.clear();
                    r.read_line(&mut line).await.unwrap();
                    if line == ".\r\n" {
                        break;
                    }
                    data.push_str(&line);
                }
                b"250 encolado\r\n"
            } else if cmd == "QUIT" {
                w.write_all(b"221 adios\r\n").await.unwrap();
                break;
            } else {
                b"250 vale\r\n"
            };
            w.write_all(reply).await.unwrap();
        }
        let _ = tx.send((rcpt, data));
    });
    (port, rx)
}

#[tokio::test]
async fn identities_drafts_and_uploads() {
    let t = app().await;
    let cookie = t.setup().await;
    let ids = t.call("GET", "/api/v1/identities", Some(&cookie), None).await;
    assert_eq!(ids.status, StatusCode::OK, "{}", ids.body);
    let list = ids.body.as_array().unwrap();
    assert_eq!(list.len(), 1, "la cuenta trae su identidad");
    assert_eq!(list[0]["email"], "yo@ejemplo.es");
    assert_eq!(list[0]["is_default"], true);
    let first = list[0]["id"].as_str().unwrap().to_string();
    let account = list[0]["account_id"].as_str().unwrap().to_string();

    let alias = t
        .call(
            "POST",
            "/api/v1/identities",
            Some(&cookie),
            Some(
                json!({"account_id": account, "name": "Javier V.", "email": "hola@ejemplo.es",
                "signature": "Un saludo,\nJavier\n\n", "is_default": true}),
            ),
        )
        .await;
    assert_eq!(alias.status, StatusCode::OK, "{}", alias.body);
    assert_eq!(alias.body["signature"], "Un saludo,\nJavier");
    let alias_id = alias.body["id"].as_str().unwrap().to_string();
    let ids = t.call("GET", "/api/v1/identities", Some(&cookie), None).await;
    let defaults: Vec<_> = ids
        .body
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["is_default"] == true)
        .collect();
    assert_eq!(defaults.len(), 1);
    assert_eq!(defaults[0]["id"], alias_id.as_str());

    let bad = t
        .call(
            "POST",
            "/api/v1/identities",
            Some(&cookie),
            Some(json!({"account_id": account, "name": "", "email": "sin-arroba"})),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);

    let del = t
        .call("DELETE", &format!("/api/v1/identities/{alias_id}"), Some(&cookie), None)
        .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    let last = t
        .call("DELETE", &format!("/api/v1/identities/{first}"), Some(&cookie), None)
        .await;
    assert_eq!(last.status, StatusCode::CONFLICT);
    assert_eq!(last.body["error"], "last_identity");
    let ids = t.call("GET", "/api/v1/identities", Some(&cookie), None).await;
    assert_eq!(ids.body[0]["is_default"], true, "vuelve a ser la predeterminada");

    // Subidas: sin JSON solo si llevan la cabecera propia.
    let up = t
        .upload(&cookie, Some("men%C3%BA.pdf"), "application/pdf", b"%PDF-1.4")
        .await;
    assert_eq!(up.status, StatusCode::OK, "{}", up.body);
    assert_eq!(up.body["name"], "menú.pdf");
    assert_eq!(up.body["size"], 8);
    let upload_id = up.body["id"].as_str().unwrap().to_string();
    let csrf = t.upload(&cookie, None, "text/plain", b"hola").await;
    assert_eq!(csrf.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let draft = json!({"to": [{"name": "Ana", "email": "ana@ejemplo.es"}], "subject": "Menú",
        "html": "<p>Hola</p>", "text": "Hola", "attachments": [upload_id]});
    let d = t
        .call("POST", "/api/v1/drafts", Some(&cookie), Some(draft.clone()))
        .await;
    assert_eq!(d.status, StatusCode::OK, "{}", d.body);
    assert_eq!(d.body["attachments"][0]["name"], "menú.pdf");
    let draft_id = d.body["id"].as_str().unwrap().to_string();
    let mut changed = draft;
    changed["subject"] = json!("Menú del viernes");
    let u = t
        .call(
            "PUT",
            &format!("/api/v1/drafts/{draft_id}"),
            Some(&cookie),
            Some(changed),
        )
        .await;
    assert_eq!(u.body["subject"], "Menú del viernes");
    let all = t.call("GET", "/api/v1/drafts", Some(&cookie), None).await;
    assert_eq!(all.body.as_array().unwrap().len(), 1);

    // Otra persona no ve ni toca los borradores.
    let ana = t.invite_user(&cookie, "ana").await;
    let other = t
        .call(
            "PUT",
            &format!("/api/v1/drafts/{draft_id}"),
            Some(&ana),
            Some(json!({})),
        )
        .await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);

    let del = t
        .call("DELETE", &format!("/api/v1/drafts/{draft_id}"), Some(&cookie), None)
        .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
    assert_eq!(
        t.call("GET", "/api/v1/drafts", Some(&cookie), None).await.body,
        json!([])
    );
    let user = t.call("GET", "/api/v1/me", Some(&cookie), None).await.body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let file = t.dir.path().join("users").join(user).join("uploads").join(&upload_id);
    assert!(!file.exists(), "el adjunto se borra con el borrador");
}

#[tokio::test]
async fn sends_replies_through_smtp() {
    let t = app_with(true).await;
    let cookie = t.setup().await;
    let (_, original) = seed_mail(&t, &cookie).await;
    let (port, received) = fake_smtp().await;
    let acc = t
        .call(
            "POST",
            "/api/v1/accounts",
            Some(&cookie),
            Some(json!({
                "name": "Pruebas", "email": "yo@pruebas.es",
                "imap_host": "127.0.0.1", "imap_port": 1, "imap_security": "none",
                "smtp_host": "127.0.0.1", "smtp_port": port, "smtp_security": "none",
                "login": "yo", "password": "x", "verify": false
            })),
        )
        .await;
    assert_eq!(acc.status, StatusCode::OK, "{}", acc.body);
    let ids = t.call("GET", "/api/v1/identities", Some(&cookie), None).await;
    let identity = ids
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["email"] == "yo@pruebas.es")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let up = t.upload(&cookie, Some("notas.txt"), "text/plain", b"apuntes").await;
    let upload_id = up.body["id"].as_str().unwrap().to_string();

    let none = t
        .call(
            "POST",
            "/api/v1/send",
            Some(&cookie),
            Some(json!({"identity_id": identity, "subject": "x"})),
        )
        .await;
    assert_eq!(none.status, StatusCode::BAD_REQUEST);

    let r = t
        .call(
            "POST",
            "/api/v1/send",
            Some(&cookie),
            Some(json!({
                "identity_id": identity,
                "to": [{"name": "Ana", "email": "ana@ejemplo.es"}],
                "bcc": [{"email": "oculto@ejemplo.es"}],
                "subject": "Re: Fotos",
                "html": "<p>¡Gracias!</p><script>alert(1)</script>",
                "text": "¡Gracias!",
                "reply_to": original.to_base58(),
                "attachments": [upload_id],
            })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body["message_id"].as_str().unwrap().ends_with("@pruebas.es"));

    let book = t.call("GET", "/api/v1/contacts", Some(&cookie), None).await;
    assert!(book
        .body
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["email"] == "ana@ejemplo.es" && c["source"] == "sent"));

    let (rcpt, data) = received.await.unwrap();
    assert_eq!(rcpt.len(), 2, "{rcpt:?}");
    assert!(data.contains("In-Reply-To: <f1@ejemplo.es>"), "{data}");
    assert!(data.contains("notas.txt"));
    assert!(!data.contains("oculto@"));
    assert!(!data.contains("<script>"), "el HTML sale saneado");

    let thread = t.call("GET", "/api/v1/threads?role=inbox", Some(&cookie), None).await;
    let detail = t
        .call(
            "GET",
            &format!("/api/v1/threads/{}", thread.body["threads"][0]["id"].as_str().unwrap()),
            Some(&cookie),
            None,
        )
        .await;
    assert_eq!(detail.body["messages"][0]["answered"], true, "{}", detail.body);

    // Si el servidor no responde, se avisa y no se pierde nada.
    let mut dead = t
        .call(
            "POST",
            "/api/v1/drafts",
            Some(&cookie),
            Some(json!({"identity_id": identity, "to": [{"email": "ana@ejemplo.es"}]})),
        )
        .await
        .body;
    let draft_id = dead["id"].take();
    let r = t
        .call(
            "POST",
            "/api/v1/send",
            Some(&cookie),
            Some(json!({"identity_id": identity, "to": [{"email": "ana@ejemplo.es"}], "draft_id": draft_id})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.body);
    assert_eq!(r.body["error"], "smtp_failed");
    assert_eq!(
        t.call("GET", "/api/v1/drafts", Some(&cookie), None)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn organizes_searches_and_labels_threads() {
    let t = app().await;
    let cookie = t.setup().await;
    let (thread, _) = seed_mail(&t, &cookie).await;
    let me = t.call("GET", "/api/v1/me", Some(&cookie), None).await;
    let user: Id = me.body["id"].as_str().unwrap().parse().unwrap();
    let account: Id = t.call("GET", "/api/v1/accounts", Some(&cookie), None).await.body[0]["id"]
        .as_str()
        .unwrap()
        .parse()
        .unwrap();
    let udb = UserDb::open(t.dir.path(), user).await.unwrap();
    udb.upsert_mailbox(account, "Archivo", None, Some("archive"))
        .await
        .unwrap();
    udb.upsert_mailbox(account, "Papelera", None, Some("trash"))
        .await
        .unwrap();
    let list = |q: &str| format!("/api/v1/threads?{q}");

    // Buscar.
    let found = t.call("GET", &list("q=fotos"), Some(&cookie), None).await;
    assert_eq!(found.status, StatusCode::OK, "{}", found.body);
    assert_eq!(found.body["threads"][0]["id"], thread.to_base58());
    for (q, n) in [
        ("q=de%3Aana%20te%20paso", 1),
        ("q=de%3Aluis", 0),
        ("q=tiene%3Aadjunto%20es%3Anoleido", 1),
    ] {
        let r = t.call("GET", &list(q), Some(&cookie), None).await;
        assert_eq!(r.body["threads"].as_array().unwrap().len(), n, "{q}");
    }

    // Etiquetas.
    let bad = t
        .call(
            "POST",
            "/api/v1/labels",
            Some(&cookie),
            Some(json!({"name": "Casa", "color": "verde"})),
        )
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let label = t
        .call(
            "POST",
            "/api/v1/labels",
            Some(&cookie),
            Some(json!({"name": "Casa", "color": "#2F9A72"})),
        )
        .await;
    assert_eq!(label.status, StatusCode::OK, "{}", label.body);
    assert_eq!(label.body["color"], "#2f9a72");
    let label_id = label.body["id"].as_str().unwrap().to_string();
    let dup = t
        .call(
            "POST",
            "/api/v1/labels",
            Some(&cookie),
            Some(json!({"name": "casa", "color": "#000000"})),
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    let r = t
        .call(
            "POST",
            "/api/v1/threads/labels",
            Some(&cookie),
            Some(json!({"threads": [thread.to_base58()], "add": [label_id]})),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let by_label = t
        .call("GET", &list(&format!("label={label_id}")), Some(&cookie), None)
        .await;
    assert_eq!(by_label.body["threads"][0]["labels"][0], label_id.as_str());
    let by_name = t.call("GET", &list("q=etiqueta%3Acasa"), Some(&cookie), None).await;
    assert_eq!(by_name.body["threads"].as_array().unwrap().len(), 1);
    let none = t.call("GET", &list("q=etiqueta%3Anada"), Some(&cookie), None).await;
    assert_eq!(none.body["threads"].as_array().unwrap().len(), 0);
    let detail = t
        .call("GET", &format!("/api/v1/threads/{thread}"), Some(&cookie), None)
        .await;
    assert_eq!(detail.body["labels"][0], label_id.as_str());
    assert_eq!(
        t.call("GET", "/api/v1/labels", Some(&cookie), None).await.body[0]["threads"],
        1
    );

    // Leído y destacado, varios a la vez.
    let r = t
        .call(
            "POST",
            "/api/v1/threads/flags",
            Some(&cookie),
            Some(json!({"threads": [thread.to_base58()], "seen": true, "flagged": true})),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let page = t.call("GET", &list("role=inbox"), Some(&cookie), None).await;
    assert_eq!(page.body["threads"][0]["unread"], 0);
    assert_eq!(page.body["threads"][0]["flagged"], true);

    // Archivar, sin carpeta de spam, a la papelera y borrar para siempre.
    let mv = |to: &str| json!({"threads": [thread.to_base58()], "from_role": "inbox", "to_role": to});
    let r = t
        .call("POST", "/api/v1/threads/move", Some(&cookie), Some(mv("archive")))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["moved"], 1);
    assert_eq!(
        t.call("GET", &list("role=inbox"), Some(&cookie), None).await.body["threads"],
        json!([])
    );
    assert_eq!(
        t.call("GET", &list("role=archive"), Some(&cookie), None).await.body["threads"][0]["id"],
        thread.to_base58()
    );
    let r = t
        .call(
            "POST",
            "/api/v1/threads/move",
            Some(&cookie),
            Some(json!({"threads": [thread.to_base58()], "to_role": "junk"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.body["error"], "missing_folder");
    let r = t
        .call(
            "POST",
            "/api/v1/threads/move",
            Some(&cookie),
            Some(json!({"threads": [thread.to_base58()], "to_role": "sent"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "a Enviados no se mueve");

    // Lo recién movido espera a que el servidor lo confirme antes de volver a moverse,
    // así que para la prueba se da por confirmado.
    sqlx_confirm(&udb).await;
    let r = t
        .call(
            "POST",
            "/api/v1/threads/move",
            Some(&cookie),
            Some(json!({"threads": [thread.to_base58()], "to_role": "trash"})),
        )
        .await;
    assert_eq!(r.body["moved"], 1);
    let r = t
        .call(
            "POST",
            "/api/v1/threads/delete",
            Some(&cookie),
            Some(json!({"threads": [thread.to_base58()]})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["deleted"], 1);
    assert_eq!(
        t.call("GET", &format!("/api/v1/threads/{thread}"), Some(&cookie), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Otra persona no puede tocar nada de esto.
    let ana = t.invite_user(&cookie, "ana").await;
    let r = t
        .call(
            "PUT",
            &format!("/api/v1/labels/{label_id}"),
            Some(&ana),
            Some(json!({"name": "Mía", "color": "#000000"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

/// Da por confirmados en el servidor los mensajes movidos (como haría la sincronización).
async fn sqlx_confirm(udb: &UserDb) {
    for mb in udb.mailboxes(None).await.unwrap() {
        for t in udb
            .threads(&buzon_store::mail::ThreadQuery {
                mailbox: Some(mb.mailbox.id),
                limit: 100,
                ..Default::default()
            })
            .await
            .unwrap()
        {
            let th = udb.thread(t.id).await.unwrap().unwrap();
            for m in th.messages.iter().filter(|m| m.uid == 0) {
                udb.confirm_move_for_tests(m.id, 99).await.unwrap();
            }
        }
    }
}

#[tokio::test]
async fn rules_and_address_book() {
    let t = app().await;
    let cookie = t.setup().await;
    let label = t
        .call(
            "POST",
            "/api/v1/labels",
            Some(&cookie),
            Some(json!({"name": "Facturas", "color": "#c4683a"})),
        )
        .await;
    let label_id = label.body["id"].as_str().unwrap().to_string();

    let empty = t
        .call(
            "POST",
            "/api/v1/rules",
            Some(&cookie),
            Some(json!({"name": "Todo", "mark_read": true})),
        )
        .await;
    assert_eq!(empty.status, StatusCode::BAD_REQUEST, "sin condiciones no");
    let nothing = t
        .call(
            "POST",
            "/api/v1/rules",
            Some(&cookie),
            Some(json!({"name": "Nada", "from_has": "x"})),
        )
        .await;
    assert_eq!(nothing.status, StatusCode::BAD_REQUEST, "sin acciones no");
    let rule = t
        .call(
            "POST",
            "/api/v1/rules",
            Some(&cookie),
            Some(json!({"name": "Luz", "from_has": "@luz.es", "subject_has": "factura",
                "label_id": label_id, "mark_read": true, "move_to": "archive"})),
        )
        .await;
    assert_eq!(rule.status, StatusCode::OK, "{}", rule.body);
    assert_eq!(rule.body["enabled"], true);
    assert_eq!(rule.body["move_to"], "archive");
    let id = rule.body["id"].as_str().unwrap().to_string();
    let mut changed = rule.body.clone();
    changed["enabled"] = json!(false);
    let r = t
        .call("PUT", &format!("/api/v1/rules/{id}"), Some(&cookie), Some(changed))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["enabled"], false);
    assert_eq!(
        t.call("GET", "/api/v1/rules", Some(&cookie), None)
            .await
            .body
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let del = t
        .call("DELETE", &format!("/api/v1/rules/{id}"), Some(&cookie), None)
        .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);

    // Libreta.
    let c = t
        .call(
            "POST",
            "/api/v1/contacts",
            Some(&cookie),
            Some(json!({"name": "Ana", "email": "ana@ejemplo.es"})),
        )
        .await;
    assert_eq!(c.status, StatusCode::OK, "{}", c.body);
    assert_eq!(c.body["source"], "manual");
    let dup = t
        .call(
            "POST",
            "/api/v1/contacts",
            Some(&cookie),
            Some(json!({"email": "ANA@ejemplo.es"})),
        )
        .await;
    assert_eq!(dup.status, StatusCode::CONFLICT);
    let bad = t
        .call("POST", "/api/v1/contacts", Some(&cookie), Some(json!({"email": "ana"})))
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let imp = t
        .call(
            "POST",
            "/api/v1/contacts/import",
            Some(&cookie),
            Some(json!({"vcard": "BEGIN:VCARD\nFN:Luis\nEMAIL:luis@ejemplo.es\nEND:VCARD\nBEGIN:VCARD\nFN:Ana G\nEMAIL:ana@ejemplo.es\nEND:VCARD\n"})),
        )
        .await;
    assert_eq!(imp.body, json!({"added": 1, "existing": 1}));
    let list = t.call("GET", "/api/v1/contacts?q=ejemplo", Some(&cookie), None).await;
    assert_eq!(list.body.as_array().unwrap().len(), 2);
    let sug = t
        .call("GET", "/api/v1/contacts/suggest?q=lu", Some(&cookie), None)
        .await;
    assert_eq!(sug.body[0]["email"], "luis@ejemplo.es");
    let id = c.body["id"].as_str().unwrap();
    let r = t
        .call(
            "PUT",
            &format!("/api/v1/contacts/{id}"),
            Some(&cookie),
            Some(json!({"name": "Ana García", "email": "ana@ejemplo.es", "phone": "600"})),
        )
        .await;
    assert_eq!(r.body["name"], "Ana García");
    assert_eq!(
        t.call("DELETE", &format!("/api/v1/contacts/{id}"), Some(&cookie), None)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn focus_flow_screener_piles_and_notes() {
    let t = app().await;
    let cookie = t.setup().await;
    let (thread, msg) = seed_mail(&t, &cookie).await;
    let me = t.call("GET", "/api/v1/me", Some(&cookie), None).await;
    let user: Id = me.body["id"].as_str().unwrap().parse().unwrap();
    let udb = UserDb::open(t.dir.path(), user).await.unwrap();
    // El correo llegó después de la primera descarga: su remitente espera aprobación.
    let from = udb.thread(thread).await.unwrap().unwrap().messages[0]
        .from
        .clone()
        .unwrap();
    assert!(udb.screen_sender(&from, "principal", true).await.unwrap());
    let ids = |r: &Res| -> Vec<String> {
        r.body["threads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["id"].as_str().unwrap().to_string())
            .collect()
    };
    let view = |v: &str| format!("/api/v1/threads?view={v}");
    let tid = thread.to_base58();

    let r = t.call("GET", &view("screener"), Some(&cookie), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(ids(&r), std::slice::from_ref(&tid));
    assert!(ids(&t.call("GET", &view("principal"), Some(&cookie), None).await).is_empty());
    assert_eq!(
        t.call("GET", &view("nada"), Some(&cookie), None).await.status,
        StatusCode::BAD_REQUEST
    );
    let pending = t
        .call("GET", "/api/v1/senders?status=pending", Some(&cookie), None)
        .await;
    assert_eq!(pending.body[0]["email"], from.email);
    assert_eq!(pending.body[0]["last_thread"], tid);
    let counts = t.call("GET", "/api/v1/flow", Some(&cookie), None).await;
    assert_eq!(counts.body["screener"], 1, "{}", counts.body);

    // Aprobar, a Novedades.
    let bad = json!({"email": from.email, "status": "quizá"});
    assert_eq!(
        t.call("PUT", "/api/v1/senders", Some(&cookie), Some(bad)).await.status,
        StatusCode::BAD_REQUEST
    );
    let ok = t
        .call(
            "PUT",
            "/api/v1/senders",
            Some(&cookie),
            Some(json!({"email": from.email, "status": "approved", "bucket": "news"})),
        )
        .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.body);
    assert_eq!(ok.body["bucket"], "news");
    assert_eq!(
        ids(&t.call("GET", &view("news"), Some(&cookie), None).await),
        std::slice::from_ref(&tid)
    );
    let detail = t
        .call("GET", &format!("/api/v1/threads/{tid}"), Some(&cookie), None)
        .await;
    assert_eq!(detail.body["sender"]["status"], "approved");
    assert_eq!(detail.body["messages"][0]["trackers"], json!([]));

    // Pilas y pospuestos.
    let flow = |body: serde_json::Value| t.call("POST", "/api/v1/threads/flow", Some(&cookie), Some(body));
    assert_eq!(flow(json!({"threads": [tid]})).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        flow(json!({"threads": [tid], "reply_later": true})).await.status,
        StatusCode::NO_CONTENT
    );
    let rl = t.call("GET", &view("reply_later"), Some(&cookie), None).await;
    assert_eq!(ids(&rl), std::slice::from_ref(&tid));
    assert_eq!(rl.body["threads"][0]["reply_later"], true);
    assert!(ids(&t.call("GET", &view("news"), Some(&cookie), None).await).is_empty());
    assert_eq!(
        flow(json!({"threads": [tid], "snooze_until": 1})).await.status,
        StatusCode::BAD_REQUEST
    );
    let later = buzon_store::now() + 86_400;
    assert_eq!(
        flow(json!({"threads": [tid], "snooze_until": later})).await.status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        ids(&t.call("GET", &view("snoozed"), Some(&cookie), None).await),
        std::slice::from_ref(&tid)
    );
    assert!(ids(&t.call("GET", &view("reply_later"), Some(&cookie), None).await).is_empty());
    assert_eq!(
        flow(json!({"threads": [tid], "snooze_until": 0, "reply_later": false}))
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        ids(&t.call("GET", &view("news"), Some(&cookie), None).await),
        std::slice::from_ref(&tid)
    );

    // Nota privada.
    for (id, status) in [
        (tid.clone(), StatusCode::NO_CONTENT),
        (Id::new().to_base58(), StatusCode::NOT_FOUND),
    ] {
        let url = format!("/api/v1/threads/{id}/note");
        let r = t
            .call("PUT", &url, Some(&cookie), Some(json!({"note": "Llevar vino"})))
            .await;
        assert_eq!(r.status, status);
    }
    let detail = t
        .call("GET", &format!("/api/v1/threads/{tid}"), Some(&cookie), None)
        .await;
    assert_eq!(detail.body["note"], "Llevar vino");
    assert_eq!(
        t.call("GET", &view("news"), Some(&cookie), None).await.body["threads"][0]["has_note"],
        true
    );
    let _ = msg;
}
