# Criterios de aceptación y medición

## Presupuesto de rendimiento propuesto
Son objetivos por validar, no resultados ni promesas. Registrar CPU, RAM, GPU, OS, versión Rust, commit, modo release y corpus. Separar benchmarks del motor de latencia visual; una medición de función no demuestra latencia tecla→pantalla.

| Escenario | Objetivo inicial |
|---|---|
| Arranque en caliente, ventana vacía interactiva | p95 < 500 ms, 20 ejecuciones |
| Edición en archivo de 1 MiB con resaltado | p95 tecla→frame < 16,7 ms si puede medirse correctamente |
| Procesamiento de una operación local de edición | p95 < 2 ms en corpus de 1 MiB |
| Reposo sin tareas, ventana enfocada | CPU media < 1% de un núcleo durante 60 s; reportar normalización |
| Memoria residente, ventana vacía estabilizada | < 150 MiB; detallar métrica por OS |
| Archivo de 10 MiB o respuesta HTTP grande | UI utilizable, sin crecimiento de memoria ilimitado |

Registrar arranque en frío por separado sin fingir vaciar cachés. Si no hay instrumentación de tecla→frame, marcar pendiente y medir coste de frame y motor como proxies identificados. Ante objetivos incumplidos, explicar perfiles y corregir cuellos de botella antes de añadir features. Ajustar presupuestos con justificación y revisión del propietario, no para ocultar regresiones.

Corpus determinista: archivos UTF-8 de 1 KiB, 1 MiB, 10 MiB y estrés de 100 MiB; línea única de 1 MiB; mezcla de ASCII, acentos, emoji y caracteres combinados; LF y CRLF. Registrar límites del modo de archivo grande. No exigir que resaltado completo de 100 MiB sea parte del MVP.

## Editor
- Round-trip abrir/guardar conserva bytes cuando no hay cambios, incluidos BOM y CRLF.
- Undo/redo de inserciones, borrados, pegado, reemplazo y Unicode restaura contenido y estado coherentes.
- Cerrar documento modificado: guardar, descartar y cancelar funcionan; cancelación deja documento abierto.
- Error por permisos/disco lleno/archivo bloqueado conserva original y buffer; temporales no sustituyen un archivo válido por uno parcial.
- Archivo modificado externamente no se sobreescribe sin informar. Restauración tras crash no sobrescribe original automáticamente.
- Búsqueda/reemplazo no rompe índices UTF-8; probar selección multilínea y extremos del documento.
- Escritura con acentos y composición IME revisada manualmente en cada OS; portapapeles, foco y escalado comprobados.
- Escritura y scroll en corpus grande no disparan copias/parsing completos por tecla.

## HTTP — servidor local determinista
Endpoints de prueba: eco de método/headers/body, respuesta JSON, texto, cuerpo vacío, binario, 400/500, retraso, redirect, stream grande y conexión interrumpida. TLS con fixture local y certificado de prueba explícitamente confiado, sin desactivar validación global.

- GET con query repetida y POST JSON producen los bytes/headers esperados.
- Basic/Bearer se aplican a la petición elegida y no aparecen en logs/export por defecto.
- Variables resuelven precedencia; las desconocidas bloquean envío; los secretos de sesión no se escriben en disco.
- Cancelar libera tarea/lectura y deja UI usable; timeout distinguible de cancelación.
- Respuesta tardía de envío anterior no reemplaza a la actual.
- 4xx/5xx muestran cuerpo y status; no se transforman en error genérico de red.
- Límite de preview se aplica a datos descomprimidos; se muestra truncación y consumo acotado.
- Redirect a otro origen no reenvía Authorization; máximo de saltos se cumple.
- Importar cURL no ejecuta shell; probar comillas, caracteres especiales y opciones no admitidas.
- Colecciones exportadas/importadas conservan estructura; versión incompatible produce error claro sin pérdida.

## Plataformas y release
CI ejecuta formato, lint, pruebas y build en macOS/Windows con versiones identificadas. Pruebas de CLI con rutas relativas, espacios y Unicode. Revisar apertura desde Finder/Explorer, cerrar/reabrir, instalación opt-in de CLI y conflictos con un `se` existente. No anunciar compatibilidad con arquitecturas no compiladas/probadas.

## Informe mínimo por hito
Comando → resultado → entorno. Para cada requisito: PASS, FAIL o NOT RUN con explicación. Adjuntar métricas reales y qué se midió. Checklist manual separada de tests automáticos. El código implementado sin probar se declara implementado, no validado.
