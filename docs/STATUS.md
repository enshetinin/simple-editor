# Estado de SimpleEditor

Actualizado: 2026-10-15 — cierre de **H0** (viabilidad y aplicación vertical).
Base: commit `d0462fb` + cambios **sin commit** en el árbol de trabajo (no se ha hecho commit ni push).

## Resumen
- H0: **implementado y validado en macOS (Apple Silicon)**. Windows: **NOT RUN** (sin acceso; CI preparada, nunca ejecutada).
- H1, H2, H3: pendientes.
- La vista del editor es el `TextEdit` estándar de egui: **prototipo**, no el motor definitivo. Las mediciones de abajo justifican sustituirla en H1.

## Entorno de las mediciones
macOS 27.0.1 (26A434), Apple M3, 8 GB RAM, arm64. rustc 1.99.0 / cargo 1.99.0 (estable). Perfil release salvo indicación. Pantalla del propio equipo (sin control de escala).

## Qué funciona (H0)
- Proyecto Rust de un crate: `src/editor` (documento, codificación, guardado), `src/http` (validación, cliente, servicio cancelable), `src/workspace.rs` (árbol bajo demanda + apertura en segundo plano), `src/cli.rs`, `src/app` (UI egui). Binarios `se` y `simpleeditor`.
- Dependencias (versiones estables verificadas en crates.io el 2026-10-15): eframe/egui 0.36.2 (wgpu, accesskit), reqwest 0.13.5 (TLS por defecto: rustls + verificador de plataforma = almacén de certificados del sistema), tokio 1.53.3, rfd 0.17.2, tempfile 3.27.0, url 2.5.8. Dev: ropey 1.6.1 (solo para la medición). `Cargo.lock` incluido.
- Ventana: barra superior (proyecto, Open folder/file, New file, New HTTP request, Save, tema System/Light/Dark), sidebar redimensionable con secciones FILES y HTTP, pestañas con punto de cambios (y texto accesible «modified»), barra de estado (lenguaje, Ln/Col, codificación, final de línea). Tokens de color de UX.md para claro/oscuro, radio 6 px, espaciado 4/8/12.
- Archivos: abrir (diálogo, árbol o CLI; lectura y decodificación en hilo secundario), editar, guardar / guardar como, indicador dirty, cierre de pestaña con Save / Don't save / Cancel, cierre de ventana interceptado con Save all / Discard all / Cancel. Detección de modificación externa antes de guardar (Overwrite / Save as / Cancel).
- Formato: UTF-8 con o sin BOM; LF, CRLF y mixto se conservan (mixto se guarda tal cual, sin conversión). Binarios, UTF-8 inválido y >2 MiB se rechazan con mensaje explícito. Aviso visible si hay líneas >64 KiB.
- Guardado: temporal en el mismo directorio → `fsync` → `rename` atómico (`MoveFileExW` con reemplazo en Windows vía tempfile); conserva permisos; un symlink sigue siendo symlink y se reemplaza su destino; un fallo no toca el original ni el buffer.
- HTTP: GET/POST/PUT/PATCH/DELETE/HEAD/OPTIONS, URL validada (solo http/https, host obligatorio; nada se interpreta como shell), cuerpo opcional JSON/Text con Content-Type, Send/Cancel (y Cmd/Ctrl+Enter solo con la pestaña HTTP activa), estado, tiempo, bytes, versión, URL final, cuerpo raw y cabeceras. Timeout 30 s, máx. 10 redirecciones, preview limitado a 5 MiB descomprimidos con lectura abortada y etiqueta «Truncated». Errores diferenciados: Timeout, DNS, TLS, conexión, cuerpo interrumpido, redirección. 4xx/5xx son respuestas. Runtime Tokio único (2 hilos), cancelación con `abort` (libera la conexión), resultados con id: un resultado antiguo nunca sustituye al actual.
- La respuesta se renderiza virtualizada (solo líneas visibles; líneas >2000 caracteres recortadas con aviso y botón Copy body).
- CLI: `se .`, `se archivo…`, rutas relativas al directorio de invocación, espacios/Unicode, archivo inexistente → pestaña nueva que se crea al guardar, `--help`, `--version`, opción desconocida → código 2.

