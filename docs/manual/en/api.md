# API and webhooks

Buzón has an HTTP API to connect your email with scripts and other apps (n8n, Zapier, Home Assistant…). The Buzón web app uses this same API.

The full interactive documentation is at **`/api/docs`** on your installation, and the OpenAPI specification at `/api/openapi.json`.

## Personal tokens

Create a token in **Settings › API**. When you create it you choose:

- A **name**, to remember what it's for.
- The **permissions**: only the ones it needs.
- When it **expires** (or never).

| Permission | What it allows |
| --- | --- |
| `mail:read` | read email, folders and labels |
| `mail:write` | move, mark, label and filter |
| `mail:send` | write drafts and send |
| `contacts` | read and change contacts |
| `webhooks` | manage webhooks |

The token is shown **only once**: copy it and keep it somewhere safe. Send it in the `Authorization` header:

```sh
curl -H "Authorization: Bearer bz_…" https://mail.example.com/api/v1/threads?view=principal
```

Each token can make up to 300 requests per minute. The account, security, tokens and invitations can only be managed from the web app, never with a token. You can revoke a token at any time: whatever uses it stops working immediately.

## Webhooks

A webhook notifies a URL of yours every time something happens in your email. Create one in **Settings › API › Webhooks** and choose the events:

| Event | When |
| --- | --- |
| `message.received` | an email arrives in the inbox |
| `message.sent` | you send an email |
| `message.read` | you mark a conversation as read or unread |
| `message.moved` | you move conversations (archive, trash, spam…) |
| `thread.labeled` | you add or remove labels |
| `contact.screened` | you approve or screen out a sender |
| `account.sync_failed` | an account stops syncing |

Buzón makes a `POST` with a JSON body `{id, event, created_at, data}` and these headers:

- `Buzon-Event`: the event.
- `Buzon-Delivery`: the delivery id.
- `Buzon-Signature: t=<seconds>,v1=<signature>`: the signature is HMAC-SHA256, in hex, of `<t>.<body>` with the webhook's secret.

Always check the signature and reject a `t` older than 5 minutes. For example, in Python:

```python
import hmac, hashlib, time

def valid(secret: str, header: str, body: bytes) -> bool:
    parts = dict(p.split("=", 1) for p in header.split(","))
    if abs(time.time() - int(parts["t"])) > 300:
        return False
    expected = hmac.new(secret.encode(), parts["t"].encode() + b"." + body, hashlib.sha256).hexdigest()
    return hmac.compare_digest(expected, parts["v1"])
```

Reply with a 2xx code within 10 seconds. Otherwise Buzón retries after 1 min, 5 min, 30 min, 2 h, 6 h and 15 h. Each webhook's **History** shows every delivery and its response, and lets you **Resend** it.

For security, webhooks can only go to public HTTPS addresses. The administrator can allow the local network (useful for Home Assistant or n8n at home).
