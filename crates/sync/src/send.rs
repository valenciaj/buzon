//! Envío por SMTP: construye el mensaje MIME y lo entrega al servidor de la cuenta.

use std::time::Duration;

use buzon_store::mail::Addr;
use lettre::address::Envelope;
use lettre::message::header::ContentType;
use lettre::message::{Attachment, Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor};

use crate::{Auth, Mech, Security};

const TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("dirección no válida: {0}")]
    Address(String),
    #[error("no se pudo crear el mensaje: {0}")]
    Build(String),
    #[error("el servidor SMTP rechazó el envío: {0}")]
    Smtp(String),
}

#[derive(Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub security: Security,
    pub login: String,
    pub auth: Auth,
    pub mech: Mech,
}

/// Un adjunto ya leído.
pub struct OutAttachment {
    pub name: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub struct Outgoing {
    pub from: Addr,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub bcc: Vec<Addr>,
    pub subject: String,
    pub text: String,
    pub html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<OutAttachment>,
}

fn mailbox(a: &Addr) -> Result<Mailbox, SendError> {
    let email = a
        .email
        .trim()
        .parse()
        .map_err(|_| SendError::Address(a.email.clone()))?;
    Ok(Mailbox::new(a.name.clone().filter(|n| !n.trim().is_empty()), email))
}

/// Construye el mensaje. Devuelve también su `Message-ID` (sin `<>`).
pub fn build(o: &Outgoing) -> Result<(Message, String), SendError> {
    build_with(o, None)
}

/// Cabecera con la que Buzón reconoce sus borradores en la carpeta Borradores del servidor.
pub const DRAFT_HEADER: &str = "X-Buzon-Draft";

/// El borrador `key` como mensaje para guardarlo en la carpeta Borradores. Puede no tener
/// destinatarios todavía.
pub fn build_draft(o: &Outgoing, key: &str) -> Result<Vec<u8>, SendError> {
    let from = mailbox(&o.from)?;
    let envelope =
        Envelope::new(Some(from.email.clone()), vec![from.email]).map_err(|e| SendError::Build(e.to_string()))?;
    let (msg, _) = build_with(o, Some(envelope))?;
    let mut raw = format!("{DRAFT_HEADER}: {key}\r\n").into_bytes();
    // lettre quita la copia oculta de las cabeceras; en un borrador hay que conservarla.
    if !o.bcc.is_empty() {
        let bcc: Vec<String> = o
            .bcc
            .iter()
            .map(|a| mailbox(a).map(|m| m.email.to_string()))
            .collect::<Result<_, _>>()?;
        raw.extend(format!("Bcc: {}\r\n", bcc.join(", ")).into_bytes());
    }
    raw.extend(msg.formatted());
    Ok(raw)
}

/// Si un mensaje es una copia de un borrador de Buzón (mira solo las cabeceras).
pub fn is_draft_copy(raw: &[u8]) -> bool {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
    let name = DRAFT_HEADER.as_bytes();
    raw[..end].split(|b| *b == b'\n').any(|line| {
        line.len() > name.len() && line[..name.len()].eq_ignore_ascii_case(name) && line[name.len()] == b':'
    })
}

fn build_with(o: &Outgoing, envelope: Option<Envelope>) -> Result<(Message, String), SendError> {
    let from = mailbox(&o.from)?;
    let domain = from.email.domain().to_string();
    let message_id = format!("{}@{domain}", buzon_store::Id::new());
    let mut b = Message::builder()
        .from(from)
        .subject(o.subject.trim())
        .message_id(Some(format!("<{message_id}>")))
        .date_now()
        .user_agent(format!("Buzón {}", env!("CARGO_PKG_VERSION")));
    if let Some(e) = envelope {
        b = b.envelope(e);
    }
    for a in &o.to {
        b = b.to(mailbox(a)?);
    }
    for a in &o.cc {
        b = b.cc(mailbox(a)?);
    }
    for a in &o.bcc {
        b = b.bcc(mailbox(a)?);
    }
    if let Some(r) = &o.in_reply_to {
        b = b.in_reply_to(format!("<{r}>"));
    }
    if !o.references.is_empty() {
        let refs: Vec<String> = o.references.iter().map(|r| format!("<{r}>")).collect();
        b = b.references(refs.join(" "));
    }

    let alternative = |html: &String| MultiPart::alternative_plain_html(o.text.clone(), html.clone());
    let msg = match (&o.html, o.attachments.is_empty()) {
        (Some(html), true) => b.multipart(alternative(html)),
        (None, true) => b.singlepart(SinglePart::plain(o.text.clone())),
        (html, false) => {
            let mut mixed = match html {
                Some(html) => MultiPart::mixed().multipart(alternative(html)),
                None => MultiPart::mixed().singlepart(SinglePart::plain(o.text.clone())),
            };
            for a in &o.attachments {
                let ct = ContentType::parse(&a.content_type)
                    .unwrap_or_else(|_| ContentType::parse("application/octet-stream").expect("tipo"));
                mixed = mixed.singlepart(Attachment::new(a.name.clone()).body(a.data.clone(), ct));
            }
            b.multipart(mixed)
        }
    }
    .map_err(|e| SendError::Build(e.to_string()))?;
    Ok((msg, message_id))
}

/// Entrega el mensaje al servidor SMTP.
pub async fn send(cfg: &SmtpConfig, msg: &Message) -> Result<(), SendError> {
    let smtp = |e: lettre::transport::smtp::Error| SendError::Smtp(e.to_string());
    let builder = match cfg.security {
        Security::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(&cfg.host).map_err(smtp)?,
        Security::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&cfg.host).map_err(smtp)?,
        Security::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&cfg.host),
    };
    let builder = match &cfg.auth {
        Auth::Password(p) => {
            let b = builder.credentials(Credentials::new(cfg.login.clone(), p.clone()));
            match cfg.mech {
                Mech::Auto => b,
                Mech::Plain => b.authentication(vec![Mechanism::Plain]),
                Mech::Login => b.authentication(vec![Mechanism::Login]),
            }
        }
        Auth::XOAuth2(token) => builder
            .credentials(Credentials::new(cfg.login.clone(), token.clone()))
            .authentication(vec![Mechanism::Xoauth2]),
    };
    let transport = builder.port(cfg.port).timeout(Some(TIMEOUT)).build();
    transport.send(msg.clone()).await.map_err(smtp)?;
    Ok(())
}