## Comandos ejecutados y resultados (macOS, este equipo)
| Comando | Resultado |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS (0 avisos) |
| `cargo test --locked --all-targets --all-features` | PASS, 27 pruebas, 3 ejecuciones consecutivas sin fallos |
| `cargo build --locked --release --bins` | PASS (`se` ≈ 22 MB con tablas de línea de depuración) |
| `se --version`, `simpleeditor --help`, `se --nope` | Salida correcta; `--nope` → exit 2 |
| `cargo run --release --example editor_probe` | Ver métricas |
| Ventana real (`se .`) | Se abre y cierra (arranque medido); interacción manual: ver checklist |

Pruebas automáticas (qué cubren):
- `src/` (7): round-trip LF/CRLF/mixto/BOM/Unicode, normalización CRLF al pegar, binario/UTF-8 inválido, Ln/Col en caracteres, validación de URL (incluye metacaracteres de shell), CLI (`.`, rutas relativas con espacios/ñ/emoji, opciones, errores).
- `tests/save.rs` (7 en Unix; 6 en Windows): bytes idénticos sin cambios, CRLF+BOM tras editar, sin temporales residuales, conflicto externo, fallo de guardado (directorio de solo lectura) conserva original y buffer, permisos y symlink, untitled → save as, rechazo de binario/latin-1/>2 MiB/directorio. Test específico de reemplazo en Windows (no ejecutado).
- `tests/http.rs` (10) contra servidor local propio (`tests/support`): POST JSON con bytes y Content-Type exactos, los 7 métodos, 500/204/binario, redirect con URL final, truncado a 5 MiB, cancelación (el servidor observa el cierre de la conexión; no llega resultado; el servicio sigue usable), timeout ≠ cancelación, conexión rechazada vs cuerpo interrumpido, DNS (`.invalid`, usa el resolvedor del sistema), resultado tardío vs nuevo.
- `tests/ui_flow.rs` (3): la UI real headless (egui sin ventana): abrir por CLI → teclear «café 👋» mediante eventos de egui en el `TextEdit` → Cmd/Ctrl+S → archivo guardado con CRLF intacto → pestaña HTTP → respuesta 200 del servidor local; cerrar con cambios abre diálogo y cancelar conserva la pestaña; cierre de ventana interceptado; cancelación ignora resultados tardíos; URL inválida bloquea y nombra el campo.

## Métricas reales
**Arranque en caliente** (`SE_STARTUP_PROBE=1 se`, desde `main()` hasta construir el segundo frame; no incluye carga del ejecutable ni presentación en pantalla): 20 ejecuciones, la 1.ª tras compilar 931,9 ms; las 19 restantes p50 115,7 ms, p95 125,5 ms, máx. 131,4 ms. p95 de las 20: 131,4 ms. Objetivo p95 < 500 ms: PASS (con la definición indicada). Arranque en frío: NOT RUN.

**Reposo** (ventana `se .` en primer plano, sin interacción, 60 s tras 8 s de espera, `ps -o time`): 4 mediciones: 0,017 %, 0,000 %, 0,00 % (9 ventanas de 10 s) y una atípica de 6,25 % no reproducida; un `sample` de 10 s en otra ejecución mostró todos los hilos bloqueados en espera. Causa de la atípica no confirmada (posible actividad de ratón sobre la ventana). Objetivo < 1 %: PASS en 3 de 4; repetir en H1.

**Memoria** (ventana vacía con proyecto, estabilizada): RSS 112,7–126,1 MiB; `footprint` (phys_footprint de macOS) 71–169 MB según ejecución. Objetivo < 150 MiB: PASS por RSS; footprint variable, revisar en H1.

