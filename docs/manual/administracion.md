# Administración

Esta parte es para quien instala y mantiene Buzón en un servidor.

## Requisitos

- Un servidor Linux con systemd (Debian o Ubuntu en los ejemplos).
- Un dominio que apunte al servidor, por ejemplo `correo.ejemplo.es`.
- Para compilar: Rust estable y Node 22.

Buzón no es un servidor de correo: se conecta por IMAP y SMTP a tus cuentas de siempre.

## Instalar

1. Compila el paquete (en el servidor o en otro equipo con la misma arquitectura):

   ```sh
   ./deploy/build.sh
   ```

   Deja `dist/buzon-<versión>-<arquitectura>.tar.gz` con el programa, la web, el manual y los ficheros de `deploy/`.

2. Cópialo al servidor, descomprímelo e instala:

   ```sh
   tar xzf buzon-*.tar.gz && cd buzon-*/
   sudo ./deploy/install.sh correo.ejemplo.es tu@correo.es
   ```

   El script instala nginx y certbot si faltan, crea el usuario `buzon`, las carpetas, la clave maestra y la configuración, activa los servicios de systemd, pide el certificado de Let's Encrypt y configura nginx.

3. Abre `https://correo.ejemplo.es` y completa el asistente.

### Dónde está cada cosa

| Ruta | Qué es |
| --- | --- |
| `/usr/local/bin/buzon` | el programa |
| `/usr/share/buzon/web` | la web y este manual |
| `/etc/buzon/config.toml` | la configuración |
| `/etc/buzon/master.key` | la clave maestra (permisos 600) |
| `/var/lib/buzon` | los datos: `app.db`, una base de datos por usuario y los correos descargados |
| `/var/backups/buzon` | las copias de seguridad |

::: danger Guarda la clave maestra
Las contraseñas de las cuentas de correo, los secretos de 2FA y los de los webhooks están cifrados con la clave maestra. Sin ella, una copia de seguridad no sirve para nada. Guárdala aparte, en un lugar seguro.
:::

## Configuración

Todo está en `/etc/buzon/config.toml`, con comentarios. Lo más importante:

| Opción | Para qué |
| --- | --- |
| `public_url` | la dirección pública; se usa en las invitaciones y para volver de Google o Microsoft |
| `listen` | dónde escucha Buzón (solo local: nginx hace de proxy) |
| `cookie_secure` | cookie de sesión solo por HTTPS (déjalo en `true`) |
| `allow_private_webhooks` | permite webhooks a la red local y sin HTTPS |
| `metrics_token` | activa las métricas para Grafana en `/metrics` (ver más abajo) |
| `[oauth.google]`, `[oauth.microsoft]` | conectar Gmail y Outlook sin contraseña |

Después de cambiar algo: `sudo systemctl restart buzon`.

## Gmail y Outlook con OAuth2

Para que los usuarios conecten Gmail u Outlook sin contraseña, registra Buzón como aplicación en Google o Microsoft. La **dirección de vuelta** (redirect URI) es siempre:

```
https://correo.ejemplo.es/api/v1/oauth/callback
```

**Google**

