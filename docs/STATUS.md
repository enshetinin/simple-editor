# Estado de SimpleEditor

Actualizado: 2026-10-15 — cierre de **H3** (pulido y distribución), **sin publicar**.
Base: `91a521a` (H2) + cambios de H3 **sin commit**. `develop` va 2 commits por delante de `origin`: H1, H2 y H3 **no se han probado en Windows**, porque la CI solo corre tras el push. No he hecho commit, push, tag ni release.

## H3 — resumen
| Área | Estado |
|---|---|
| CLI instalable (`se --install-cli`, `--uninstall-cli`, `--name`, `--dir`) | PASS (unitarias + binario real: instala, detecta la reinstalación, no toca un comando ajeno y desinstala solo lo suyo) |
| Windows: `simpleeditor.exe` gráfico sin consola, `se.exe` CLI | Implementado; NOT RUN (sin Windows) |
| Restauración de sesión (archivos + cursor, peticiones, borradores saneados, pestaña activa) y tema persistente | PASS (`tests/session.rs`, dos arranques de la app sin ventana) |
| Arrastrar y soltar archivos o carpetas sobre la ventana | Implementado; NOT RUN (manual) |
| Accesibilidad: el editor expone la línea actual y la posición a AccessKit | PASS (prueba con AccessKit activado). VoiceOver/Narrador reales: NOT RUN |
| Teclado: Ctrl+Tab / Ctrl+Shift+Tab | PASS (prueba) |
| Layout a 960×640 al 100/150/200 % (editor, entornos, 6 sub-pestañas HTTP) | PASS (prueba: nada se sale de la ventana). Corregido un desbordamiento de 5 px en la fila de la URL |
| Icono propio (`scripts/make_icon.py`, sin dependencias): PNG, ICO, icns y icono de ventana | Hecho, revisado visualmente |
| Paquete macOS (`scripts/package-macos.sh`) | PASS en local: `.app` arm64, macOS ≥ 11.0 (leído del binario), zip, DMG y sha256 verificados; firma ad-hoc válida; arranca por LaunchServices. **Gatekeeper: rejected** (falta Developer ID/notarización, credenciales tuyas) |
| Paquete Windows (`scripts/package-windows.ps1`, zip portable) | NOT RUN en local; lo ejecuta el job `package` de la CI (artefactos de 14 días, sin publicar) |
| Licencias de terceros (`THIRD_PARTY_LICENSES.md`, 318 crates + fuentes embebidas) | PASS (`--check` en la CI de macOS). Hay 41 crates sin archivo de licencia en su paquete; se usan bajo una licencia de su expresión SPDX y su texto canónico está en `scripts/license-texts/` |
| CONTRIBUTING, SECURITY, CHANGELOG, `docs/RELEASE.md` | Hecho. El canal de seguridad y el código de conducta quedan pendientes de tu decisión |
| Optimización medida | Av Pág en una línea de 1 MiB: 3,4 ms → 0,01 ms (columna visual sin recorrido en líneas largas) |

