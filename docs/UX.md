# Interfaz y comportamiento

## Dirección visual
Inspiración Jira: navegación clara, jerarquía, espacios consistentes y acciones comprensibles. Crear identidad propia para SimpleEditor. Escritorio compacto, sin paneles promocionales ni cuentas. Tipografía de UI legible y monoespaciada para código; usar fuentes del sistema o redistribuibles.

Tema claro inicial: fondo #F7F8FA, superficie #FFFFFF, texto #172B4D, texto secundario #44546F, borde #DCDFE4, acción #0C66E4. Tema oscuro: fondo #161A1D, superficie #1D2125, texto #DEE4EA, secundario #9FADBC, borde #454F59, acción #579DFF. Son tokens iniciales: comprobar contraste en estados reales, incluyendo disabled, error y selección. Grid de 4 px, espacios habituales de 8/12/16 px, esquinas de 6 px, animación mínima y prescindible.

## Distribución
Barra superior compacta con nombre de proyecto y acciones esenciales. Barra lateral redimensionable con secciones Files y HTTP. Área central con pestañas de documentos y peticiones; pestaña activa inequívoca; dirty dot con texto accesible. Barra inferior con lenguaje, línea/columna, indentación y finales de línea.

La pestaña HTTP muestra fila de método + URL + Send/Cancel, selector de entorno y pestañas Params / Headers / Body / Auth. Respuesta debajo con status, tiempo, tamaño y Body / Headers. Separador redimensionable y opción de disposición lateral en ventanas anchas. Nunca tapar el código con una petición en curso.

## Estados necesarios
Sin proyecto: Open folder, Open file y New HTTP request. Documento vacío; carga; error al abrir; cambios sin guardar; conflicto externo; recuperación disponible. Petición sin enviar; validación fallida; enviando; cancelada; timeout; respuesta recibida; cuerpo vacío; JSON inválido; binario; respuesta truncada. Ninguna pantalla finge resultados de red.

Los errores deben incluir qué ocurrió y la acción posible, conservando el trabajo. Toda operación costosa ofrece progreso discreto y cancelación cuando sea aplicable. Confirmar solo acciones que pueden perder trabajo, no la navegación habitual.

## Teclado y accesibilidad
Atajos basados en Cmd en macOS y Ctrl en Windows: S guardar, O abrir archivo, W cerrar pestaña, F buscar, Z deshacer; redo según convención de plataforma. Cmd/Ctrl+Enter envía petición solo cuando el foco pertenece a su pestaña. Escape cierra diálogos o limpia modo activo de forma predecible. Evitar capturar combinaciones de IME o atajos del sistema.

Tab/Shift+Tab y foco visible en formularios. Etiquetas accesibles para iconos; tooltips con nombre y atajo; no depender solo del color para errores/estados. Comprobar lector de pantalla donde la biblioteca lo permita y documentar limitaciones observadas. Probar 100%, 150% y 200% de escala y ventanas desde 960×640. Reducir o colapsar paneles en vez de solapar controles.
