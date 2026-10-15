# Estado de SimpleEditor

Actualizado: 2026-10-15 — cierre de **H1** (editor fiable).
Base: commit `568b61b` (H0) + cambios de H1 **sin commit** en el árbol de trabajo (no he hecho commit ni push).

## Resumen
- **H0**: cerrado. La CI de GitHub para `568b61b` pasó en **macOS y Windows**: runs `38705227999` (push) y `38705289540` (pull request), con formato, clippy, tests y build release en verde en ambos sistemas. Consultado vía la API pública de GitHub; no he visto los logs completos porque requieren autenticación.
- **H1**: implementado y validado en macOS con pruebas automáticas y mediciones. En Windows **no se ha ejecutado** el código de H1: la CI correrá cuando hagas push. Revisión manual en la ventana real (IME, ratón, aspecto): **pendiente** (ver checklist).
- H2 y H3: pendientes. El HTTP no se ha ampliado respecto a H0.

## Entorno de las mediciones
macOS 27.0.1 (26A434), Apple M3, 8 GB RAM, arm64. rustc 1.99.0. Perfil release. Corpus determinista generado por `examples/editor_probe.rs` (código Rust repetido; mezcla Unicode con acentos, emoji ZWJ, banderas, combinados, griego, cirílico, CJK, árabe y `\r`; línea única de 1 MiB; 10 MiB; 100 MiB).

## Qué hay nuevo en H1
**Motor (`src/editor`, sin UI)**
- Buffer **ropey 1.6.1** con saltos de línea solo LF (`default-features = false`), coherente con tree-sitter y con el archivo. Un `\r` de CRLF en documentos mixtos queda dentro de la línea y no se dibuja.
- Historial transaccional: cada paso guarda las operaciones y la selección antes/después. Escritura natural agrupada por palabras (pausa > 1 s o movimiento de cursor cortan el grupo), Backspace consecutivos agrupados, pegar/reemplazar/indentar = un paso. El estado «sucio» se calcula por identificador de estado del historial, así que deshacer hasta el punto guardado deja el documento limpio.
- Selección ancla/cabeza en índices de carácter. Movimiento y borrado por **grapheme clusters** extendidos (`unicode-segmentation`, también a través de los fragmentos internos del rope): emoji ZWJ, banderas, acentos combinados y `\r\n` nunca se parten. Palabras, inicio inteligente (Home), columna preferida en vertical con tabuladores.
- Offsets separados: bytes (tree-sitter, `InputEdit` con `Point` en bytes), caracteres (selección), graphemes (movimiento) y columnas visuales (tabulador = 4).
- Indentación: detección (tabs / 2 / 4 / 8 espacios), Tab inserta hasta el siguiente tope o indenta las líneas seleccionadas, Shift+Tab desindenta, Enter conserva la indentación y Backspace en espacios iniciales vuelve al tope anterior.
- Pegar normaliza `\r\n`/`\r` a `\n` en documentos LF/CRLF; en documentos mixtos el texto se conserva tal cual.
- Búsqueda (`search.rs`) sobre una instantánea del rope en un worker, con índices de carácter UTF-8 seguros, sensible o no a mayúsculas (plegado simple 1:1) y un máximo de 100 000 resultados. Reemplazar todo es un único paso de deshacer.
- Guardado en streaming desde el rope (`encode_to` + `atomic_write_with`: temporal en el mismo directorio → fsync → rename) ejecutado en un worker. Comprueba conflictos externos antes de escribir.
- Archivos de hasta **256 MiB**. Por encima de 8 MiB se abren en modo archivo grande, con el resaltado desactivado y aviso en la barra de estado. Las líneas de más de 10 000 caracteres se dibujan con ancho fijo aproximado y sin resaltado, con un aviso al abrir.