## Comandos ejecutados (macOS, Apple M3, rustc 1.99.0)
| Comando | Resultado |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --locked --all-targets --all-features` | PASS: **106 pruebas**, 3 ejecuciones seguidas (1 ignorada) |
| `cargo test --test save -- --ignored` | PASS (disco lleno real) |
| `python3 scripts/third_party_licenses.py --check` | PASS |
| `./scripts/package-macos.sh` | PASS (ver arriba); `shasum -c` OK. `hdiutil` avisa de que está obsoleto en macOS 27, pero funciona |
| `spctl --assess` sobre el `.app` | rejected (esperado sin firma Developer ID ni notarización) |

## Métricas finales (release)
- **Editor** (`editor_probe --100mib`): tecleo p95 0,09–0,14 ms en todos los corpus, incluido 100 MiB; Av Pág p95 ≤ 0,19 ms (también la línea única de 1 MiB tras la optimización); operaciones del motor ≈ 1 µs; reparseo incremental p50 1,83 ms (1 MiB) y 27,5 ms (8 MiB) en el worker; abrir/guardar 100 MiB en 126/91 ms en worker; JSON de 5 MiB procesado en ≈ 26 ms en worker.
- **Arranque en caliente** (`se .`, 19 ejecuciones tras la primera, que tardó 680 ms): p50 94,5 ms, p95 118,9 ms, máx. 119,4 ms. Objetivo < 500 ms: PASS.
- **Reposo**:
  - **0,00 % sin eventos de entrada** (4 ventanas seguidas de 10 s con 0 frames);
  - con actividad del ratón sobre la ventana, la CPU sigue a los frames dibujados (79 frames ≈ 5,1 % en 10 s);
  - en dos mediciones de 60 s con el equipo en uso salió 3,2 % y 1,7 %; al registrar los eventos, los frames eran `PointerMoved`/`MouseMoved`/clics/foco, no repintado propio.

  Conclusión: no hay repintado continuo. Repite la medida sin tocar el ratón para la cifra oficial (checklist).
- **Memoria** (`se .`): RSS 89,0 MiB, footprint 61 MB; con un archivo abierto y resaltado: RSS 106,7 MiB. Objetivo < 150 MiB: PASS.

## Decisiones de H3
- `se --install-cli` crea un symlink (macOS) o un shim `.cmd` (Windows) en una carpeta de usuario. No modifica el PATH: solo imprime cómo añadirla. Nunca reemplaza un archivo que no creó y ofrece `simpleeditor` como nombre alternativo.
- La sesión se guarda en `state.json` (datos de usuario) al cerrar limpiamente, con una escritura síncrona serializada con las demás. Los borradores HTTP pasan por `strip_secrets`. Las escrituras del historial esperan a su carga para no machacarlo (riesgo detectado y corregido en H3).
- Sin MSI ni icono de recurso en el .exe de Windows (habría que añadir `embed-resource`, que no puedo probar aquí); el zip portable es el artefacto de H3.
- El identificador de bundle por defecto `io.github.enshetinin.simpleeditor` se deriva del repo y se puede cambiar con `SE_BUNDLE_ID`; es una decisión tuya (`docs/RELEASE.md`).

## Checklist manual de H3 (además de los de H1/H2 más abajo)
1. Abre el DMG `dist/SimpleEditor-0.1.0-macos-arm64.dmg` y arrastra la app a Aplicaciones. Ábrela: Gatekeeper avisará (clic derecho → Abrir).
2. `/Applications/SimpleEditor.app/Contents/MacOS/se --install-cli`; añade `~/.local/bin` al PATH y prueba `se .` desde la terminal.
3. Abre archivos y peticiones y cierra la app; `se .` de nuevo: deben volver los archivos con su cursor y la pestaña activa.
4. Arrastra un archivo y una carpeta desde Finder a la ventana.
5. VoiceOver (Cmd+F5): enfoca el editor y muévete con las flechas; debe leer la línea y la posición.
6. Ctrl+Tab entre pestañas; ventana a 960×640; escalado en Ajustes > Pantallas.
7. Reposo: `se .`, sin tocar el ratón 60 s (Monitor de Actividad ≈ 0 %).
8. Windows: zip de la CI → `simpleeditor.exe` desde el Explorador (sin consola), `se.exe --install-cli`, Narrador, 150/200 %.

## Siguiente paso
Push de `develop` → revisar la CI (Windows y el job `package`) → checklists manuales → decisiones de `docs/RELEASE.md` → release solo cuando lo pidas.

---

Actualizado: 2026-10-15 — cierre de **H2** (HTTP de uso diario).
Base: commit local `dbc9938` (H1) + cambios de H2 **sin commit**. Tu rama `develop` va 1 commit por delante de `origin` (H1 no está pusheado). No he hecho commit ni push.

## Resumen
- **H0**: CI en verde en macOS y Windows (runs `38705227999`, `38705289540`; el merge en `main`, `38710953013`, estaba en curso al consultarlo).
- **H1**: validado en macOS. En Windows **NOT RUN**: el commit `dbc9938` aún no está en GitHub, así que la CI no lo ha probado.
- **H2**: implementado y validado en macOS con pruebas automáticas contra un servidor local (HTTP y HTTPS) y con la app real sin ventana. Windows: **NOT RUN**. El Keychain real: **NOT RUN** (las pruebas usan un almacén en memoria; ver checklist). Revisión visual: **pendiente**.
- H3: pendiente.

## Fallos corregidos
### 2026-10-15 — El panel lateral crece sin límite
- **Informe**: al ejecutar la app, el panel lateral crecía hasta ocupar toda la ventana.
- **Reproducción** (`tests/sidebar_width.rs`, la app real sin ventana, 120 frames midiendo `PanelState` de `sidebar`): con un proyecto abierto, el ancho pasaba de 285 a 835 px (+5 px por frame). Con nombres largos (colección, entorno, historial, petición) llegaba a los 1200 px de la ventana.
- **Causa**: bucles de realimentación de tamaño en un panel que se ajusta a su contenido.
  1. El selector de entorno pedía `available_width() - 32`, pero el combo más el botón ✎ ocupaban algo más; el panel se ensanchaba y en el frame siguiente se pedía todavía más.
  2. Etiquetas y cabeceras con texto largo usaban el modo Extend y ensanchaban el panel.
  3. Con texto truncado, la etiqueta o el combo ocupaban todo el resto de la fila y el botón posterior («⋯» o ✎) se salía, con el mismo efecto.
- **Corrección** (`src/app/view.rs`):
  - el texto de toda la barra lateral se trunca con «…» (egui muestra el texto completo al pasar el ratón);
  - en las filas con botón (cabecera de colección, selector de entorno), el botón se coloca primero (de derecha a izquierda) y el texto truncado ocupa solo lo que queda;
  - el combo tiene un ancho mínimo fijo.

  El panel solo cambia de ancho cuando el usuario arrastra su borde.
- **Prueba de regresión**: `sidebar_width_is_stable` combina el panel vacío, colección, entorno, historial y petición sin guardar con nombres de 300 caracteres, y nombres de archivo/carpeta de 200 caracteres: el ancho se mantiene en 240 px en todos los casos. Antes de la corrección fallaba (285→835 y 1200 px).
- **Verificación**: `fmt --check`, `clippy -D warnings`, `cargo test` (102 pruebas, incluido el flujo editar→guardar→petición HTTP de `ui_flow` y `http_app`) y build release: PASS. No he podido revisarlo en una ventana real (sin permiso de captura); confírmalo con `./target/release/se .`.

## Entorno
macOS 27.0.1, Apple M3, 8 GB, rustc 1.99.0, perfil release para las métricas.

## Qué hay nuevo en H2
**Modelo y resolución (`src/http`, sin UI)**
- `RequestDefinition` persistible: método, URL, query params y headers con activar/desactivar y claves repetidas (en orden), body None/Text/JSON, auth None/Bearer/Basic, variables propias de la petición, timeout (30 s por defecto) e `id` estable dentro de la colección.
- Variables `{{name}}` (nombres `[A-Za-z0-9_.-]`) en URL, params, headers, body y auth.
  - Precedencia: **petición > entorno activo > variables de proyecto**.
  - Los valores se insertan literalmente, sin expansión recursiva.
  - Una variable desconocida o un secreto sin valor en la sesión **bloquean el envío** y el error nombra el campo («Headers › X-Token: {{token}}: unknown variable»).
- `resolve()` valida todo antes de enviar:
  - URL http/https con host;
  - nombres y valores de header;
  - JSON del body en modo JSON;
  - conflicto entre Auth y un header `Authorization`;
  - usuario Basic con `:`;
  - timeout ≥ 1.

  Añade el `Content-Type` del modo de body salvo que el usuario ya lo haya puesto. Basic usa un base64 propio, probado.
- Cliente (`client.rs`):
  - headers en orden y timeout por petición;
  - raíces TLS adicionales con `tls_certs_merge`, que añade y **nunca desactiva** la verificación;
  - gzip/deflate/brotli (features de reqwest);
  - preview de 5 MiB **sobre bytes descomprimidos**: deja de leer y lo marca como truncado.

  La validación y el formateo de JSON y la división en líneas se hacen en `spawn_blocking`, nunca en el hilo de UI. Los errores de transporte tienen el texto de los secretos redactado.
- Descarga a archivo («Send and save response to file…»): streaming a un temporal en la misma carpeta, renombrado solo al completar y límite configurable (1 GiB por defecto). Si se supera el límite, se cancela o falla la escritura (disco lleno), no queda archivo parcial.
- Redirecciones: máximo 10 y se muestra la URL final. reqwest (verificado en su código, `remove_sensitive_headers`) quita `Authorization`, `Cookie`, `Proxy-Authorization` y `WWW-Authenticate` al cambiar de host, puerto o esquema. 301/302/303 pasan POST a GET sin body; 307/308 conservan método y body (probado con 303 y 307).
- JSON pretty propio (`json.rs`): reindenta tokens, así que **conserva el orden de claves, la escritura de los números y los escapes**. Se valida antes con `serde_json` (`IgnoredAny`).
- cURL (`curl.rs`): tokenizador POSIX explícito y exportación POSIX. El subconjunto exacto está en **[docs/CURL.md](CURL.md)**: nunca se ejecuta nada, no se expande `$VAR`/`$(…)`, los operadores de shell son error y las opciones no soportadas generan una nota.
- Saneado (`sanitize.rs`): criterio único de qué es secreto, usado al guardar en el proyecto, al exportar y en el historial.
  - Son secretos: variables marcadas *secret*; valores literales de Auth; valores literales de headers sensibles (Authorization, Cookie, X-Api-Key… o nombres con token/secret/password/key/auth/session…) y de params sensibles; contraseña en la URL (`user:pw@`) y query params sensibles dentro de la URL.
  - Los valores que son solo `{{variables}}` no son secretos.
- Secretos (`secrets.rs`): por defecto solo en memoria durante la sesión. Con el opt-in **Remember** se guardan en el almacén del sistema mediante `keyring-core 1.0.1` + `apple-native-keyring-store 1.0.2` (macOS) y `windows-native-keyring-store 1.1.0` (Windows), todos MIT/Apache-2.0. Si el almacén falla, el secreto queda solo en memoria, se avisa y se desmarca Remember: **nunca hay fallback a texto plano**. `SE_SECRET_BACKEND=memory` fuerza el modo memoria.

**Persistencia (`src/storage`)**
- Proyecto, en `.simpleeditor/` (JSON legible con `schema_version: 1`, terminado en salto de línea, escrito de forma atómica):
  - `collections/<id>.json`, `environments/<id>.json` y `variables.json`;
  - **no se crea nada hasta que el usuario guarda**;
  - los valores secretos y las credenciales literales nunca se escriben ahí;
  - un archivo con `schema_version` desconocido o JSON inválido se informa y **no se toca**; un id nuevo nunca reutiliza el nombre de un archivo existente.
- Importar colección: máximo 10 MiB, verificación de esquema, nombre único («API (2)»); nunca sobrescribe. Exportar: secretos fuera por defecto, o «Export including secrets…» explícito.
- Datos de usuario (`~/Library/Application Support/SimpleEditor`, `%LOCALAPPDATA%\SimpleEditor` o `$SE_DATA_DIR`):
  - `history.json`: **100 entradas**, sin bodies, sin valores de auth y sin secretos literales en URL, params ni headers;
  - `state.json`: entorno activo por proyecto;
  - `recovery/`: recuperación, como en H1.

  Las escrituras van en un hilo, serializadas y por generaciones, para que una instantánea vieja no pise a una nueva.

**UI**
- Pestaña HTTP:
  - nombre y colección, selector de entorno, Save (Cmd/Ctrl+S), Copy as cURL;
  - método + URL + Send/Cancel (Cmd/Ctrl+Enter) con el menú ▾ para enviar a archivo;
  - sub-pestañas Params / Headers / Body (con «Format JSON») / Auth (campos enmascarados salvo `{{variables}}`) / Variables / Settings;
  - errores por campo;
  - respuesta con estado, tiempo, tamaño, versión, URL final, Pretty/Raw, Headers y avisos de JSON inválido, truncado, binario o vacío, con vista virtualizada.
- Barra lateral HTTP:
  - entorno activo con editor ✎;
  - colecciones plegables, con menú ⋯ (Export, Export including secrets, Delete con confirmación) y borrado de una petición desde el menú contextual, también con confirmación;
  - peticiones sin guardar;
  - historial (los 100, se abren como petición nueva) y «Clear history»;
  - menú ⋯ con Import cURL e Import collection.
- Editor de entornos (pestaña): variables de proyecto y entornos (añadir, renombrar, borrar), columnas Name/Value/Secret/Remember, valores secretos enmascarados y Save explícito.
- Integridad: una petición sin guardar con contenido, una petición modificada de una colección o un editor de entornos con cambios cuentan como trabajo sin guardar al cerrar la pestaña o la ventana («Save all» los guarda todos).

## Comandos ejecutados y resultados (macOS)
| Comando | Resultado |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS |
| `cargo test --locked --all-targets --all-features` | PASS: **101 pruebas**, 3 ejecuciones seguidas sin fallos (1 ignorada: disco lleno). Tras la corrección del panel lateral: 102 |
| `cargo build --locked --release --bins` | PASS (`se` ≈ 29,8 MiB con tablas de línea) |
| `cargo run --release --example editor_probe` | Ver métricas |
| Escaneo de licencias del árbol | Sin MPL/GPL/LGPL obligatorias |

Pruebas nuevas de H2:
- Unitarias (`src/http`, `src/storage`):
  - plantillas `{{}}` (incluido `{{{x}}}`), precedencia de variables, secreto sin valor, sin recursión;
  - resolución: params y headers repetidos con codificación, Basic/Bearer, `Content-Type`, errores por campo en orden;
  - base64 de ida y vuelta;
  - JSON pretty con orden, números y escapes;
  - cURL: comillas, `$'…'`, continuaciones, operadores rechazados, sin expansión, Chrome «Copy as cURL», `-sSL -XPUT`, `-G` + `--data-urlencode`, `-I`, `-u`, `--json`, opciones no soportadas con nota, export→import de ida y vuelta, secretos `<omitted>`;
  - saneado: URL, params, headers, auth y variables, comprobado sobre el JSON serializado;
  - proyecto: no crea nada al cargar, round-trip sin secretos, esquema futuro/JSON roto informados e intactos, id sin colisión, import sin sobrescribir;
  - historial acotado y sin secretos; estado persistente.
- `tests/http.rs` (16, servidor local):
  - POST JSON exacto;
  - query y headers repetidos con variables y Bearer tal como llegan al servidor;
  - Basic exacto con UTF-8;
  - los 7 métodos;
  - 500/204/binario;
  - JSON pretty y JSON inválido (el raw se sigue mostrando);
  - URL final, 303→GET y 307 que conserva POST;
  - **Authorization y Cookie no llegan a otro origen** (otro puerto) y sí al mismo origen;
  - más de 10 redirecciones → error, 10 → OK;
  - **límite de 5 MiB descomprimido** con un cuerpo gzip de 6 MiB;
  - descarga: completa, límite superado y cancelación, sin archivos parciales;
  - **TLS**: certificado autofirmado rechazado por defecto y aceptado al confiar en él explícitamente;
  - cancelación, timeout por petición, conexión rechazada, cuerpo cortado, DNS, resultado tardío.
- `tests/http_app.rs` (5, la app real sin ventana):
  - colección + entorno activo + historial **persisten tras reiniciar la app**, sin el token literal en ningún archivo;
  - secretos: un secreto «Remember» sobrevive al reinicio (almacén en memoria compartido) y uno solo de sesión no; ninguno aparece en `variables.json` ni en `history.json`; tras reiniciar, el de sesión bloquea el envío nombrando «Headers › X-B»;
  - importar cURL y enviarlo;
  - una petición sin guardar pregunta al cerrar y `.simpleeditor/` no se crea;
  - todas las vistas, sub-pestañas y diálogos se dibujan sin pánico.

## Métricas (release, este equipo)
- **Procesado de respuesta en el worker**: JSON de 5,0 MiB (8,3 MiB formateado, 635 122 líneas): validar 12,81 ms, formatear 11,04 ms, dividir en líneas 5,12 ms (≈ 29 ms en total, fuera del hilo UI). La vista solo dibuja las líneas visibles.
- **Arranque en caliente** con proyecto (`se .`), 20 ejecuciones: la 1.ª tras compilar tardó 886,7 ms; las 19 restantes p50 120,7 ms, p95 134,4 ms y máx. 136,9 ms. H1 daba p50 112,7 ms (+8 ms). Objetivo < 500 ms: PASS.
- **Reposo** 60 s con `se .`: **0,000 %** de CPU; RSS 97,6 MiB; footprint 68 MB. PASS.
- Las métricas del motor y del editor de H1 no se han tocado (el código del editor no ha cambiado en H2).

## Decisiones
- **Params**: la tabla se **añade** a la query que ya tenga la URL, en lugar de sincronizarse con ella. Es más simple y explícito y conserva lo que el usuario escribió en la URL.
- **Credenciales literales**: no se guardan en colecciones (se avisa de qué campos se omitieron). En la sesión siguen en memoria; para que persistan, se usa una variable secreta con «Remember».
- **Fuera del proyecto**: el historial y el entorno activo se guardan en datos de usuario, no en el proyecto, para no ensuciar Git.
- **Ids de petición**: aleatorios dentro de su colección; si un archivo editado a mano no los tiene, se asignan al cargar.
- **Secretos**: el almacén del sistema se inicializa de forma perezosa (solo se accede al Keychain si hay secretos marcados «Remember»).
- **Exportación cURL**: solo POSIX (no hay pruebas propias para PowerShell ni cmd.exe).
- **`directories`**: sigue fuera de las dependencias por la licencia MPL.

## Limitaciones conocidas
- **Keychain / Credential Manager reales: NOT RUN.** El flujo está probado con un almacén en memoria con la misma interfaz. Probarlo de verdad escribe en tu llavero, así que no lo he hecho sin tu permiso (paso 6 del checklist).
- No hay multipart, OAuth interactivo, scripts, ejecutor de colecciones, cookie jar, WebSockets ni proxy (fuera del MVP según PRODUCT).
- Los params no se extraen de la URL al escribirla, y la URL no se reescribe con los params.
- No hay import/export de Postman.
- La exportación cURL no resuelve variables.
- El historial es global (no separa por proyecto) y guarda la definición saneada, no la respuesta.
- No hay barra de progreso de bytes en las descargas (solo tiempo y spinner).
- El límite de descarga es un ajuste de sesión (no se persiste).
- Una petición con un secreto literal reabierta tras reiniciar llega sin ese valor (por diseño).
- Sin lector de pantalla para el contenido del editor (como en H1).

## Matriz HTTP (ACCEPTANCE, servidor local)
| Requisito | Estado |
|---|---|
| GET con query repetida y POST JSON: bytes y headers esperados | PASS |
| Basic/Bearer aplicados; fuera de logs y export por defecto | PASS (export/historial/colección probados; los errores de transporte se redactan) |
| Precedencia de variables; desconocidas bloquean; secretos de sesión no se escriben en disco | PASS |
| Cancelar libera la conexión; timeout distinto de cancelación | PASS |
| Una respuesta tardía no reemplaza a la actual | PASS |
| 4xx/5xx muestran cuerpo y estado | PASS |
| Límite de preview sobre datos descomprimidos, truncado visible, consumo acotado | PASS |
| Redirect a otro origen sin Authorization; máximo de saltos | PASS |
| Import cURL sin shell; comillas, caracteres especiales, opciones no admitidas | PASS |
| Colecciones: export/import conservan estructura; versión incompatible → error claro sin pérdida | PASS |
| TLS con certificado de prueba confiado explícitamente, sin desactivar la validación | PASS |
| Colección entre reinicios | PASS (app sin ventana) |
| Keychain / Credential Manager reales | NOT RUN |
| Windows | NOT RUN (pendiente del push y la CI) |

## Checklist manual pendiente
1. `cargo build --release && ./target/release/se .` → menú ⋯ de HTTP → **Import cURL…** → pega `curl https://httpbin.org/get -H 'Accept: application/json'` → Send → Pretty/Raw/Headers. (O, sin red externa: `python3 -m http.server 8000` y `http://127.0.0.1:8000/`.)
2. ✎ → crea el entorno «local» con `base = http://127.0.0.1:8000` → selecciónalo → URL `{{base}}/` → Send. Prueba también `{{nope}}`: debe bloquear el envío y decir qué campo falla.
3. Save (Cmd+S) → «Save request» → nueva colección → comprueba `.simpleeditor/collections/*.json` (legible, sin credenciales literales) → cierra y reabre la app → la colección y el entorno activo siguen ahí.
4. Bearer con token literal → Save → aparece el aviso de credenciales omitidas.
5. «Send and save response to file…» con un recurso grande; prueba Cancel a mitad: no debe quedar ningún archivo.
6. **Keychain**: en el editor de entornos marca una variable *Secret* + *Remember*, Save, reinicia la app y comprueba que resuelve. Verifícalo en «Acceso a Llaveros» (servicio «SimpleEditor»). Después desmarca Remember y guarda: la entrada desaparece del llavero.
7. Historial: envía varias peticiones, reinicia y abre una desde History.
8. Windows: pasos 1–7 con `target\release\se.exe .` y Credential Manager.

## Cómo ejecutar
```sh
cargo test --all-targets --all-features
cargo run --release -- .
cargo run --release --example editor_probe -- --100mib
SE_DATA_DIR=/tmp/se-data SE_SECRET_BACKEND=memory ./target/release/se .   # sin tocar tus datos ni el Keychain
```

## Siguiente paso: H3 — pulido y distribución
1. Push de H1+H2 y revisión de la CI en Windows (recuperación con `try_lock`, archivo bloqueado, almacén de Windows, compilación de los nuevos crates).
2. H3 según ROADMAP: accesibilidad (contenido del editor para AccessKit), restauración de sesión, escalas 150/200 %, paquetes .app/.msi o .zip, `se` instalable opt-in, avisos de licencias (fuentes OFL/Ubuntu, CDLA de webpki-root-certs), guía de contribución, checklist de release.

## Registro que debe mantener Claude
Para cada hito: fecha/commit, cambios, comandos y resultados, métricas con entorno, decisiones técnicas, limitaciones y siguiente paso. No marcar como completas tareas futuras.
