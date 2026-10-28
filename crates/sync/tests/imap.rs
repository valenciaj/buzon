//! Prueba de la sincronización completa contra un servidor IMAP de mentira, en memoria.
//! Implementa solo lo que usa Buzón: LOGIN, LIST, SELECT, UID SEARCH/FETCH/STORE/MOVE e IDLE.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use buzon_store::crypto::MasterKey;
use buzon_store::mail::ThreadQuery;
use buzon_store::organize::{Place, Rule};
use buzon_store::user::{user_raw_dir, NewAccount};
use buzon_store::{Id, UserDb};
use buzon_sync::{Security, ServerConfig, State, SyncHub};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

struct Msg {
    uid: u32,
    flags: Vec<String>,
    raw: Vec<u8>,
}

struct Folder {
    name: &'static str,
    attrs: &'static str,
    validity: u32,
    next_uid: u32,
    msgs: Vec<Msg>,
}

#[derive(Default)]
struct Server {
    folders: Vec<Folder>,
}

impl Server {
    fn folder(&mut self, name: &str) -> &mut Folder {
        self.folders.iter_mut().find(|f| f.name == name).expect("carpeta")
    }

    fn append(&mut self, folder: &str, raw: &str, flags: &[&str]) {
        let f = self.folder(folder);
        f.next_uid += 1;
        let uid = f.next_uid;
        f.msgs.push(Msg {
            uid,
            flags: flags.iter().map(|s| s.to_string()).collect(),
            raw: raw.replace('\n', "\r\n").into_bytes(),
        });
    }
}

struct Fake {
    state: Arc<Mutex<Server>>,
    notify: Arc<Notify>,
    port: u16,
}

fn parse_set(set: &str, max: u32) -> Vec<(u32, u32)> {
    set.split(',')
        .map(|p| {
            let n = |s: &str| if s == "*" { max } else { s.parse().unwrap_or(0) };
            match p.split_once(':') {
                Some((a, b)) => {
                    let (a, b) = (n(a), n(b));
                    (a.min(b), a.max(b))
                }
                None => (n(p), n(p)),
            }
        })
        .collect()
}

fn in_set(uid: u32, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|(a, b)| uid >= *a && uid <= *b)
}