**Resaltado (`src/syntax`)**
- tree-sitter 0.27.1 con las gramáticas oficiales (todas MIT): rust 0.24.2, javascript 0.25.0 (con JSX), typescript 0.23.2 (TS y TSX, sobre la consulta de JS), json 0.24.8, html 0.23.2 y css 0.25.0. La prueba `all_grammars_load_and_queries_compile` verifica que cargan y que las consultas compilan.
- Worker propio: recibe `InputEdit`s y una instantánea O(1) del rope, reparsea de forma incremental y consulta solo el rango visible ±200 líneas. Agrupa ráfagas de teclas en un único parseo. Las respuestas llevan la revisión; mientras llegan, los spans se desplazan localmente con cada edición (sin parpadeo) y los resultados antiguos se ajustan o se descartan.

**Vista (`src/app/editor_view.rs`)**: widget propio que sustituye al `TextEdit`
- Solo calcula el layout de las líneas visibles (egui reutiliza galleys idénticos), con números de línea fijos al desplazar en horizontal, línea actual resaltada, selección, coincidencias de búsqueda y cursor sin parpadeo (sin repintados en reposo).
- IME: el preedit se dibuja en línea y subrayado sin entrar en el documento; el commit inserta (un paso de deshacer); se soporta `DeleteSurrounding`; la ventana de candidatos se posiciona con `IMEOutput`. Las teclas durante la composición pertenecen al IME.
- Portapapeles (Copy/Cut/Paste de egui), ratón (clic, Shift+clic, arrastre, doble clic = palabra, triple = línea) y desplazamiento inmediato al cursor.
- Atajos según la plataforma (ver README). Undo/redo funcionan aunque ningún widget tenga foco (por ejemplo, tras pulsar «Replace all»).

**App**
- Guardar es asíncrono (indicador «Saving…»). Cerrar pestaña o ventana con cambios pregunta guardar/descartar/cancelar; «Save all» espera a que terminen todos los guardados.
- Conflictos externos: al recuperar el foco de la ventana se comprueban en segundo plano los archivos abiertos. Una pestaña sin cambios se recarga; una con cambios muestra un banner (Reload / Keep my version); un archivo borrado queda marcado como modificado. Guardar sobre un cambio externo abre el diálogo Overwrite / Save as / Cancel.
- Recuperación ante caídas (`src/storage/recovery.rs`):
  - Cada instancia tiene una sesión bloqueada con `File::try_lock` en el directorio de datos: `~/Library/Application Support/SimpleEditor/recovery` en macOS, `%LOCALAPPDATA%\SimpleEditor\recovery` en Windows, o `$SE_DATA_DIR/recovery` si está definido.
  - Las instantáneas (texto + JSON con `schema_version`) se escriben de forma atómica en un hilo, 1,5 s después de la última edición (30 s en archivos grandes).
  - Al arrancar se detectan las sesiones sin lock (instancias caídas) y se ofrece Restore / Discard. Restaurar abre el archivo y aplica el texto recuperado como cambio sin guardar y deshacible; nunca escribe en el original. El cierre limpio borra la sesión.
- Barra de estado: lenguaje, Ln/Col (y caracteres seleccionados), indentación, codificación, final de línea y estado de guardado/resaltado.
- Dependencias añadidas: ropey, unicode-segmentation, tree-sitter + 6 gramáticas, streaming-iterator, serde y serde_json (todas MIT o MIT/Apache-2.0). Probé `directories`, pero lo **retiré** porque arrastraba `option-ext` (MPL-2.0); calculo las rutas de datos con unas pocas líneas propias.

## Comandos ejecutados y resultados (macOS, este equipo)
| Comando | Resultado |
|---|---|
| `cargo fmt --check` | PASS |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | PASS (0 avisos) |
| `cargo test --locked --all-targets --all-features` | PASS, **67 pruebas**, 3 ejecuciones seguidas sin fallos (+1 ignorada por defecto) |
| `cargo test --test save -- --ignored` | PASS: disco lleno real en una imagen HFS+ de 1 MiB |
| `cargo build --locked --release --bins --examples` | PASS (`se` ≈ 28,5 MiB con tablas de línea de depuración) |
| `cargo run --release --example editor_probe -- --100mib` | Ver métricas |
| `cargo run --release --example textedit_probe` | Reproduce las cifras de H0 (TextEdit 1 MiB: tecleo ≈ 5 ms) |
| App real (`se`, `se . archivo.rs`, `se c10.rs`, `se c100.rs`) | Se abre, en reposo 60 s; arranque, CPU y memoria medidos |
| Pulsaciones reales con `osascript` | **No ejecutado**: macOS deniega el permiso de accesibilidad a osascript |

