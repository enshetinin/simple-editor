# Prompts para Claude Code

## Primer arranque: implementar H0

Lee CLAUDE.md y todos los documentos de docs indicados allí. Estamos creando SimpleEditor, un editor de código de escritorio muy rápido, en Rust, para macOS y Windows, con cliente HTTP integrado. El comando será se . y la interfaz debe seguir la dirección visual definida en docs/UX.md.

Inspecciona primero el repositorio y el entorno. Conserva el trabajo y las instrucciones existentes. Expón un plan breve y después implementa H0 de docs/ROADMAP.md hasta tener una aplicación ejecutable: apertura/edición/guardado de archivo y una petición HTTP real con respuesta y cancelación. No te quedes en el scaffolding, documentación, mockups o datos simulados.

Comprueba versiones/APIs oficiales antes de añadir dependencias. Valida pronto las limitaciones del widget de texto y del renderizado con Unicode, archivos grandes y consumo en reposo. Si una decisión propuesta no funciona, documenta evidencia y una alternativa concreta antes de ampliar el alcance.

Ejecuta las comprobaciones disponibles y configura CI macOS/Windows. No afirmes haber probado un sistema al que no tienes acceso. Actualiza docs/STATUS.md con resultados reales y deja instrucciones exactas para que yo lo ejecute. Al terminar H0, resume qué funciona, cómo probarlo y qué falta antes de H1. No implementes hitos posteriores ni publiques cambios en remoto en esta primera tarea.

## Continuar con H1

Lee CLAUDE.md y docs/STATUS.md. Verifica el estado real de H0 y corrige bloqueos de ese hito. Implementa H1 según ROADMAP y ACCEPTANCE. Prioriza fiabilidad del motor, Unicode/IME, guardado y rendimiento. No amplíes funciones HTTP más allá de lo necesario para conservar el flujo existente. Ejecuta pruebas y mediciones, actualiza STATUS y termina con instrucciones de prueba manual y limitaciones verificadas.

## Continuar con H2

Lee CLAUDE.md y docs/STATUS.md. Implementa H2 completo según PRODUCT, ARCHITECTURE y ACCEPTANCE. Usa servidor local en las pruebas. Colecciones, variables, secretos, importación cURL y límites de respuesta deben ser reales, persistentes donde corresponde y probados. Conserva la fluidez del editor. Actualiza STATUS con evidencias y decisiones, y documenta exactamente el subconjunto cURL soportado.

## Continuar con H3

Lee CLAUDE.md y docs/STATUS.md. Implementa H3: pulido de UI, accesibilidad, rendimiento medido, paquetes macOS/Windows, CLI instalable y documentación open source. Conserva la licencia existente; si falta, prepara la decisión para el propietario antes de publicar. No inventes titulares, certificados ni pruebas de plataforma. Prepara los artefactos disponibles y un checklist de release, sin publicar aún.

## Retomar tras una interrupción

Lee CLAUDE.md, docs/STATUS.md y el diff actual. Determina qué está implementado y probado realmente, identifica el hito en curso y continúa desde ahí. Conserva cambios sin commit. No reinicies la arquitectura ni repitas trabajo validado salvo que haya evidencia de un problema. Actualiza STATUS al terminar.

## Corregir un fallo

Reproduce el fallo que describo a continuación. Localiza la causa, añade una prueba de regresión si corresponde, aplica la corrección mínima coherente con la arquitectura y verifica que el flujo código→HTTP sigue funcionando. No añadas funciones ajenas. Actualiza docs/STATUS.md con reproducción, corrección y pruebas. Fallo: [pegar aquí pasos, resultado esperado, resultado observado y logs sin secretos].
