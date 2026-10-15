# Producto y alcance

## Objetivo
Abrir un proyecto, editar código con respuesta inmediata y probar sus endpoints sin salir de la aplicación. Una herramienta que se entiende al abrirla y sigue respondiendo mientras carga archivos o recibe una respuesta HTTP.

## Usuarios y flujo principal
Desarrollador individual trabajando con proyectos locales y APIs. Abre `se .`, selecciona un archivo, lo modifica, guarda, abre una petición HTTP, cambia `{{base_url}}`, envía y revisa estado, cabeceras y JSON. Puede volver al código mientras la petición sigue pendiente.

## MVP
### Editor
Abrir archivo o carpeta; árbol de archivos cargado bajo demanda; pestañas; indicador de cambios; guardar y guardar como; cierre con guardar/descartar/cancelar; undo/redo; selección y portapapeles; números de línea; indentación; buscar/reemplazar en archivo; resaltado inicial Rust, JavaScript, TypeScript, JSON, HTML y CSS; tema claro y oscuro; recuperación de buffers no guardados y sesión.

Un único proyecto por ventana es suficiente. Búsqueda global, multicursor, plegado, minimapa, Git integrado y terminal quedan fuera del MVP. Archivos grandes deben degradar funciones costosas de forma explícita, conservando navegación y contenido.

### HTTP
GET, POST, PUT, PATCH, DELETE, HEAD y OPTIONS. URL; query params y headers con activar/desactivar y claves repetidas; body vacío, texto o JSON; autenticación None, Bearer y Basic. Enviar/cancelar; timeout; estado, duración y bytes recibidos; cabeceras; cuerpo raw y JSON formateado; errores de DNS, TLS, conexión y timeout diferenciados.

Colecciones por proyecto, historial local limitado, entornos y variables `{{name}}`. Precedencia: valores locales de petición > entorno activo > variables de proyecto. Variables sin resolver bloquean el envío e identifican el campo. Secretos se resuelven al enviar y se omiten de exportaciones por defecto.

Importar/exportar el subconjunto documentado de cURL: método, URL, headers y cuerpos de texto/JSON. Nunca ejecutar cURL importado. Opciones no soportadas producen advertencia; no se descartan en silencio. Exportación inicialmente con quoting POSIX, identificado como tal; PowerShell no se promete sin pruebas propias.

Colecciones y entornos no secretos en JSON legible con versión de esquema, aptos para Git. Importar/exportar el formato propio. Compatibilidad completa con Postman no forma parte del MVP.

### Fuera de alcance
WebSockets, GraphQL especializado, gRPC, OAuth interactivo, scripts pre/post request, ejecutor de colecciones, multipart, cookie jar persistente, cloud sync, plugins, colaboración, cuentas y monetización integrada. Pueden añadirse posteriormente.

## Requisitos de producto
Sin backend propio. No enviar datos salvo las peticiones y operaciones iniciadas por el usuario. No contactar servidores al abrir una colección. El proyecto debe poder compilarse desde el código publicado. Elegir licencia antes de la primera publicación si aún no existe; no inventar el titular de copyright.

## Compatibilidad inicial propuesta
macOS Apple Silicon y Windows x86_64. Mantener el código portable y evaluar macOS Intel según CI disponible. Registrar versiones mínimas de OS después de comprobar dependencias y probar paquetes; no anunciarlas por suposición.