Pruebas automáticas por área:
- Motor (`src/editor`): round-trip de LF/CRLF/mixto/BOM; streaming CRLF entre fragmentos; graphemes (ZWJ, banderas, combinados, `\r\n`, a través de fragmentos del rope); palabras; agrupación y restauración de selección en undo/redo; dirty a través de undo/redo después de guardar; Backspace de graphemes y topes de indentación; Enter con indentación; pegar normalizado como un paso; indentar/desindentar; reemplazo múltiple UTF-8 como un paso; `InputEdit` en bytes/puntos con emoji; búsqueda Unicode, límites y consultas multilínea; detección de indentación.
- Sintaxis: las 7 gramáticas cargan y sus consultas compilan; cada lenguaje produce resaltado; el reparseo incremental tras una edición cambia la categoría; los spans se desplazan con inserciones/borrados y los resultados antiguos se ajustan.
- Vista: columna preferida con líneas cortas y tabuladores; aterrizaje en límites de grapheme; atajos (Home inteligente, Shift+End, palabra, borrar palabra, undo); el cursor queda en pantalla en una línea de 300 000 caracteres y en un documento de 200 000 líneas.
- Guardado (`tests/save.rs`): bytes idénticos sin cambios; CRLF+BOM tras editar; conflicto externo; directorio de solo lectura (original y buffer intactos, sin temporales); permisos y symlinks; untitled → save as; rechazo de binario/latin-1/>256 MiB/directorio; ~18 MiB CRLF con round-trip exacto en streaming; archivo bloqueado (solo Windows, lo ejecutará la CI); disco lleno (macOS, ignorada por defecto, PASS).
- UI headless (`tests/ui_flow.rs`, la app real sin ventana):
  - escribir → Cmd+S → archivo con CRLF intacto → petición HTTP al servidor local;
  - IME (preedit fuera del documento, teclas durante la composición, commit, tecla muerta «´»→«é», un paso de deshacer);
  - teclado: Enter con indentación, Tab, selección, cortar/pegar, Backspace de emoji, deshacer hasta el estado del archivo y rehacer;
  - Cmd+F con búsqueda en worker, siguiente coincidencia, «Replace all» y Cmd+Z;
  - resaltado de la región visible;
  - **caída simulada** con restauración: el texto vuelve, el archivo no se toca y deshacer vuelve al contenido del disco;
  - cambios externos (recarga si la pestaña está limpia, banner si tiene cambios, diálogo de conflicto al guardar);
  - cerrar con guardar o descartar.
- HTTP (sin cambios desde H0): 10 pruebas contra un servidor local.

## Métricas reales (release, este equipo)
**Motor `Document`** (rope + historial + `InputEdit`), µs p50 / p95, 2000 operaciones (pegar/undo/redo: 200):
| Corpus | Carga ms | Insertar carácter | Backspace | Pegar 150 líneas | Undo pegar | Redo pegar |
|---|---|---|---|---|---|---|
| código 1 KiB | 0,13 | 0,8 / 0,8 | 1,0 / 1,2 | 8,4 / 8,6 | 0,9 / 1,0 | 8,1 / 8,2 |
| código 1 MiB | 0,75 | 0,9 / 1,0 | 1,0 / 1,0 | 11,5 / 11,6 | 1,0 / 1,0 | 11,2 / 11,3 |
| Unicode+CR 1 MiB | 0,58 | 0,9 / 1,0 | 1,2 / 1,3 | 11,5 / 12,0 | 1,0 / 1,0 | 11,2 / 11,6 |
| línea única 1 MiB | 0,23 | 0,7 / 0,8 | 0,8 / 0,9 | 11,7 / 12,2 | 1,0 / 1,0 | 11,5 / 12,0 |
| código 10 MiB | 6,27 | 0,9 / 1,0 | 1,3 / 1,4 | 12,5 / 14,8 | 1,0 / 1,2 | 12,1 / 14,5 |
| código 100 MiB | 63,97 | 0,9 / 1,0 | 1,2 / 1,4 | 13,3 / 13,5 | 1,0 / 1,1 | 13,0 / 13,2 |