1. En [Google Cloud Console](https://console.cloud.google.com/), crea un proyecto.
2. En **Pantalla de consentimiento de OAuth**, configura la app y añade el permiso `https://mail.google.com/`.
3. En **Credenciales**, crea un **ID de cliente de OAuth** de tipo *Aplicación web* con la dirección de vuelta de arriba.
4. Copia el ID y el secreto en la configuración:

   ```toml
   [oauth.google]
   client_id = "…apps.googleusercontent.com"
   client_secret = "…"
   ```

Mientras la app esté en modo *prueba*, solo pueden conectarse los usuarios de prueba que añadas en la pantalla de consentimiento.

**Microsoft**

1. En [Microsoft Entra](https://entra.microsoft.com/), ve a **Registros de aplicaciones › Nuevo registro**. Plataforma *Web*, con la dirección de vuelta de arriba.
2. En **Permisos de API**, añade los permisos delegados `IMAP.AccessAsUser.All`, `SMTP.Send` y `offline_access`.
3. En **Certificados y secretos**, crea un secreto de cliente.
4. Añádelo a la configuración:

   ```toml
   [oauth.microsoft]
   client_id = "…"
   client_secret = "…"
   tenant = "common"   # o organizations, consumers o el id de tu organización
   ```

En cuentas de empresa, el administrador de Microsoft 365 debe tener activado SMTP autenticado para los buzones.

## Copias de seguridad

El instalador activa una copia diaria (a las 03:30) con el temporizador `buzon-backup.timer`. Guarda las 7 últimas en `/var/backups/buzon/buzon-AAAAMMDD-HHMMSS`.

- Las bases de datos se copian **en caliente** (`VACUUM INTO`), sin parar Buzón y sin riesgo de copias a medias.
- Los correos descargados y los adjuntos que no han cambiado se **enlazan** con la copia anterior, así que cada copia diaria apenas ocupa.

Para hacer una copia ahora o cambiar cuántas se guardan:

```sh
sudo -u buzon buzon --config /etc/buzon/config.toml backup /var/backups/buzon --keep 14
systemctl list-timers buzon-backup     # cuándo es la próxima
```

Copia `/var/backups/buzon` (y la clave maestra, aparte) a otro equipo: una copia en el mismo disco no te protege si falla el disco.

### Restaurar

```sh
sudo systemctl stop buzon
sudo rsync -a --delete /var/backups/buzon/buzon-AAAAMMDD-HHMMSS/ /var/lib/buzon/
sudo chown -R buzon:buzon /var/lib/buzon
sudo systemctl start buzon
```

Restaura también `/etc/buzon/master.key` si es un servidor nuevo. Para recuperar a un solo usuario, basta con copiar su fichero `users/<id>.db` y su carpeta `users/<id>/`.

## Actualizar

Compila el paquete de la nueva versión y vuelve a ejecutar el instalador:

```sh
sudo ./deploy/install.sh correo.ejemplo.es
```

Sustituye el programa y la web, y reinicia Buzón. No toca la configuración ni los datos. Las migraciones de la base de datos se aplican solas al arrancar. Haz antes una copia, por si acaso.

## Métricas para Grafana

Buzón publica sus estadísticas en `/metrics`, en el formato de Prometheus. Para activarlas, pon un token en la configuración y reinicia:

```toml
metrics_token = "pon-aquí-un-token-largo"   # openssl rand -hex 32
```

Sin token, `/metrics` no existe. Con él, hay que mandarlo en cada petición. En Prometheus:

```yaml
scrape_configs:
  - job_name: buzon
    scheme: https
    authorization:
      credentials: pon-aquí-un-token-largo
    static_configs:
      - targets: ["correo.ejemplo.es"]
```

Después, en Grafana, añade Prometheus como origen de datos. Las métricas:

| Métrica | Qué es |
| --- | --- |
| `buzon_users` | usuarios |
| `buzon_accounts{state}` | cuentas de correo por estado: `idle`, `syncing`, `connecting`, `error`, `off` |
| `buzon_scheduled_sends` | correos programados que aún no han salido |
| `buzon_webhook_queue` | avisos de webhooks pendientes de entregar |
| `buzon_mails_sent_total` | correos enviados desde que arrancó el servidor |
| `buzon_send_failures_total` | envíos que no pudieron salir |
| `buzon_webhook_deliveries_total{result}` | entregas de webhooks: `ok` o `failed` |
| `buzon_start_time_seconds`, `buzon_info{version}` | cuándo arrancó y qué versión es |

Una alerta útil: `buzon_accounts{state="error"} > 0`.

## Registros y problemas

```sh
journalctl -u buzon -f          # registro en vivo
systemctl status buzon          # estado del servicio
```

- **Una cuenta no sincroniza:** el error aparece en *Ajustes › Cuentas de correo*. Suele ser una contraseña cambiada o un permiso de OAuth retirado.
- **Los avisos en vivo no llegan:** revisa que nginx no guarde en búfer `/api/v1/events` (la configuración de ejemplo ya lo tiene en cuenta).
- **Adjuntos grandes fallan:** sube `client_max_body_size` en nginx.
