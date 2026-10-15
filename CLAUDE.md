# SimpleEditor — instrucciones de desarrollo

## Producto
SimpleEditor es un editor de código de escritorio, open source, escrito en Rust, para macOS y Windows. Incluye un cliente HTTP sencillo y completo para el trabajo diario. Nombre visible: SimpleEditor. Comando principal: `se .`; nombre alternativo previsto: `simpleeditor`. Lema: «Escribe código. Prueba APIs. Así de simple.»

Prioridades: integridad del trabajo del usuario, latencia de edición, estabilidad, sencillez de uso y funciones. El cliente HTTP pertenece al núcleo del producto. La aplicación funciona localmente, sin cuentas obligatorias, suscripciones, servicios propios ni telemetría por defecto.

## Antes de implementar
1. Inspecciona el repositorio, su estado Git y sus instrucciones existentes. Conserva los cambios del usuario y la licencia existente. No supongas que el repositorio está vacío.
2. Lee `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`, `docs/UX.md`, `docs/ROADMAP.md` y `docs/ACCEPTANCE.md`.
3. Consulta `docs/STATUS.md` para continuar desde el estado real. Este paquete contiene especificaciones: no implica que exista código implementado.
4. Comprueba Rust, Cargo y las herramientas nativas necesarias. Verifica versiones y APIs en documentación oficial, selecciona versiones estables compatibles y conserva Cargo.lock. No inventes APIs ni resultados de compilación.
5. Implementa el hito solicitado hasta dejarlo ejecutable y verificable. No te detengas después de proponer un plan. Si hay un bloqueo externo, completa lo independiente y documenta el bloqueo exacto.

## Decisiones iniciales
- Rust para aplicación, motor y cliente HTTP. Sin Electron ni WebView para la interfaz principal.
- egui/eframe como punto de partida, sujeto a la prueba técnica del hito 0.
- Motor de edición separado de la UI. Tree-sitter para resaltado incremental; reqwest y Tokio para HTTP asíncrono.
- Empieza con un crate y módulos bien delimitados. Crea un workspace de varios crates solo si existe un motivo concreto.
- Código e identificadores en inglés; comunicación con el propietario en español. UI inicialmente en inglés para el proyecto open source; textos centralizados y preparados para futura traducción.
- No implementes LSP, terminal, extensiones, sincronización, colaboración o IA integrada en el MVP.

## Reglas de implementación
- Nunca realices red, recorridos grandes de archivos o parsing costoso en el hilo de UI.
- No clones ni analices el documento completo en cada pulsación o frame. Procesa cambios incrementales y renderiza contenido visible.
- Un widget de texto estándar puede servir para validar el primer flujo, pero no se considera el motor definitivo sin mediciones de documentos grandes, Unicode e IME.
- Conserva finales de línea y BOM existentes. No conviertas archivos binarios o codificaciones no soportadas de forma silenciosa.
- Guardado mediante archivo temporal y sustitución segura con comportamiento verificado en ambos sistemas. Un fallo nunca debe destruir el archivo original ni borrar el buffer modificado.
- No interpretes URL, texto de peticiones o imports cURL como comandos de shell. Parser y validación explícitos.
- TLS verificado por defecto. HTTP local debe funcionar. No persistas secretos en texto plano por defecto; utiliza el almacén del sistema cuando se pida recordarlos.
- Errores comprensibles y recuperables; evita panics por entradas o fallos de I/O. No añadas unsafe sin necesidad documentada.
- Evita dependencias innecesarias y revisa licencias de dependencias, fuentes, iconos y gramáticas antes de distribuir.
- No cambies de stack sin documentar el problema observado, alternativas y decisión.

## Validación y entrega
Ejecuta, cuando exista el proyecto: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings` y `cargo test --all-targets --all-features`. Compila release. Ajusta features si son mutuamente excluyentes y documenta la matriz real.
Prueba los comportamientos, no detalles triviales de implementación. Usa servidor HTTP local para pruebas deterministas. Configura CI en Windows y macOS; una compilación en un único sistema no demuestra compatibilidad con ambos.

Al cerrar cada hito actualiza `docs/STATUS.md`: qué funciona, comandos ejecutados y resultados, métricas reales, límites, siguiente paso. Distingue pruebas automáticas, revisión visual y comprobaciones pendientes. Nunca inventes screenshots, benchmarks o resultados de CI. No publiques releases ni hagas push sin una instrucción del propietario; sí prepara los archivos necesarios.