Objetivo «operación local p95 < 2 ms en 1 MiB»: **PASS** (≈ 1 µs).

**Widget de editor headless** (layout + teselado en CPU, sin GPU; es un proxy, **no** latencia tecla→pantalla), ms, 60 frames:
| Corpus | 1.er frame | Reposo p50/p95 | Tecleo p50/p95/máx | Av Pág p50/p95/máx | Teclas aplicadas |
|---|---|---|---|---|---|
| código 1 KiB | 2,90 | 0,04/0,04 | 0,05/0,11/0,16 | 0,04/0,04/0,05 | 60/60 |
| código 1 MiB | 2,00 | 0,07/0,07 | 0,08/0,14/0,21 | 0,16/0,17/0,18 | 60/60 |
| Unicode+CR 1 MiB | 2,15 | 0,07/0,08 | 0,09/0,09/0,11 | 0,17/0,18/7,51 | 60/60 |
| línea única 1 MiB | 1,01 | 0,01/0,01 | 0,02/0,04/0,04 | 3,36/3,39/3,43 | 60/60 |
| código 10 MiB | 1,96 | 0,06/0,07 | 0,07/0,10/0,12 | 0,16/0,17/0,23 | 60/60 |
| código 100 MiB | 1,89 | 0,06/0,07 | 0,07/0,11/0,16 | 0,16/0,18/0,19 | 60/60 |
| código 1 MiB + spans | 2,06 | 0,06/0,07 | 0,07/0,13/0,17 | 0,15/0,16/0,16 | 60/60 |

Comparación con H0 (`TextEdit` estándar): 1 MiB 5,1 → 0,08 ms; 10 MiB 54 → 0,07 ms; línea de 1 MiB 74–112 → 0,02 ms por tecla. El máximo de 7,5 ms con Unicode es el primer dibujo de glifos nuevos (CJK/árabe) en el atlas de fuentes. Av Pág en la línea única (3,4 ms) se debe al cálculo de la columna visual, que es O(columna).

**Tree-sitter** (worker, Rust), ms:
| Corpus | Parseo inicial | Reparseo incremental p50/p95 | Consulta de pantalla p50/p95 | Edición→resultado p50/p95/máx |
|---|---|---|---|---|
| 1 MiB | 79,7 | 1,88 / 2,44 | 0,17 / 0,18 | 2,05 / 2,61 / 3,14 |
| 8 MiB (límite) | 649,6 | 26,8 / 29,5 | 0,17 / 0,19 | 27,0 / 29,8 / 30,2 |

Todo ocurre fuera del hilo de UI. En 8 MiB el color llega ~2 frames después de la tecla; mientras tanto se usan los spans desplazados localmente.

**Disco** (ms; la app los ejecuta en workers): abrir = leer + validar UTF-8 + finales de línea + rope + detección de indentación; guardar = escritura atómica + fsync.
| Corpus | Abrir | Guardar |
|---|---|---|
| 1 MiB | 1,59 | 9,96 |
| 10 MiB | 13,13 | 12,37 |
| 100 MiB | 131,39 | 82,91 |

**App real**:
- **Arranque en caliente** (`SE_STARTUP_PROBE=1`, desde `main()` hasta el 2.º frame), 20 ejecuciones: la 1.ª tras compilar tardó 514,2 ms; las 19 restantes p50 112,7 ms, p95 115,5 ms y máx. 117,3 ms. Objetivo < 500 ms: **PASS**.
- **Reposo** 60 s en primer plano: ventana vacía **0,000 %**; proyecto + `editor_view.rs` con el editor enfocado y resaltado **0,000 %**. Objetivo < 1 %: **PASS**.
- **Memoria**:
  - ventana vacía: RSS 113,0 MiB (footprint 71 MB);
  - con proyecto y archivo resaltado: RSS 95,1 MiB (footprint 87 MB);
  - con 10 MiB abiertos: RSS 136,8 MiB (footprint 99 MB);
  - con 100 MiB abiertos: RSS 297,6 MiB (footprint 384 MB).

  Objetivo «ventana vacía < 150 MiB»: **PASS**. Los 100 MiB funcionan con memoria acotada (≈ 3× el tamaño); no hay objetivo numérico.
