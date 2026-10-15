# Checklist de release (no publicado)

Nada de esto se ha publicado. No hay tag, release de GitHub, firma Developer ID ni notarización. Los artefactos de CI son temporales (14 días) y no son una release.

## 1. Decisiones del propietario (bloquean la primera release pública)
- [ ] **Licencia**: el repo tiene MIT con «Copyright (c) 2026 Yev». Confirma que el titular es correcto. No lo he cambiado.
- [ ] **Identificador de bundle macOS**: el script usa `io.github.enshetinin.simpleeditor` (derivado del repo; se cambia con `SE_BUNDLE_ID`). Una vez publicado no conviene cambiarlo.
- [ ] **Canal de seguridad** en `SECURITY.md` (email o GitHub private reporting).
- [ ] **Código de conducta**: adoptar uno (por ejemplo, Contributor Covenant) o no. No he añadido ninguno.
- [ ] **Versión**: `Cargo.toml` tiene 0.1.0. Decide si la primera release es 0.1.0.
- [ ] **Firma y notarización**:
  - macOS: certificado «Developer ID Application» y perfil de `notarytool` (Apple Developer Program, de pago).
  - Windows: certificado de firma de código (OV/EV) o Azure Trusted Signing.

  Sin firma, Gatekeeper rechaza la app (verificado: `spctl: rejected`) y SmartScreen avisará en Windows.
- [ ] **Arquitecturas**: solo se construye **macOS arm64** (en local y en `macos-latest`) y **Windows x86_64** (CI). macOS Intel y Windows ARM64 **no** se construyen ni se prueban: no las anuncies.

## 2. Verificación previa
- [ ] Push de `develop`. CI verde en macOS y Windows: fmt, clippy, tests (incluido `locked_file_on_windows…`), build y job `package`.
- [ ] `python3 scripts/third_party_licenses.py --check` en verde (lo comprueba la CI en macOS).
- [ ] Revisa en `THIRD_PARTY_LICENSES.md` la tabla «Crates without a license file in their package»: 41 crates usados bajo Apache-2.0 / MIT / BSD-3-Clause / BSL-1.0 con el texto canónico incluido.
- [ ] Prueba manual en **macOS** (checklists de `docs/STATUS.md`: editor, HTTP y H3) con el DMG de la CI o el local:
  - abrir desde Finder, arrastrar archivos y carpetas a la ventana, `se --install-cli`;
  - IME, VoiceOver, escalado y Keychain (entorno con Remember).
- [ ] Prueba manual en **Windows** con el zip de la CI:
  - `simpleeditor.exe` desde el Explorador (sin consola) y `se.exe .` desde la terminal;
  - `se --install-cli` y añadir la carpeta al PATH;
  - IME, Narrador, escalado al 150/200 % y Credential Manager;
  - guardar con el archivo abierto en otra aplicación.
- [ ] Métricas finales repetidas en la máquina de release (`editor_probe`, arranque, reposo) y anotadas en STATUS.

## 3. Construcción de artefactos
- macOS: `./scripts/package-macos.sh` → `dist/SimpleEditor-<v>-macos-arm64.{zip,dmg,sha256}`.
  - Con firma: `SE_CODESIGN_IDENTITY="Developer ID Application: …" SE_NOTARY_PROFILE=<perfil> ./scripts/package-macos.sh`.
- Windows: `pwsh ./scripts/package-windows.ps1` → `dist\SimpleEditor-<v>-windows-x86_64.{zip,sha256}`.
  - Con firma: `SE_SIGN_CERT_THUMBPRINT=<huella>`.
- [ ] Comprueba los hashes de `*.sha256` antes de subir nada.

## 4. Publicación (solo cuando lo pidas)
- [ ] Tag `v<versión>` sobre el commit verificado en CI.
- [ ] Release de GitHub con los zip/dmg/sha256, las notas del `CHANGELOG.md` y las limitaciones conocidas.
- [ ] Mover `[Unreleased]` del CHANGELOG a la versión publicada.

## Limitaciones a declarar en la release
- Sin firma ni notarización, salvo que se aporten las credenciales.
- macOS: «Abrir con…» desde Finder no está soportado (no se tratan los Apple Events de apertura); hay que arrastrar a la ventana o usar `se`.
- Windows: `simpleeditor.exe --help` no muestra nada (programa gráfico); usa `se.exe --help`. El .exe no lleva icono de recurso propio (solo el de la ventana) y no hay instalador MSI (zip portable).
- El lector de pantalla lee la línea actual del editor, no el documento entero.
