# API y webhooks

Buzón tiene una API HTTP para conectar tu correo con scripts y otras apps (n8n, Zapier, Home Assistant…). La web de Buzón usa esta misma API.

La documentación completa e interactiva está en **`/api/docs`** de tu instalación, y la especificación OpenAPI en `/api/openapi.json`.

## Tokens personales

Crea un token en **Ajustes › API**. Al crearlo eliges:

- Un **nombre**, para recordar para qué es.
- Los **permisos**: solo los que necesite.
- Cuándo **caduca** (o nunca).

| Permiso | Qué permite |
| --- | --- |
| `mail:read` | leer el correo, las carpetas y las etiquetas |
| `mail:write` | mover, marcar, etiquetar y filtrar |
| `mail:send` | escribir borradores y enviar |
| `contacts` | leer y cambiar los contactos |
| `webhooks` | gestionar los webhooks |

El token se muestra **una sola vez**: cópialo y guárdalo en un sitio seguro. Se envía en la cabecera `Authorization`:

```sh
curl -H "Authorization: Bearer bz_…" https://correo.ejemplo.es/api/v1/threads?view=principal
```

Cada token puede hacer hasta 300 peticiones por minuto. La cuenta, la seguridad, los tokens y las invitaciones solo se gestionan desde la web, nunca con un token. Puedes revocar un token en cualquier momento: lo que lo use dejará de funcionar al momento.

## Webhooks

Un webhook avisa a una URL tuya cada vez que pasa algo en tu correo. Créalo en **Ajustes › API › Webhooks** y elige los eventos:

| Evento | Cuándo |
| --- | --- |
| `message.received` | llega un correo a la entrada |
| `message.sent` | envías un correo |
| `message.read` | marcas una conversación como leída o no leída |
| `message.moved` | mueves conversaciones (archivar, papelera, spam…) |
| `thread.labeled` | pones o quitas etiquetas |
| `contact.screened` | apruebas o rechazas a un remitente |
| `account.sync_failed` | una cuenta deja de sincronizar |

Buzón hace un `POST` con un JSON `{id, event, created_at, data}` y estas cabeceras:

- `Buzon-Event`: el evento.
- `Buzon-Delivery`: el id del envío.
- `Buzon-Signature: t=<segundos>,v1=<firma>`: la firma es HMAC-SHA256, en hexadecimal, de `<t>.<cuerpo>` con el secreto del webhook.

Comprueba siempre la firma y rechaza un `t` de hace más de 5 minutos. Por ejemplo, en Python:

```python
import hmac, hashlib, time

def valido(secreto: str, cabecera: str, cuerpo: bytes) -> bool:
    partes = dict(p.split("=", 1) for p in cabecera.split(","))
    if abs(time.time() - int(partes["t"])) > 300:
        return False
    esperado = hmac.new(secreto.encode(), partes["t"].encode() + b"." + cuerpo, hashlib.sha256).hexdigest()
    return hmac.compare_digest(esperado, partes["v1"])
```

Responde con un código 2xx en menos de 10 segundos. Si no, Buzón lo reintenta a los 1 min, 5 min, 30 min, 2 h, 6 h y 15 h. En el **historial** de cada webhook ves cada envío, su respuesta, y puedes **reenviarlo**.

Por seguridad, los webhooks solo pueden ir a direcciones públicas con HTTPS. El administrador puede permitir la red local (útil para Home Assistant o n8n en casa).
