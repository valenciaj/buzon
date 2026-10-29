#!/usr/bin/env bash
# Instala o actualiza Buzón en Debian o Ubuntu, detrás de nginx con Let's Encrypt.
# Se ejecuta como root desde la carpeta del paquete (ver build.sh):
#
#   sudo ./deploy/install.sh correo.ejemplo.es tu@correo.es
#
# Es seguro volver a ejecutarlo para actualizar: no toca la configuración ni los datos.
set -euo pipefail

domain=${1:?Uso: install.sh DOMINIO [EMAIL_PARA_LETSENCRYPT]}
email=${2:-}
here=$(cd "$(dirname "$0")/.." && pwd)
[ "$(id -u)" = 0 ] || { echo "Ejecútalo como root (sudo)."; exit 1; }
[ -x "$here/buzon" ] || { echo "No encuentro el binario: ejecútalo desde el paquete de build.sh."; exit 1; }

echo "==> Paquetes: nginx y certbot"
if ! command -v nginx >/dev/null || ! command -v certbot >/dev/null; then
  apt-get update -q
  apt-get install -y -q nginx certbot python3-certbot-nginx
fi

echo "==> Usuario y carpetas"
id buzon >/dev/null 2>&1 || useradd --system --home /var/lib/buzon --shell /usr/sbin/nologin buzon
install -d -o buzon -g buzon -m 700 /var/lib/buzon /var/backups/buzon
install -d -o root -g buzon -m 750 /etc/buzon

echo "==> Programa y web"
updating=false
systemctl is-active --quiet buzon && updating=true
install -m 755 "$here/buzon" /usr/local/bin/buzon
rm -rf /usr/share/buzon/web
install -d /usr/share/buzon
cp -r "$here/web" /usr/share/buzon/web

if [ ! -f /etc/buzon/config.toml ]; then
  sed "s#https://correo.ejemplo.es#https://$domain#" "$here/deploy/config.example.toml" > /etc/buzon/config.toml
  chown root:buzon /etc/buzon/config.toml
  chmod 640 /etc/buzon/config.toml
fi
# El servicio solo puede leer /etc/buzon, así que la clave maestra se crea aquí.
if [ ! -f /etc/buzon/master.key ]; then
  head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n' > /etc/buzon/master.key
  echo >> /etc/buzon/master.key
  chown buzon:buzon /etc/buzon/master.key
  chmod 600 /etc/buzon/master.key
  echo "    Clave maestra creada en /etc/buzon/master.key: guárdala en un lugar seguro."
fi

echo "==> systemd"
install -m 644 "$here/deploy/buzon.service" "$here/deploy/buzon-backup.service" "$here/deploy/buzon-backup.timer" /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now buzon-backup.timer
if $updating; then systemctl restart buzon; else systemctl enable --now buzon; fi

echo "==> nginx"
site=/etc/nginx/sites-available/buzon
if [ ! -f "$site" ]; then
  if [ ! -d "/etc/letsencrypt/live/$domain" ]; then
    # Primero un sitio solo HTTP para que certbot pueda validar el dominio.
    printf 'server {\n    listen 80;\n    listen [::]:80;\n    server_name %s;\n    location / { proxy_pass http://127.0.0.1:8080; }\n}\n' "$domain" > "$site"
    ln -sf "$site" /etc/nginx/sites-enabled/buzon
    nginx -t && systemctl reload nginx
    if [ -n "$email" ]; then account=(--email "$email"); else account=(--register-unsafely-without-email); fi
    # Al renovar (temporizador de certbot), nginx recarga el certificado nuevo.
    certbot certonly --nginx -d "$domain" --non-interactive --agree-tos "${account[@]}" \
      --deploy-hook "systemctl reload nginx"
  fi
  sed "s/correo.ejemplo.es/$domain/g" "$here/deploy/nginx.conf" > "$site"
  ln -sf "$site" /etc/nginx/sites-enabled/buzon
fi
nginx -t && systemctl reload nginx

echo
if $updating; then
  echo "Buzón actualizado. Las migraciones se aplican solas al arrancar."
else
  echo "Buzón instalado. Abre https://$domain para terminar el asistente."
fi
echo "Copias diarias en /var/backups/buzon (systemctl list-timers buzon-backup)."
