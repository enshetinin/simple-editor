# Builds a portable zip of SimpleEditor for Windows x86_64 in dist\.
#
#   se.exe            command-line program (console): `se .`, `se --help`,
#                     `se --install-cli`
#   simpleeditor.exe  GUI launcher (no console window) for Explorer, shortcuts
#                     and the Start menu
#
# Signing: unsigned by default. If SE_SIGN_CERT_THUMBPRINT is set, both .exe
# files are signed with signtool using the owner's certificate (not stored in
# this repository). Nothing is uploaded or published.
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

$version = (cargo metadata --no-deps --format-version 1 --locked | ConvertFrom-Json).packages[0].version
$name = "SimpleEditor-$version-windows-x86_64"
$dist = Join-Path 'dist' $name

cargo build --release --locked --bins
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

if (Test-Path $dist) { Remove-Item -Recurse -Force $dist }
New-Item -ItemType Directory -Force $dist | Out-Null
Copy-Item target\release\se.exe, target\release\simpleeditor.exe, LICENSE, THIRD_PARTY_LICENSES.md, README.md $dist
Copy-Item assets\icon\simpleeditor.ico $dist

if ($env:SE_SIGN_CERT_THUMBPRINT) {
    foreach ($exe in @("$dist\se.exe", "$dist\simpleeditor.exe")) {
        signtool sign /sha1 $env:SE_SIGN_CERT_THUMBPRINT /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 $exe
        if ($LASTEXITCODE -ne 0) { throw "signing $exe failed" }
    }
    $signed = 'signed'
} else {
    $signed = 'unsigned (SmartScreen will warn)'
}

$zip = "dist\$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $dist -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
"$hash  $name.zip" | Out-File -Encoding ascii "dist\$name.sha256"

# Smoke test of the packaged CLI.
& "$dist\se.exe" --version
if ($LASTEXITCODE -ne 0) { throw 'se.exe --version failed' }

Write-Output "Built $zip ($signed)"
