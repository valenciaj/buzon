# Prototipo visual (fase 0)

Prototipo navegable de las pantallas clave de Buzón con datos de ejemplo: bandeja en modo Enfoque y Clásico, lector, redactar, filtro de remitentes («Por aprobar») y ajustes (tema, acento, fuente, tamaño de texto, densidad, vista de cuentas).

Es un único fichero sin dependencias de build. Ábrelo en el navegador o sírvelo con cualquier servidor estático:

```sh
python3 -m http.server -d prototype 8000
```

No envía ni recibe correo: todas las acciones son simuladas. Sirve para validar el diseño antes de la fase 1.