**Vista `TextEdit` de egui** (headless: layout + teselado en CPU, sin GPU; proxy, **no** latencia tecla→pantalla; 30 frames; ms):
| Corpus | 1.er frame | Reposo p50/p95 | Tecleo p50/p95/máx |
|---|---|---|---|
| código 1 KiB | 3,00 | 0,02/0,02 | 0,03/0,11/0,12 |
| código 100 KiB | 2,40 | 0,07/0,08 | 0,64/0,77/0,84 |
| código 1 MiB | 5,93 | 0,45/0,46 | 5,14/5,33/5,65 |
| Unicode mixto 1 MiB | 4,02 | 0,31/0,32 | 3,96/4,22/4,23 |
| línea única 1 MiB | 80,31 | 0,29/0,30 | 74,44/95,61/104,41 |
| código 4 MiB | 17,70 | 1,72/1,77 | 20,18/20,61/21,10 |
| código 8 MiB | 33,66 | 3,61/3,76 | 41,68/42,06/42,47 |
| código 10 MiB | 41,52 | 4,58/4,81 | 54,03/55,02/55,16 |

**Buffer: insertar un carácter en el centro** (2000 ops, p50/p95): String 1 MiB 8/8 µs, 10 MiB 87/91 µs, 100 MiB 1107/1310 µs; `ropey::Rope` ≈ 0,083/0,084–0,125 µs en los tres tamaños.

## Decisiones técnicas
- **egui/eframe se mantiene** como UI: ventana, paneles, diálogos, foco, portapapeles y reposo sin repintado continuo funcionan; no hay evidencia para cambiar de stack.
- **El widget `TextEdit` no sirve como editor definitivo**: rehace el layout del documento completo en cada pulsación (coste lineal: ~5 ms/MiB, 54 ms en 10 MiB) y una línea de 1 MiB cuesta 75–105 ms por tecla. Además exige `String` contiguo, lo que impide un rope. Solución H0: límite editable de **2 MiB** (≈10 ms/tecla por interpolación entre 1 y 4 MiB) y aviso de líneas largas, ambos explícitos.
- **Propuesta para H1**: buffer `ropey` (inserción O(log n), medida) + widget de editor propio sobre egui que haga layout solo de líneas visibles con caché por línea/revisión, con índices separados de bytes/caracteres/graphemes. Alternativa si el IME/selección propios no son viables en egui: evaluar otro toolkit nativo con evidencia antes de cambiar.
- Guardado síncrono en el hilo de UI (archivos ≤2 MiB); mover a worker en H1 con la recuperación.
- Nuevo documento usa LF. HEAD/GET pueden llevar cuerpo si el usuario lo escribe.
- Windows: binario de subsistema consola (para que `--help` funcione); al abrir desde el Explorador aparecerá una consola. Resolver en H3.

## Limitaciones conocidas (H0)
- Sin resaltado, números de línea, buscar/reemplazar, recuperación, sesión, detección de indentación ni undo transaccional propio (se usa el undo interno del `TextEdit`: no agrupa por revisión del documento y se pierde al cerrar).
- Archivos >2 MiB no se abren. Mixed line endings: `\r` visibles tal como el widget los dibuje.
- Ln/Col se recalcula recorriendo el texto hasta el cursor (solo cuando cambia).
- HTTP: sin params/headers/auth/variables/colecciones/historial (H2). Borradores HTTP no se persisten y se cierran sin confirmación.
- Diálogos de archivo (rfd) son modales y síncronos.
- Licencias para distribución (H3): `epaint_default_fonts` (OFL-1.1 + Ubuntu Font Licence) y `webpki-root-certs` (CDLA-Permissive-2.0) requieren avisos; el resto MIT/Apache/BSD/ISC/Zlib o con alternativa permisiva.

