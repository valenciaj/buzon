# Administration

This part is for whoever installs and maintains Buzón on a server.

## Requirements

- A Linux server with systemd (Debian or Ubuntu in the examples).
- A domain pointing at the server, for example `mail.example.com`.
- To build: stable Rust and Node 22.

Buzón isn't a mail server: it connects over IMAP and SMTP to your existing accounts.

## Installing

1. Build the package (on the server or another machine with the same architecture):

   ```sh
   ./deploy/build.sh
   ```

   This leaves `dist/buzon-<version>-<arch>.tar.gz` with the program, the web app, this manual and the `deploy/` files.

2. Copy it to the server, unpack it and install:

   ```sh
   tar xzf buzon-*.tar.gz && cd buzon-*/
   sudo ./deploy/install.sh mail.example.com you@example.com
   ```

   The script installs nginx and certbot if missing, creates the `buzon` user, the folders, the master key and the configuration, enables the systemd services, requests the Let's Encrypt certificate and configures nginx.

3. Open `https://mail.example.com` and complete the setup wizard.

### Where everything lives

| Path | What it is |
| --- | --- |
| `/usr/local/bin/buzon` | the program |
| `/usr/share/buzon/web` | the web app and this manual |
| `/etc/buzon/config.toml` | the configuration |
| `/etc/buzon/master.key` | the master key (mode 600) |
| `/var/lib/buzon` | the data: `app.db`, one database per user and downloaded email |
| `/var/backups/buzon` | backups |

::: danger Keep the master key safe
Email account passwords, 2FA secrets and webhook secrets are encrypted with the master key. Without it, a backup is useless. Keep it separately, somewhere safe.
:::

## Configuration

Everything is in `/etc/buzon/config.toml`, with comments. The most important options:

| Option | What for |
| --- | --- |
| `public_url` | the public address; used in invitations and to return from Google or Microsoft |
| `listen` | where Buzón listens (local only: nginx is the proxy) |
| `cookie_secure` | session cookie over HTTPS only (leave it `true`) |
| `allow_private_webhooks` | allows webhooks to the local network and without HTTPS |
| `[oauth.google]`, `[oauth.microsoft]` | connect Gmail and Outlook without a password |

After changing anything: `sudo systemctl restart buzon`.

## Gmail and Outlook with OAuth2

For users to connect Gmail or Outlook without a password, register Buzón as an application with Google or Microsoft. The **redirect URI** is always:

```
https://mail.example.com/api/v1/oauth/callback
```

**Google**

1. In [Google Cloud Console](https://console.cloud.google.com/), create a project.
2. In **OAuth consent screen**, set up the app and add the `https://mail.google.com/` scope.
3. In **Credentials**, create an **OAuth client ID** of type *Web application* with the redirect URI above.
4. Copy the ID and secret into the configuration:

   ```toml
   [oauth.google]
   client_id = "…apps.googleusercontent.com"
   client_secret = "…"
   ```

While the app is in *testing* mode, only the test users you add on the consent screen can connect.

**Microsoft**

1. In [Microsoft Entra](https://entra.microsoft.com/), go to **App registrations › New registration**. Platform *Web*, with the redirect URI above.
2. In **API permissions**, add the delegated permissions `IMAP.AccessAsUser.All`, `SMTP.Send` and `offline_access`.
3. In **Certificates & secrets**, create a client secret.
4. Add it to the configuration:

   ```toml
   [oauth.microsoft]
   client_id = "…"
   client_secret = "…"
   tenant = "common"   # or organizations, consumers, or your organization's id
   ```

For work accounts, the Microsoft 365 administrator must have authenticated SMTP enabled for the mailboxes.

## Backups

The installer enables a daily backup (at 03:30) with the `buzon-backup.timer` timer. It keeps the last 7 in `/var/backups/buzon/buzon-YYYYMMDD-HHMMSS`.

- Databases are copied **hot** (`VACUUM INTO`), without stopping Buzón and with no risk of half-written copies.
- Downloaded email and attachments that haven't changed are **hard-linked** to the previous backup, so each daily backup takes very little space.

To back up now or change how many are kept:

```sh
sudo -u buzon buzon --config /etc/buzon/config.toml backup /var/backups/buzon --keep 14
systemctl list-timers buzon-backup     # when the next one runs
```

Copy `/var/backups/buzon` (and, separately, the master key) to another machine: a backup on the same disk won't help if the disk fails.

### Restoring

```sh
sudo systemctl stop buzon
sudo rsync -a --delete /var/backups/buzon/buzon-YYYYMMDD-HHMMSS/ /var/lib/buzon/
sudo chown -R buzon:buzon /var/lib/buzon
sudo systemctl start buzon
```

Restore `/etc/buzon/master.key` too if it's a new server. To recover a single user, copy just their `users/<id>.db` file and `users/<id>/` folder.

## Updating

Build the package for the new version and run the installer again:

```sh
sudo ./deploy/install.sh mail.example.com
```

It replaces the program and the web app and restarts Buzón. It doesn't touch the configuration or the data. Database migrations run automatically on start. Make a backup first, just in case.

## Logs and troubleshooting

```sh
journalctl -u buzon -f          # live log
systemctl status buzon          # service status
```

- **An account doesn't sync:** the error shows in *Settings › Email accounts*. It's usually a changed password or revoked OAuth access.
- **Live updates don't arrive:** check that nginx doesn't buffer `/api/v1/events` (the example configuration already handles this).
- **Large attachments fail:** raise `client_max_body_size` in nginx.
