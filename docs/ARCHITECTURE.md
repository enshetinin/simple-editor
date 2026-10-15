# Arquitectura inicial

## Estructura orientativa
Un crate con `src/main.rs`, `src/app/`, `src/editor/`, `src/http/`, `src/storage/`, `src/platform/` y `tests/`. Los nombres pueden adaptarse al repositorio. Estado y operaciones del editor deben poder probarse sin iniciar una ventana. La UI adapta eventos a comandos y presenta resultados.

## Stack y puerta de validación
Usar egui/eframe inicialmente. Verificar en el hito 0 soporte real de escritura con acentos, composición IME, selección, portapapeles, escalado y teclado en los sistemas objetivo. Medir consumo en reposo y coste de edición; no asumir que escoger Rust o GPU garantiza baja latencia.

Evaluar almacenamiento de texto tipo rope (p. ej. Ropey) o piece table según necesidades medidas. La elección debe quedar registrada. Un buffer basado en String solo es válido como prototipo identificado. Separar offsets de bytes UTF-8, índices de caracteres, graphemes y coordenadas visuales. Tree-sitter usa offsets de bytes: las conversiones deben probarse, especialmente con emoji y saltos CRLF.

## Edición
DocumentId estable, revisión creciente, buffer, metadatos de encoding/newline, estado dirty, selección, cursor e historial de transacciones. Undo/redo agrupa escritura natural y conserva el contenido exacto. Mover cursor y borrar respetan unidades Unicode apropiadas. Tabulación, columnas visuales y posición lógica no son intercambiables.

Cache de layout y resaltado por rangos/revisión. Renderizar líneas visibles con margen, no el documento completo. Desactivar o limitar resaltado en archivos o líneas extraordinariamente grandes con aviso visible. Las respuestas de workers llevan DocumentId y revisión; descartar resultados obsoletos. El árbol de archivos evita recorridos completos síncronos y ciclos de enlaces simbólicos.

## Concurrencia
La UI es dueña del estado visual. Workers procesan I/O, parsing y HTTP; mensajes tipados comunican resultados. Canales acotados o coalescing para evitar acumulación. Runtime Tokio reutilizado. Integración por eventos que solicitan repintado cuando hay cambios; no bucle de polling continuo. Debounce en tareas repetidas y cancelación en búsquedas o análisis reemplazados.

## HTTP
RequestDefinition separada de ResolvedRequest. Validación y expansión de variables antes de enviar; esquema HTTP/HTTPS explícito; preservar headers y query params repetidos. Reutilizar clientes y pools por configuración compatible. Credenciales no deben filtrarse al redirigir a otro origen.

Cada envío tiene un id propio y cancelación. Una respuesta antigua no sustituye a la más reciente de una pestaña. Recibir en streaming; límite por defecto de previsualización de 5 MiB sobre bytes descomprimidos, con estado «truncated». No seguir acumulando en RAM después del límite; abortar lectura de preview o escribir a disco cuando el usuario elija descarga. Para descargas, límite configurable, cancelación y manejo de disco lleno.

Timeout total inicial 30 s, configurable; redirects limitados a 10, mostrar URL final y documentar cambios de método. Sin reintento automático de peticiones. El estado HTTP 4xx/5xx es una respuesta válida, no un error de transporte. Medir duración total hasta terminar la lectura de preview o truncarla y etiquetarla claramente. Nunca pretty-print JSON gigante en el hilo UI.

## Persistencia
Configuración, historial y recuperación en directorios de datos del usuario según plataforma. Colecciones versionables en `.simpleeditor/collections/`; entornos públicos en `.simpleeditor/environments/`. Archivos locales secretos no se crean ahí por defecto. No añadir configuración al proyecto hasta que el usuario guarde una colección o entorno.

JSON con schema_version y validación. Imports con límites de tamaño, sin sobrescribir colecciones existentes automáticamente. Recuperación separada de los archivos originales; restaurarla requiere elección del usuario. Detectar modificación externa antes de guardar y ofrecer recargar o guardar copia. Mantener permisos y tratar symlinks deliberadamente; probar sustitución en Windows, fallos de permisos y bloqueo por otro proceso.

Historial acotado inicialmente a 100 entradas, sin bodies ni credenciales persistidos por defecto. Filtrar headers sensibles conocidos y variables marcadas secretas; las URLs también pueden contener secretos, por lo que deben sanearse antes de guardar. Valores de sesión en memoria; opt-in para recordar secretos con Keychain/almacén de credenciales de Windows. Fallo del almacén: modo sesión, nunca fallback silencioso a texto plano.

## CLI y paquetes
`se .`, `se path/to/file`, rutas con espacios/Unicode, varios archivos, `--help` y `--version`. Resolver rutas respecto al directorio de invocación, no al bundle. Puede abrir una ventana por invocación en MVP. La instalación del comando es opt-in y no sobrescribe otro `se`; ofrecer nombre alternativo `simpleeditor`.

Crear paquetes nativos y documentar ejecución desde Finder/Explorer. Firma, notarización y certificados de distribución son pasos de release dependientes de credenciales reales; no equivalen a una suscripción dentro de la app. No inventar firma ni solicitar secretos en archivos del repositorio.

## Referencias oficiales
Verificar versiones al implementar; este documento no fija versiones sin compilar.
- https://github.com/emilk/egui
- https://docs.rs/eframe/latest/eframe/
- https://tree-sitter.github.io/tree-sitter/
- https://docs.rs/reqwest/latest/reqwest/
- https://tokio.rs/
