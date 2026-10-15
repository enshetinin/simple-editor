# Hitos y reglas de avance

## H0 — Viabilidad y aplicación vertical
Inspeccionar repo. Crear proyecto Rust mínimo y estructura modular. Ventana con sidebar y pestañas. Abrir/editar/guardar UTF-8, indicador dirty y cierre seguro. Cliente HTTP con método, URL, cuerpo opcional, Send/Cancel, status y respuesta raw contra un servidor local. CLI `se .` y apertura de archivo. README con requisitos y comandos reales. CI mínima de compilación y pruebas en macOS y Windows.

Prototipar edición de archivo grande, Unicode/IME y consumo en reposo. Documentar la decisión de buffer/renderizado y limitaciones del widget. No declarar listo el editor definitivo solo porque funciona un campo multilinea.

Salida: aplicación ejecutable, flujo real código→HTTP, pruebas de guardado y HTTP, primer informe de rendimiento. Detenerse al cerrar H0 si este era el encargo; dejar H1 claramente descrito.

## H1 — Editor fiable
Buffer definitivo según H0; historial transaccional; selección/Unicode/IME; buscar/reemplazar; árbol bajo demanda; indentación; números de línea; resaltado incremental para lenguajes acordados; final de línea/BOM; conflictos externos; guardado robusto y recuperación. Validar límites de archivos grandes y líneas largas.

Salida: matriz del editor en ACCEPTANCE completada y métricas sin bloqueos perceptibles en corpus acordado.

## H2 — HTTP de uso diario
Query/headers repetidos, auth, variables, entornos, JSON pretty/raw, límites de respuesta, historial saneado, colecciones versionadas, import/export propio y subconjunto cURL, secretos con persistencia opt-in, redirecciones y errores claros.

Salida: suite local reproducible de HTTP y flujo de colección entre reinicios, sin secretos en exportación por defecto.

## H3 — Pulido y distribución
Temas, teclado, accesibilidad, restauración de sesión, optimización medida, paquetes, CLI instalable, documentación de contribución, licencia y avisos de dependencias. CI completa, scripts de empaquetado y checklist de release. Preparar release sin publicarla hasta que el propietario lo solicite.

Salida: pruebas reales en ambos sistemas, paquetes instalables, métricas finales y límites públicos. Si falta un OS, firma o credencial, marcarlo pendiente; no simular validación.

## Después del MVP
Evaluar LSP/autocompletado, terminal, búsqueda global, Git, multipart y otras funciones a partir de uso real. No introducir estas dependencias durante H0–H3.
