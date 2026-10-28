//! Análisis de mensajes MIME y saneado del HTML para el lector.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use buzon_store::mail::{Addr, Attachment, Flags, NewMessage};
use buzon_store::Id;
use mail_parser::{Address, HeaderValue, MessageParser, MimeHeaders};

/// URL de un adjunto dentro de la API (también para las imágenes `cid:`).
pub fn attachment_url(message: Id, index: u32) -> String {
    format!("/api/v1/messages/{message}/attachments/{index}")
}

/// Lo que se sabe de un mensaje antes de guardarlo: ids, carpeta y UID.
pub struct Envelope {
    pub id: Id,
    pub account_id: Id,
    pub mailbox_id: Id,
    pub uid: u32,
    pub size: u32,
    pub flags: Flags,
    /// Fecha a usar si el mensaje no trae cabecera `Date`.
    pub fallback_date: i64,
}

fn addrs(a: Option<&Address<'_>>) -> Vec<Addr> {
    a.map(|a| {
        a.iter()
            .filter_map(|x| {
                Some(Addr {
                    name: x.name().map(str::trim).filter(|n| !n.is_empty()).map(String::from),
                    email: x.address()?.trim().to_string(),
                })
            })
            .collect()
    })
    .unwrap_or_default()
}

fn id_list(v: &HeaderValue<'_>) -> Vec<String> {
    match v {
        HeaderValue::Text(t) => vec![t.to_string()],
        HeaderValue::TextList(l) => l.iter().map(|t| t.to_string()).collect(),
        _ => Vec::new(),
    }
}

/// Texto corto para la lista: sin líneas citadas y con los espacios colapsados.
pub fn snippet(text: &str) -> String {
    let joined = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('>'))
        .collect::<Vec<_>>()
        .join(" ");
    let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = collapsed.chars().take(200).collect();
    if collapsed.chars().count() > 200 {
        out.push('…');
    }
    out
}

/// Analiza un mensaje original. Nunca falla: si no se entiende, queda casi vacío.
pub fn parse(raw: &[u8], env: Envelope) -> NewMessage {
    let mut m = NewMessage {
        id: env.id,
        account_id: env.account_id,
        mailbox_id: env.mailbox_id,
        uid: env.uid,
        message_id: None,
        in_reply_to: None,
        refs: Vec::new(),
        subject: String::new(),
        from: None,
        to: Vec::new(),
        cc: Vec::new(),
        date: env.fallback_date,
        snippet: String::new(),
        size: env.size,
        flags: env.flags,
        text: None,
        html: None,
        remote_images: 0,
        attachments: Vec::new(),
    };
    let Some(msg) = MessageParser::default().parse(raw) else {
        m.text = Some(String::from_utf8_lossy(raw).into_owned());
        return m;
    };

    m.message_id = msg.message_id().map(String::from);
    m.in_reply_to = id_list(msg.in_reply_to()).into_iter().next();
    m.refs = id_list(msg.references());
    m.subject = msg.subject().unwrap_or("").trim().to_string();
    m.from = addrs(msg.from()).into_iter().next();
    m.to = addrs(msg.to());
    m.cc = addrs(msg.cc());
    if let Some(d) = msg.date() {
        m.date = d.to_timestamp();
    }

    let mut cids = HashMap::new();
    for (i, part) in msg.attachments().enumerate() {
        let ct = part
            .content_type()
            .map(|c| match c.subtype() {
                Some(s) => format!("{}/{}", c.ctype(), s),
                None => c.ctype().to_string(),
            })
            .unwrap_or_else(|| "application/octet-stream".into())
            .to_ascii_lowercase();
        let content_id = part
            .content_id()
            .map(|c| c.trim_matches(|ch| ch == '<' || ch == '>').to_string());
        if let Some(cid) = &content_id {
            cids.insert(cid.clone(), attachment_url(env.id, i as u32));
        }
        m.attachments.push(Attachment {
            index: i as u32,
            name: part
                .attachment_name()
                .map(String::from)
                .unwrap_or_else(|| format!("adjunto-{}", i + 1)),
            content_type: ct,
            size: part.contents().len() as u64,
            content_id,
            inline: false,
        });
    }

    let has_html = msg.html_bodies().any(|p| p.is_content_type("text", "html"));
    if has_html {
        if let Some(html) = msg.body_html(0) {
            let clean = sanitize_html(&html, &cids);
            m.remote_images = clean.remote_images;
            for a in &mut m.attachments {
                if a.content_id.as_ref().is_some_and(|c| clean.used_cids.contains(c)) {
                    a.inline = true;
                }
            }
            m.html = Some(clean.html);
        }
    }
    m.text = msg.body_text(0).map(|t| t.into_owned());
    m.snippet = snippet(m.text.as_deref().unwrap_or(""));
    m
}