- Estas mediciones de la app se tomaron antes de sustituir `directories` por rutas propias. Ese cambio solo afecta a la resolución de una ruta al arrancar; no las he repetido.

## Decisiones técnicas
- **Buffer: ropey** (inserción ≈ 1 µs a cualquier tamaño, frente a O(n) de `String`). Lo descarté como piece table propia porque ropey cubre las necesidades medidas y está probado.
- **Vista: widget propio sobre egui**. Mantengo egui/eframe para la interfaz: no hay evidencia que justifique cambiar de stack.
- **Error de egui corregido**: egui trata Enter/Espacio sobre un widget enfocado como un «clic». Si ese clic volvía a pedir el foco, se reiniciaba el filtro y el siguiente Tab sacaba el foco del editor. Ahora solo una pulsación real del puntero da el foco; una prueba lo cubre.
- Mi barra de búsqueda, los diálogos y los campos HTTP siguen usando el `TextEdit` de egui: son textos pequeños.
- **Resaltado**: consultas oficiales de cada gramática, con la precedencia de tree-sitter-highlight (gana el primer patrón; los nodos pequeños sobrescriben a los grandes). En Rust los enteros salen como `constant.builtin`; usan el mismo color que los números.
- Los colores de sintaxis y los tokens de la UI cumplen **WCAG AA (≥ 4,5:1)** sobre fondo y superficie en ambos temas; hay un test que lo comprueba.
- El cursor no parpadea, para que el reposo sea 0 %.
- El scroll al cursor es inmediato (sin animación).
- Recuperación: lock por sesión para distinguir instancias vivas de caídas; nunca escribe junto a los archivos del usuario. Las pruebas usan directorios temporales y no tocan tus datos.

## Limitaciones verificadas o conocidas
- **IME, ratón y aspecto en la ventana real**: no verificados por mí (sin permiso de captura ni de pulsaciones). La lógica del IME está probada con eventos de egui, no con un IME del sistema.
- Lector de pantalla: el editor se expone como «Code editor» (tipo TextEdit), pero **su contenido no es accesible** para VoiceOver/Narrator con esta implementación (el contenido accesible de un widget propio requiere AccessKit a bajo nivel). Queda para H3.
- Líneas de más de 10 000 caracteres: ancho fijo aproximado (CJK/emoji pueden desalinear el cursor dentro de la línea) y sin resaltado. Más de ~2 millones de columnas o ~1 millón de líneas pierden precisión de píxel en el desplazamiento (f32), sin perder contenido.
- Resaltado desactivado por encima de 8 MiB. El parseo inicial de 8 MiB tarda ~0,65 s en el worker.
- Búsqueda: solo una línea; el plegado de mayúsculas es simple; máximo 100 000 coincidencias (se indica). Sin regex.
- Un solo cursor (el multicursor queda fuera del MVP); sin ajuste de línea; tabulador mostrado con ancho 4.
- No se detecta un cambio externo mientras la ventana tiene el foco: se comprueba al recuperarlo y antes de guardar.
- En Windows siguen pendientes la apertura desde el Explorador (consola visible) y el «archivo bloqueado»; este último tiene test y la CI lo ejecutará.
- HTTP sin cambios desde H0: no hay persistencia de borradores.

