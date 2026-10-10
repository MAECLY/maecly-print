## English
Download the file for your computer from the assets below. `SHA256SUMS` lists the checksum of every file.

| Computer | File |
|---|---|
| Windows, Intel or AMD (x64) | `Pluryprint_<version>_windows_x64.exe` |
| Windows on ARM (Snapdragon, ARM64) | `Pluryprint_<version>_windows_arm64.exe` |
| Mac with Apple silicon (M1 and later) | `Pluryprint_<version>_apple_silicon.dmg` |
| Mac with an Intel processor | `Pluryprint_<version>_apple_intel.dmg` |

### macOS will say the app is damaged. It is not.
The app carries no Apple Developer ID signature, so macOS quarantines it and Gatekeeper reports the most misleading message it has. After dragging Pluryprint to Applications, clear the quarantine flag once, in Terminal:

```
xattr -rd com.apple.quarantine /Applications/Pluryprint.app
```

Do it only with a copy you downloaded from this release page: that flag is the check that protects you from a tampered download.

### SmartScreen will interrupt the installer.
The installer is not signed with a Microsoft code-signing certificate, so Windows shows the blue "Windows protected your PC" screen. Choose **More info**, then **Run anyway**. It installs for your user only and asks for no administrator rights.

Updates are verified against Pluryprint's own signing key either way: an update that was not signed by it is refused.

Pluryprint is free software under the GNU AGPL-3.0. The Windows installers include SumatraPDF (GPL-3.0); the source of both is attached to this release.

## Español
Descarga el archivo de tu equipo desde la lista de abajo. `SHA256SUMS` recoge la suma de comprobación de cada archivo.

| Equipo | Archivo |
|---|---|
| Windows, Intel o AMD (x64) | `Pluryprint_<versión>_windows_x64.exe` |
| Windows en ARM (Snapdragon, ARM64) | `Pluryprint_<versión>_windows_arm64.exe` |
| Mac con Apple silicon (M1 y posteriores) | `Pluryprint_<versión>_apple_silicon.dmg` |
| Mac con procesador Intel | `Pluryprint_<versión>_apple_intel.dmg` |

### macOS dirá que la app está dañada. No lo está.
La app no lleva firma de Apple Developer ID, así que macOS la pone en cuarentena y Gatekeeper muestra el mensaje más engañoso que tiene. Después de arrastrar Pluryprint a Aplicaciones, quita la marca de cuarentena una sola vez, en Terminal:

```
xattr -rd com.apple.quarantine /Applications/Pluryprint.app
```

Hazlo solo con una copia descargada de esta página: esa marca es la comprobación que te protege de una descarga manipulada.

### SmartScreen interrumpirá el instalador.
El instalador no está firmado con un certificado de firma de código de Microsoft, así que Windows muestra la pantalla azul "Windows protegió su PC". Elige **Más información** y después **Ejecutar de todas formas**. Se instala solo para tu usuario y no pide permisos de administrador.

Las actualizaciones se verifican siempre contra la clave de firma de Pluryprint: una actualización que no venga firmada por ella se rechaza.

Pluryprint es software libre bajo la GNU AGPL-3.0. Los instaladores de Windows incluyen SumatraPDF (GPL-3.0); el código fuente de ambos va adjunto a esta versión.