/// Divide una línea IMAP en palabras, respetando comillas y paréntesis.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut quoted, mut depth) = (false, 0);
    for c in line.chars() {
        match c {
            '"' if depth == 0 => quoted = !quoted,
            '(' if !quoted => {
                depth += 1;
                cur.push(c)
            }
            ')' if !quoted => {
                depth -= 1;
                cur.push(c)
            }
            ' ' if !quoted && depth == 0 => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

async fn client(stream: TcpStream, state: Arc<Mutex<Server>>, notify: Arc<Notify>) {
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();
    w.write_all(b"* OK servidor de pruebas\r\n").await.unwrap();
    let mut selected: Option<String> = None;
    while let Ok(Some(line)) = lines.next_line().await {
        let parts = words(&line);
        let (tag, cmd) = (
            parts[0].clone(),
            parts.get(1).map(|s| s.to_uppercase()).unwrap_or_default(),
        );
        let mut out = String::new();
        let mut body: Vec<u8> = Vec::new();
        let ok = format!("{tag} OK hecho\r\n");
        match cmd.as_str() {
            "CAPABILITY" => out = format!("* CAPABILITY IMAP4rev1 IDLE MOVE\r\n{ok}"),
            "LOGIN" => {
                out = if parts[3] == "clave" {
                    format!("* CAPABILITY IMAP4rev1 IDLE MOVE\r\n{ok}")
                } else {
                    format!("{tag} NO [AUTHENTICATIONFAILED] credenciales no válidas\r\n")
                }
            }
            "LOGOUT" => {
                w.write_all(format!("* BYE adiós\r\n{ok}").as_bytes()).await.unwrap();
                return;
            }
            "NOOP" => out = ok.clone(),
            "LIST" => {
                let s = state.lock().unwrap();
                for f in &s.folders {
                    out += &format!("* LIST ({}) \"/\" \"{}\"\r\n", f.attrs, f.name);
                }
                out += &ok;
            }
            "SELECT" | "EXAMINE" => {
                let mut s = state.lock().unwrap();
                let f = s.folder(&parts[2]);
                out = format!(
                    "* {} EXISTS\r\n* 0 RECENT\r\n* OK [UIDVALIDITY {}] v\r\n* OK [UIDNEXT {}] n\r\n\
                     * FLAGS (\\Seen \\Flagged \\Answered)\r\n{tag} OK [READ-WRITE] hecho\r\n",
                    f.msgs.len(),
                    f.validity,
                    f.next_uid + 1
                );
                selected = Some(parts[2].clone());
            }
            "UID" => {
                let sub = parts[2].to_uppercase();
                let mut s = state.lock().unwrap();
                let f = s.folder(selected.as_deref().unwrap());
                let max = f.msgs.iter().map(|m| m.uid).max().unwrap_or(0);
                match sub.as_str() {
                    "SEARCH" => {
                        let ranges = if parts[3].eq_ignore_ascii_case("ALL") {
                            vec![(0, u32::MAX)]
                        } else {
                            parse_set(&parts[4], max)
                        };
                        let uids: Vec<String> = f
                            .msgs
                            .iter()
                            .filter(|m| in_set(m.uid, &ranges))
                            .map(|m| m.uid.to_string())
                            .collect();
                        out = format!("* SEARCH {}\r\n{ok}", uids.join(" "));
                    }
                    "FETCH" => {
                        let ranges = parse_set(&parts[3], max);
                        let items = parts[4].to_uppercase();
                        let mut text = Vec::new();
                        for (seq, m) in f.msgs.iter().enumerate() {
                            if !in_set(m.uid, &ranges) {
                                continue;
                            }
                            let mut attrs = format!("UID {}", m.uid);
                            if items.contains("FLAGS") {
                                attrs += &format!(" FLAGS ({})", m.flags.join(" "));
                            }
                            if items.contains("RFC822.SIZE") {
                                attrs += &format!(" RFC822.SIZE {}", m.raw.len());
                            }
                            text.extend(format!("* {} FETCH ({attrs}", seq + 1).bytes());
                            if items.contains("BODY.PEEK[]") {
                                text.extend(format!(" BODY[] {{{}}}\r\n", m.raw.len()).bytes());
                                text.extend(&m.raw);
                            }
                            text.extend(b")\r\n");
                        }
                        text.extend(ok.bytes());
                        body = text;
                    }
                    "STORE" => {
                        let ranges = parse_set(&parts[3], max);
                        let add = parts[4].starts_with('+');
                        let flags: Vec<String> = parts[5]
                            .trim_matches(|c| c == '(' || c == ')')
                            .split(' ')
                            .map(String::from)
                            .collect();
                        for m in f.msgs.iter_mut().filter(|m| in_set(m.uid, &ranges)) {
                            for fl in &flags {
                                m.flags.retain(|x| x != fl);
                                if add {
                                    m.flags.push(fl.clone());
                                }
                            }
                        }
                        out = ok.clone();
                    }
                    "MOVE" => {
                        let ranges = parse_set(&parts[3], max);
                        let (moved, keep): (Vec<Msg>, Vec<Msg>) = std::mem::take(&mut f.msgs)
                            .into_iter()
                            .partition(|m| in_set(m.uid, &ranges));
                        f.msgs = keep;
                        let dest = s.folder(&parts[4]);
                        for m in moved {
                            dest.next_uid += 1;
                            dest.msgs.push(Msg {
                                uid: dest.next_uid,
                                ..m
                            });
                        }
                        out = ok.clone();
                    }
                    _ => out = format!("{tag} BAD no soportado\r\n"),
                }
            }
            "IDLE" => {
                w.write_all(b"+ esperando\r\n").await.unwrap();
                let before = {
                    let mut s = state.lock().unwrap();
                    s.folder(selected.as_deref().unwrap()).msgs.len()
                };
                loop {
                    tokio::select! {
                        l = lines.next_line() => {
                            match l {
                                Ok(Some(l)) if l.eq_ignore_ascii_case("DONE") => break,
                                Ok(Some(_)) => continue,
                                _ => return,
                            }
                        }
                        _ = notify.notified() => {
                            let n = state.lock().unwrap().folder(selected.as_deref().unwrap()).msgs.len();
                            if n != before {
                                w.write_all(format!("* {n} EXISTS\r\n").as_bytes()).await.unwrap();
                            }
                        }
                    }
                }
                out = ok.clone();
            }
            _ => out = format!("{tag} BAD no soportado\r\n"),
        }
        if body.is_empty() {
            body = out.into_bytes();
        }
        if w.write_all(&body).await.is_err() {
            return;
        }
    }
}

async fn fake_server() -> Fake {
    let mut s = Server::default();
    s.folders.push(Folder {
        name: "INBOX",
        attrs: "\\HasNoChildren",
        validity: 7,
        next_uid: 0,
        msgs: vec![],
    });
    s.folders.push(Folder {
        name: "Enviados",
        attrs: "\\HasNoChildren \\Sent",
        validity: 3,
        next_uid: 0,
        msgs: vec![],
    });
    s.folders.push(Folder {
        name: "Archivo",
        attrs: "\\HasNoChildren \\Archive",
        validity: 5,
        next_uid: 0,
        msgs: vec![],
    });
    s.folders.push(Folder {
        name: "[Gmail]/Todos",
        attrs: "\\HasNoChildren \\All",
        validity: 1,
        next_uid: 0,
        msgs: vec![],
    });
    s.append(
        "INBOX",
        "From: Ana <ana@ejemplo.es>\nTo: yo@ejemplo.es\nSubject: Cena del viernes\nMessage-ID: <a1@ejemplo.es>\nDate: Mon, 26 Oct 2026 09:00:00 +0000\n\n¿Te viene bien el viernes?\n",
        &["\\Seen"],
    );
    s.append(
        "INBOX",
        "From: Ana <ana@ejemplo.es>\nTo: yo@ejemplo.es\nSubject: Re: Cena del viernes\nMessage-ID: <a3@ejemplo.es>\nIn-Reply-To: <a2@ejemplo.es>\nReferences: <a1@ejemplo.es> <a2@ejemplo.es>\nDate: Tue, 27 Oct 2026 09:00:00 +0000\n\nPerfecto, reservo.\n",
        &[],
    );
    s.append(
        "INBOX",
        "From: Banco <avisos@banco.example>\nTo: yo@ejemplo.es\nSubject: Tu extracto\nMessage-ID: <b1@banco.example>\nDate: Wed, 28 Oct 2026 08:00:00 +0000\nMIME-Version: 1.0\nContent-Type: text/html; charset=utf-8\n\n<p>Extracto <img src=\"https://banco.example/pixel.gif\"></p><script>x()</script>\n",
        &[],
    );
    s.append(
        "Enviados",
        "From: Yo <yo@ejemplo.es>\nTo: ana@ejemplo.es\nSubject: Re: Cena del viernes\nMessage-ID: <a2@ejemplo.es>\nIn-Reply-To: <a1@ejemplo.es>\nReferences: <a1@ejemplo.es>\nDate: Mon, 26 Oct 2026 10:00:00 +0000\n\nSí, a las nueve.\n",
        &["\\Seen"],
    );

    let state = Arc::new(Mutex::new(s));
    let notify = Arc::new(Notify::new());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (st, nt) = (state.clone(), notify.clone());
    tokio::spawn(async move {
        loop {
            let (sock, _) = listener.accept().await.unwrap();
            tokio::spawn(client(sock, st.clone(), nt.clone()));
        }
    });
    Fake { state, notify, port }
}

async fn wait_for<F: Fn() -> bool>(what: &str, f: F) {
    for _ in 0..100 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("no ocurrió a tiempo: {what}");
}

#[tokio::test]
async fn syncs_threads_flags_and_new_mail_with_idle() {
    let fake = fake_server().await;
    let dir = tempfile::tempdir().unwrap();
    let user = Id::new();
    let db = UserDb::open(dir.path(), user).await.unwrap();
    let key = MasterKey::generate().derive("u");
    let port = fake.port;
    let account = db
        .add_account(
            NewAccount {
                name: "Personal",
                email: "yo@ejemplo.es",
                imap_host: "127.0.0.1",
                imap_port: port,
                smtp_host: "127.0.0.1",
                smtp_port: 25,
                login: "yo",
                password: "clave",
                imap_security: "none",
                smtp_security: "none",
            },
            &key,
        )
        .await
        .unwrap();

    // Las credenciales se comprueban antes de guardar la cuenta.
    let mut cfg = ServerConfig {
        host: "127.0.0.1".into(),
        port,
        security: Security::None,
        login: "yo".into(),
        password: "mal".into(),
    };
    let err = buzon_sync::check(&cfg).await.unwrap_err().to_string();
    assert!(err.contains("contraseña"), "{err}");
    cfg.password = "clave".into();
    buzon_sync::check(&cfg).await.unwrap();

    let hub = SyncHub::new(dir.path().to_path_buf(), true);
    hub.start(user, db.clone(), &account, "clave".into());
    wait_for("primera sincronización", || hub.state(account.id) == State::Idle).await;

    let inbox = ThreadQuery {
        role: Some("inbox".into()),
        limit: 50,
        ..Default::default()
    };
    let threads = db.threads(&inbox).await.unwrap();
    assert_eq!(threads.len(), 2, "{threads:?}");
    let bank = &threads[0];
    assert_eq!(bank.subject, "Tu extracto");
    let dinner = &threads[1];
    assert_eq!(dinner.subject, "Cena del viernes");
    assert_eq!(dinner.messages, 3, "la respuesta de Enviados va en el mismo hilo");
    assert_eq!(dinner.unread, 1);
    assert_eq!(dinner.participants, vec!["Yo", "Ana"]);

    let mbs = db.mailboxes(Some(account.id)).await.unwrap();
    let roles: Vec<_> = mbs.iter().map(|m| m.mailbox.role.clone()).collect();
    assert_eq!(mbs.len(), 3, "la carpeta virtual \\All no se sincroniza");
    assert!(roles.contains(&Some("sent".into())));

    let t = db.thread(bank.id).await.unwrap().unwrap();
    let html = t.messages[0].html.as_deref().unwrap();
    assert!(!html.contains("script"));
    assert_eq!(t.messages[0].remote_images, 1);
    assert!(user_raw_dir(dir.path(), user)
        .join(format!("{}.eml", t.messages[0].id))
        .exists());

    // Leer un hilo en Buzón lo marca como leído en el servidor.
    let changes = db.update_thread(dinner.id, Some(true), None).await.unwrap();
    assert_eq!(changes.seen.len(), 1);
    hub.store_flag(&changes.seen, "\\Seen", true);
    let st = fake.state.clone();
    wait_for("\\Seen en el servidor", || {
        st.lock().unwrap().folder("INBOX").msgs[1]
            .flags
            .contains(&"\\Seen".to_string())
    })
    .await;

    // Un correo nuevo llega al momento gracias a IDLE.
    let mut events = hub.subscribe();
    tokio::time::sleep(Duration::from_millis(300)).await;
    fake.state.lock().unwrap().append(
        "INBOX",
        "From: Luis <luis@ejemplo.es>\nSubject: Hola\nMessage-ID: <l1@ejemplo.es>\n\nQué tal\n",
        &[],
    );
    let notify = fake.notify.clone();
    let poke = tokio::spawn(async move {
        loop {
            notify.notify_waiters();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
    let ev = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let e = events.recv().await.unwrap();
            if e.kind == "mail" {
                return e;
            }
        }
    })
    .await
    .expect("aviso de correo nuevo");
    poke.abort();
    assert_eq!(ev.account, account.id);
    assert_eq!(ev.user, user);
    assert_eq!(db.threads(&inbox).await.unwrap().len(), 3);

    // Lo que se borra o se lee en otro cliente se refleja al sincronizar.
    {
        let mut s = fake.state.lock().unwrap();
        let f = s.folder("INBOX");
        f.msgs.retain(|m| m.uid != 3);
        f.msgs[0].flags.clear();
    }
    hub.sync_now(account.id);
    let db2 = db.clone();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let n = db2.threads(&inbox).await.unwrap();
            if n.len() == 2 && n.iter().any(|t| t.unread == 1 && t.subject == "Cena del viernes") {
                let _ = tx.send(());
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("borrado y flags sincronizados");

    // Un filtro archiva y etiqueta lo que llega del banco.
    let label = db.save_label(None, "Banco", "#3d7fb5").await.unwrap();
    db.save_rule(&Rule {
        name: "Banco".into(),
        enabled: true,
        from_has: "banco.example".into(),
        label_id: Some(label),
        mark_read: true,
        move_to: "archive".into(),
        ..Default::default()
    })
    .await
    .unwrap();
    fake.state.lock().unwrap().append(
        "INBOX",
        "From: Banco <avisos@banco.example>\nSubject: Recibo de noviembre\nMessage-ID: <b2@banco.example>\n\nRecibo\n",
        &[],
    );
    hub.sync_now(account.id);
    let st = fake.state.clone();
    wait_for("el filtro mueve al archivo en el servidor", || {
        let mut s = st.lock().unwrap();
        s.folder("Archivo")
            .msgs
            .iter()
            .any(|m| m.flags.contains(&"\\Seen".to_string()))
            && s.folder("INBOX").msgs.len() == 3
    })
    .await;
    let archived = ThreadQuery {
        role: Some("archive".into()),
        limit: 10,
        ..Default::default()
    };
    hub.sync_now(account.id);
    let db3 = db.clone();
    let mut found = None;
    for _ in 0..100 {
        let page = db3.threads(&archived).await.unwrap();
        let t = page.into_iter().find(|t| t.subject == "Recibo de noviembre");
        let confirmed = match &t {
            Some(t) => db3.thread(t.id).await.unwrap().unwrap().messages[0].uid > 0,
            None => false,
        };
        if confirmed {
            found = t;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let receipt = found.expect("el recibo, ya confirmado, en el archivo");
    assert_eq!(receipt.labels, vec![label]);
    assert_eq!(receipt.unread, 0);

    // Y desde Buzón se devuelve a la entrada.
    let ops = db
        .move_messages(&[receipt.id], &[], None, &Place::Role("inbox".into()))
        .await
        .unwrap();
    assert_eq!(ops.len(), 1);
    hub.move_messages(&ops);
    wait_for("de vuelta en la entrada del servidor", || {
        st.lock().unwrap().folder("INBOX").msgs.len() == 4
    })
    .await;

    hub.stop(account.id);
    assert_eq!(hub.state(account.id), State::Off);
}