## Matriz del editor (ACCEPTANCE)
| Requisito | Estado |
|---|---|
| Round-trip abrir/guardar conserva bytes (BOM, CRLF) | PASS (automático) |
| Undo/redo de inserción, borrado, pegado, reemplazo y Unicode | PASS (automático) |
| Cerrar documento modificado: guardar/descartar/cancelar | PASS (automático, headless) |
| Permisos: conserva original y buffer | PASS (automático, Unix) |
| Disco lleno: conserva original y buffer | PASS (macOS, imagen de 1 MiB; test ignorado por defecto) |
| Archivo bloqueado (Windows) | Test escrito; NOT RUN (pendiente de CI) |
| Modificación externa no se sobrescribe sin informar | PASS (automático) |
| La restauración tras caída no sobrescribe el original | PASS (automático) |
| Búsqueda/reemplazo sin romper UTF-8, extremos del documento | PASS (automático) |
| Acentos e IME revisados manualmente en cada OS | NOT RUN (manual pendiente; lógica probada con eventos) |
| Portapapeles, foco y escalado | Portapapeles y foco: PASS headless; escalado: NOT RUN |
| Escritura y scroll en corpus grande sin copias ni parseos completos por tecla | PASS (medido: tecleo p95 0,11 ms con 100 MiB; parseo incremental) |
| Windows | NOT RUN (CI preparada) |

## Checklist manual pendiente (propietario)
1. `cargo build --release && ./target/release/se .` y abre `src/app/editor_view.rs`: colores de sintaxis, números de línea, línea actual.
2. Escribe con acentos (´+e, ¨+u), `ñ`, emoji (Ctrl+Cmd+Espacio) y composición IME (japonés/chino): el texto subrayado durante la composición, el texto final correcto y la ventana de candidatos junto al cursor.
3. Ratón: clic, arrastre, doble y triple clic, Shift+clic; copiar/pegar con otra app; Cmd+Z / Shift+Cmd+Z.
4. Cmd+F, escribe un término, Enter / Shift+Enter, Alt+Cmd+F → Replace / Replace all → Cmd+Z.
5. Edita sin guardar, mata el proceso (`pkill -9 se`), vuelve a abrir: aparece el banner de recuperación → Restore → el texto vuelve como cambio sin guardar y el archivo no ha cambiado.
6. Con un archivo abierto, edítalo desde otra app y vuelve a SimpleEditor: con la pestaña limpia se recarga; con la pestaña modificada aparece el banner.
7. Abre un archivo de 100 MiB (por ejemplo, generado con `python3 -c "print('let x = 1;\n'*10_000_000)" > /tmp/big.rs`): «Highlighting off (large file)» y escritura fluida.
8. Tema claro/oscuro, escalas de 100/150/200 % y ventana de 960×640.
9. Windows: los mismos pasos con `target\release\se.exe .`.

## Cómo ejecutar
```sh
cargo test --all-targets --all-features
cargo test --test save -- --ignored                    # macOS: disco lleno real
cargo run --release -- .                               # = target/release/se .
cargo run --release --example editor_probe -- --100mib
cargo run --release --example textedit_probe           # referencia de H0
SE_STARTUP_PROBE=1 ./target/release/se                 # tiempo hasta el 2.º frame
SE_DATA_DIR=/tmp/se-data ./target/release/se .         # recuperación en otra carpeta
```
`examples/editor_probe.rs` es nuevo; la sonda de H0 se ha renombrado a `examples/textedit_probe.rs` (rename ya en el índice de git).

## Siguiente paso: H2 — HTTP de uso diario
1. Hacer push y revisar la CI de Windows de H1 (en especial el test de archivo bloqueado y la recuperación con `try_lock`); completar el checklist manual.
2. H2 según PRODUCT/ACCEPTANCE: query y headers repetidos con activar/desactivar, auth Bearer/Basic, variables y entornos con precedencia, JSON pretty/raw en worker, historial saneado, colecciones versionadas en `.simpleeditor/`, import/export propio y del subconjunto cURL, secretos en el Keychain / almacén de Windows (opt-in), redirecciones sin filtrar Authorization y TLS con fixture local.

## Registro que debe mantener Claude
Para cada hito: fecha/commit, cambios, comandos y resultados, métricas con entorno, decisiones técnicas, limitaciones y siguiente paso. No marcar como completas tareas futuras.