/// Servidores que ya guardan una copia en Enviados al recibir el envío por SMTP.
pub fn server_saves_sent(smtp_host: &str) -> bool {
    let h = smtp_host.to_ascii_lowercase();
    ["gmail.com", "googlemail.com", "office365.com", "outlook.com"]
        .iter()
        .any(|d| h.ends_with(d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(name: Option<&str>, email: &str) -> Addr {
        Addr {
            name: name.map(Into::into),
            email: email.into(),
        }
    }

    #[test]
    fn builds_reply_with_attachment() {
        let (msg, id) = build(&Outgoing {
            from: addr(Some("Javier"), "yo@ejemplo.es"),
            to: vec![addr(Some("Ana García"), "ana@ejemplo.es")],
            cc: vec![],
            bcc: vec![addr(None, "oculto@ejemplo.es")],
            subject: "Re: Cena".into(),
            text: "Allí estaré.".into(),
            html: Some("<p>Allí estaré.</p>".into()),
            in_reply_to: Some("a1@ejemplo.es".into()),
            references: vec!["a0@ejemplo.es".into(), "a1@ejemplo.es".into()],
            attachments: vec![OutAttachment {
                name: "menú.pdf".into(),
                content_type: "application/pdf".into(),
                data: b"%PDF".to_vec(),
            }],
        })
        .unwrap();
        assert!(id.ends_with("@ejemplo.es"));
        let raw = String::from_utf8(msg.formatted()).unwrap();
        assert!(raw.contains(&format!("Message-ID: <{id}>")), "{raw}");
        assert!(raw.contains("In-Reply-To: <a1@ejemplo.es>"));
        assert!(raw.contains("References: <a0@ejemplo.es> <a1@ejemplo.es>"));
        assert!(raw.contains("multipart/alternative"));
        assert!(raw.contains("application/pdf"));
        assert!(!raw.contains("oculto@"), "Bcc no va en las cabeceras");
        assert_eq!(msg.envelope().to().len(), 2, "pero sí en el sobre");

        // Y se entiende al volver a leerlo.
        let parsed = crate::parse::parse(
            &msg.formatted(),
            crate::parse::Envelope {
                id: buzon_store::Id::new(),
                account_id: buzon_store::Id::new(),
                mailbox_id: buzon_store::Id::new(),
                uid: 1,
                size: 0,
                flags: Default::default(),
                fallback_date: 0,
            },
        );
        assert_eq!(parsed.in_reply_to.as_deref(), Some("a1@ejemplo.es"));
        assert_eq!(parsed.attachments[0].name, "menú.pdf");
        assert_eq!(parsed.from.unwrap().name.as_deref(), Some("Javier"));
    }

    #[test]
    fn rejects_bad_addresses() {
        let err = build(&Outgoing {
            from: addr(None, "yo@ejemplo.es"),
            to: vec![addr(None, "no es una dirección")],
            cc: vec![],
            bcc: vec![],
            subject: String::new(),
            text: String::new(),
            html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        });
        assert!(matches!(err, Err(SendError::Address(_))));
        assert!(server_saves_sent("smtp.gmail.com"));
        assert!(!server_saves_sent("smtp.ejemplo.es"));
    }

    #[test]
    fn drafts_carry_their_key_and_may_lack_recipients() {
        let mut o = Outgoing {
            from: addr(Some("Javier"), "yo@ejemplo.es"),
            to: vec![],
            cc: vec![],
            bcc: vec![addr(None, "oculto@ejemplo.es")],
            subject: "A medias".into(),
            text: "Hola".into(),
            html: None,
            in_reply_to: None,
            references: vec![],
            attachments: vec![],
        };
        let raw = build_draft(&o, "abc123").unwrap();
        let text = String::from_utf8(raw.clone()).unwrap();
        assert!(
            text.starts_with("X-Buzon-Draft: abc123\r\nBcc: oculto@ejemplo.es\r\n"),
            "{text}"
        );
        assert!(text.contains("Subject: A medias"));
        assert!(is_draft_copy(&raw));
        assert!(is_draft_copy(b"Subject: x\r\nx-buzon-draft: abc\r\n\r\nhola"));
        assert!(!is_draft_copy(b"Subject: x\r\n\r\nX-Buzon-Draft: en el cuerpo"));
        o.bcc.clear();
        assert!(build_draft(&o, "k").is_ok());
    }
}
