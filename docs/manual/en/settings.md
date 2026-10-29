# Settings

Open **Settings** from the bottom of the sidebar.

## Appearance

- **Theme:** light, dark or follow the system.
- **Work mode:** Focus or Classic (see [Focus mode](./focus)).
- **Account view:** unified (all together) or per account.
- **Font:** by default, Atkinson Hyperlegible Next, designed to be easy to read (it tells l, I and 1, or O and 0, apart). There's also Inter, Source Sans 3, IBM Plex Sans, Lexend and the system font.
- **Text size:** from 87.5% to 150%. Everything grows without breaking.
- **Density:** comfortable or compact. Compact reduces spacing, never the text size.
- **Sending and notifications:** time to undo a send, and browser notifications.

![Appearance settings](/img/en/apariencia.png)

## Security

Turn [two-step verification](./getting-started#two-step-verification) on or off.

## Email accounts

Here you see each account's state (up to date, syncing, error) and the last sync. You can:

- **Sync now**, without waiting for the next round.
- **Connect with Google or Microsoft** (if the administrator has enabled it), without a password. If you ever revoke access or it expires, click **Reconnect**.
- **Add an account** with an IMAP and SMTP server. Always use TLS or STARTTLS.
- **Delete** an account from Buzón. This deletes the mail stored in Buzón, not on your server.

![Email accounts](/img/en/cuentas.png)

::: tip Gmail without OAuth
If the administrator hasn't set up Google sign-in, you can add Gmail like any other account with an **app password** (in your Google Account › Security › App passwords). Servers: `imap.gmail.com` (993, TLS) and `smtp.gmail.com` (465, TLS).
:::

## Identities & signatures

The addresses you write from: each account's own and its aliases, each with its display name and signature. See [Writing](./writing#signatures-and-identities).

## Labels and filters

Create, rename and color labels, and set up automatic filters. See [Organizing](./organizing#labels).

## API

Personal tokens and webhooks to connect Buzón with other apps. See [API and webhooks](./api).

## Invitations

Administrators only. Create an invitation link (it expires after 72 hours and works once) and send it to whoever you want to give access. You can add a note to remember who it was for, and revoke it while it's unused.