/// Contenido de un adjunto del mensaje original, por su posición.
pub fn attachment(raw: &[u8], index: u32) -> Option<Vec<u8>> {
    let msg = MessageParser::default().parse(raw)?;
    let part = msg.attachments().nth(index as usize)?;
    Some(part.contents().to_vec())
}

pub struct CleanHtml {
    pub html: String,
    /// Imágenes y fondos remotos (se bloquean hasta que la persona los pida).
    pub remote_images: u32,
    pub used_cids: HashSet<String>,
}

/// Sanea el HTML de un correo: sin scripts, formularios ni eventos; enlaces en otra
/// pestaña; imágenes `cid:` apuntando a la API. Las imágenes remotas se dejan, pero el
/// lector las bloquea con CSP hasta que la persona las pida.
pub fn sanitize_html(html: &str, cids: &HashMap<String, String>) -> CleanHtml {
    let remote = Arc::new(AtomicU32::new(0));
    let used = Arc::new(std::sync::Mutex::new(HashSet::new()));
    let (remote_f, used_f, cids_f) = (remote.clone(), used.clone(), cids.clone());

    let mut b = ammonia::Builder::default();
    b.rm_clean_content_tags(&["style"])
        .add_tags(&[
            "style", "font", "center", "span", "div", "table", "thead", "tbody", "tfoot", "tr", "td", "th", "caption",
            "col", "colgroup", "hr", "u", "s", "big", "small",
        ])
        .add_generic_attributes(&[
            "style",
            "class",
            "align",
            "valign",
            "width",
            "height",
            "bgcolor",
            "color",
            "dir",
            "border",
            "cellpadding",
            "cellspacing",
            "face",
            "size",
            "colspan",
            "rowspan",
            "background",
        ])
        .add_tag_attributes("img", &["src", "alt", "width", "height", "title"])
        .add_url_schemes(&["cid"])
        .link_rel(Some("noopener noreferrer"))
        .set_tag_attribute_value("a", "target", "_blank")
        .attribute_filter(move |el, attr, value| {
            if !(attr == "background" || (el == "img" && attr == "src")) {
                return Some(value.into());
            }
            if let Some(cid) = value.strip_prefix("cid:") {
                let url = cids_f.get(cid)?;
                used_f.lock().expect("mutex").insert(cid.to_string());
                return Some(url.clone().into());
            }
            let lower = value.trim().to_ascii_lowercase();
            if lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("//") {
                remote_f.fetch_add(1, Ordering::Relaxed);
            }
            Some(value.into())
        });
    let html = b.clean(html).to_string();
    let used_cids = used.lock().expect("mutex").clone();
    CleanHtml {
        html,
        remote_images: remote.load(Ordering::Relaxed),
        used_cids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Envelope {
        Envelope {
            id: Id::new(),
            account_id: Id::new(),
            mailbox_id: Id::new(),
            uid: 7,
            size: 0,
            flags: Flags::default(),
            fallback_date: 1,
        }
    }

    const MAIL: &str = "From: =?UTF-8?Q?Mar=C3=ADa_L=C3=B3pez?= <maria@ejemplo.es>\r
To: Javier <javier@ejemplo.es>, otra@ejemplo.es\r
Cc: copia@ejemplo.es\r
Subject: =?UTF-8?Q?Re:_Reuni=C3=B3n?=\r
Date: Tue, 27 Oct 2026 10:00:00 +0100\r
Message-ID: <b@ejemplo.es>\r
In-Reply-To: <a@ejemplo.es>\r
References: <r0@ejemplo.es> <a@ejemplo.es>\r
MIME-Version: 1.0\r
Content-Type: multipart/mixed; boundary=\"MIX\"\r
\r
--MIX\r
Content-Type: multipart/related; boundary=\"REL\"\r
\r
--REL\r
Content-Type: multipart/alternative; boundary=\"ALT\"\r
\r
--ALT\r
Content-Type: text/plain; charset=utf-8\r
\r
Hola Javier,\r
> texto citado\r
nos vemos el jueves.\r
--ALT\r
Content-Type: text/html; charset=utf-8\r
\r
<html><head><style>p{color:red}</style><script>alert(1)</script></head><body>\
<p onclick=\"x()\" style=\"color:#333\">Hola <b>Javier</b></p><img src=\"cid:logo@x\">\
<img src=\"https://tracker.example/p.gif\"><a href=\"https://ejemplo.es\">web</a>\
<a href=\"javascript:alert(1)\">mal</a><form><input></form><iframe src=\"https://x\"></iframe>\
<table background=\"http://x/fondo.png\"><tr><td>c</td></tr></table></body></html>\r
--ALT--\r
--REL\r
Content-Type: image/png\r
Content-ID: <logo@x>\r
Content-Transfer-Encoding: base64\r
\r
iVBORw0KGgo=\r
--REL--\r
--MIX\r
Content-Type: application/pdf; name=\"factura.pdf\"\r
Content-Disposition: attachment; filename=\"factura.pdf\"\r
Content-Transfer-Encoding: base64\r
\r
JVBERi0xLjQK\r
--MIX--\r
";

    #[test]
    fn parses_headers_bodies_and_attachments() {
        let e = env();
        let id = e.id;
        let m = parse(MAIL.as_bytes(), e);
        assert_eq!(m.subject, "Re: Reunión");
        assert_eq!(m.message_id.as_deref(), Some("b@ejemplo.es"));
        assert_eq!(m.in_reply_to.as_deref(), Some("a@ejemplo.es"));
        assert_eq!(m.refs, vec!["r0@ejemplo.es", "a@ejemplo.es"]);
        let from = m.from.unwrap();
        assert_eq!(from.name.as_deref(), Some("María López"));
        assert_eq!(from.email, "maria@ejemplo.es");
        assert_eq!(m.to.len(), 2);
        assert_eq!(m.cc[0].email, "copia@ejemplo.es");
        assert_eq!(m.date, 1_793_091_600);
        assert_eq!(m.snippet, "Hola Javier, nos vemos el jueves.");

        assert_eq!(m.attachments.len(), 2, "{:?}", m.attachments);
        let logo = m
            .attachments
            .iter()
            .find(|a| a.content_id.as_deref() == Some("logo@x"))
            .unwrap();
        assert!(logo.inline);
        assert_eq!(logo.content_type, "image/png");
        let pdf = m.attachments.iter().find(|a| a.name == "factura.pdf").unwrap();
        assert_eq!(attachment(MAIL.as_bytes(), pdf.index).unwrap(), b"%PDF-1.4\n");
        assert!(!pdf.inline);
        assert_eq!(pdf.content_type, "application/pdf");

        let html = m.html.unwrap();
        assert!(html.contains(&attachment_url(id, logo.index)), "{html}");
        assert!(html.contains("p{color:red}"), "{html}");
        assert!(html.contains("style=\"color:#333\""), "{html}");
        for bad in [
            "<script",
            "alert(1)",
            "onclick",
            "javascript:",
            "<form",
            "<input",
            "<iframe",
        ] {
            assert!(!html.contains(bad), "{bad} en {html}");
        }
        assert!(html.contains("target=\"_blank\""));
        assert!(html.contains("rel=\"noopener noreferrer\""));
        assert_eq!(m.remote_images, 2);
    }

    #[test]
    fn plain_text_mail_has_no_html() {
        let raw = b"From: a@b.es\r\nSubject: Hola\r\n\r\nSolo texto\r\n";
        let m = parse(raw, env());
        assert!(m.html.is_none());
        assert_eq!(m.text.as_deref().map(str::trim), Some("Solo texto"));
        assert_eq!(m.date, 1);
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn snippet_is_short() {
        let s = snippet(&"palabra ".repeat(100));
        assert_eq!(s.chars().count(), 201);
        assert!(s.ends_with('…'));
    }
}