## Matriz H0
| Requisito | Estado |
|---|---|
| Proyecto Rust y estructura modular | PASS |
| Ventana con sidebar y pestañas | PASS (headless + apertura real); revisión visual NOT RUN |
| Abrir/editar/guardar UTF-8, dirty, cierre seguro | PASS (automático) |
| HTTP método/URL/cuerpo, Send/Cancel, status, respuesta raw, servidor local | PASS (automático) |
| CLI `se .` y apertura de archivo | PASS (parser automático; apertura real con `se .`) |
| README con requisitos y comandos reales | PASS |
| CI macOS + Windows | Preparada (`.github/workflows/ci.yml`); NOT RUN (sin push) |
| Prototipo archivo grande / Unicode / reposo | PASS (mediciones arriba); IME NOT RUN |
| Decisión de buffer/renderizado documentada | PASS |
| Windows | NOT RUN |

## Checklist manual pendiente (propietario)
No he podido interactuar con la ventana ni capturarla (el terminal no tiene permiso de grabación de pantalla). Pendiente de revisión humana en macOS y Windows:
1. `cargo build --release && ./target/release/se .` → la sidebar lista la carpeta; expandir carpetas; abrir un `.rs`.
2. Escribir con acentos (´ + e), `ñ`, emoji (Ctrl+Cmd+Espacio) y **composición IME** (p. ej. japonés/chino) → texto correcto, cursor en su sitio.
3. Selección con ratón y Shift+flechas; copiar/pegar con otra app; Cmd/Ctrl+Z / Shift+Cmd+Z.
4. Cmd/Ctrl+S; editar el archivo desde otra app y volver a guardar → diálogo de conflicto.
5. Modificar y cerrar la pestaña (×, Cmd/Ctrl+W) y la ventana → Save / Don't save / Cancel.
6. Terminal aparte: `python3 -m http.server 8000`; en la app *New HTTP request* → `http://127.0.0.1:8000/` → Send → 200 y HTML; cambiar a Headers.
7. URL `https://httpbin.org/delay/10` (o un endpoint lento propio) → Send → seguir editando código → Cancel.
8. Tema claro/oscuro (botón Theme) y contraste; escala 100/150/200 %; ventana 960×640.
9. VoiceOver/Narrator: nombres de botones y pestañas.
10. Windows: los pasos anteriores con `target\release\se.exe .`, guardado en carpeta con archivo abierto por otro programa.

## Cómo ejecutar
```sh
cargo test --all-targets --all-features
cargo run --release -- .          # equivale a: target/release/se .
cargo run --release --example editor_probe -- --10mib
SE_STARTUP_PROBE=1 ./target/release/se   # imprime el tiempo hasta el 2.º frame y cierra
```
Para la CI: hacer commit y push de la rama (no lo he hecho); el workflow corre en `macos-latest` y `windows-latest`. Riesgo a vigilar en Windows: compilación de `aws-lc-sys` (dependencia TLS de rustls).

## Siguiente paso: H1 — editor fiable
1. Ejecutar la CI y el checklist manual; corregir lo que falle en Windows.
2. Buffer `ropey` en `Document` con transacciones de undo/redo agrupadas y revisión.
3. Widget de editor propio: layout por líneas visibles con caché, cursor/selección por graphemes, IME (`Event::Ime`), portapapeles, números de línea, indentación.
4. Tree-sitter incremental (Rust, JS, TS, JSON, HTML, CSS) en worker con DocumentId+revisión.
5. Buscar/reemplazar, recuperación de buffers, conflictos externos con recarga, modo archivo grande explícito (objetivo: 10 MiB usable).
6. Re-medir con el corpus de ACCEPTANCE (incluido 100 MiB de estrés) y repetir reposo/memoria.

## Registro que debe mantener Claude
Para cada hito: fecha/commit, cambios, comandos y resultados, métricas con entorno, decisiones técnicas, limitaciones y siguiente paso. No marcar como completas tareas futuras.
