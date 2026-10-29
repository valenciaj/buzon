# Ajustes

Abre **Ajustes** desde la parte de abajo de la barra lateral.

## Apariencia

- **Tema:** claro, oscuro o como el sistema.
- **Modo de trabajo:** Enfoque o Clásico (ver [Modo Enfoque](./enfoque)).
- **Vista de cuentas:** unificada (todas juntas) o por cuenta.
- **Fuente:** por defecto, Atkinson Hyperlegible Next, diseñada para leerse con facilidad (distingue bien la l, la I y el 1, o la O y el 0). También hay Inter, Source Sans 3, IBM Plex Sans, Lexend y la fuente del sistema.
- **Tamaño del texto:** del 87,5 % al 150 %. Todo crece sin descolocarse.
- **Densidad:** cómoda o compacta. La compacta reduce márgenes, nunca el tamaño del texto.
- **Envío y avisos:** tiempo para deshacer un envío y notificaciones del navegador.

![Ajustes de apariencia](/img/es/apariencia.png)

## Seguridad

Activa o desactiva la [verificación en dos pasos](./primeros-pasos#verificacion-en-dos-pasos).

## Cuentas de correo

Aquí ves el estado de cada cuenta (al día, sincronizando, error) y la última sincronización. Puedes:

- **Sincronizar ahora**, sin esperar a la siguiente vuelta.
- **Conectar con Google o Microsoft** (si el administrador lo ha activado), sin contraseña. Si un día retiras el permiso o caduca, pulsa **Volver a conectar**.
- **Añadir una cuenta** con servidor IMAP y SMTP. Usa siempre TLS o STARTTLS.
- **Eliminar** una cuenta de Buzón. Se borra el correo guardado en Buzón, no el de tu servidor.

![Cuentas de correo](/img/es/cuentas.png)

::: tip Gmail sin OAuth
Si el administrador no ha configurado el acceso con Google, puedes añadir Gmail como cualquier otra cuenta con una **contraseña de aplicación** (en tu cuenta de Google › Seguridad › Contraseñas de aplicaciones). Servidores: `imap.gmail.com` (993, TLS) y `smtp.gmail.com` (465, TLS).
:::

## Identidades y firmas

Las direcciones desde las que escribes: la de cada cuenta y sus alias, cada una con su nombre visible y su firma. Ver [Escribir](./escribir#firmas-e-identidades).

## Etiquetas y filtros

Crea, renombra y colorea etiquetas, y define los filtros automáticos. Ver [Organizar](./organizar#etiquetas).

## API

Tokens personales y webhooks para conectar Buzón con otras apps. Ver [API y webhooks](./api).

## Invitaciones

Solo para administradores. Crea un enlace de invitación (caduca a las 72 horas y se usa una sola vez) y envíaselo a quien quieras dar acceso. Puedes añadir una nota para recordar a quién se lo diste y revocarlo mientras no se haya usado.
