$ErrorActionPreference = "Stop"

$projectRoot = $PSScriptRoot
$target = "x86_64-pc-windows-gnu"
$scriptPath = Join-Path $projectRoot "installer\DaxaControl.iss"
$outputDirectory = Join-Path $projectRoot "dist"

$iscc = $env:ISCC
if (-not $iscc) {
    $programFilesX86 = [Environment]::GetEnvironmentVariable("ProgramFiles(x86)")
    $programFiles = [Environment]::GetFolderPath("ProgramFiles")
    $candidates = @(
        (Join-Path $programFiles "Inno Setup 7\ISCC.exe"),
        (Join-Path $programFilesX86 "Inno Setup 7\ISCC.exe"),
        (Join-Path $programFiles "Inno Setup 6\ISCC.exe"),
        (Join-Path $programFilesX86 "Inno Setup 6\ISCC.exe")
    )
    $iscc = $candidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}

if (-not $iscc -or -not (Test-Path -LiteralPath $iscc)) {
    throw "Inno Setup Compiler was not found. Install Inno Setup 6 or 7, or set ISCC to the compiler path."
}

New-Item -ItemType Directory -Force -Path $outputDirectory | Out-Null
Push-Location $projectRoot
try {
    & cargo build --release --target $target
    if ($LASTEXITCODE -ne 0) {
        throw "Rust release build failed with exit code $LASTEXITCODE."
    }

    & $iscc $scriptPath
    if ($LASTEXITCODE -ne 0) {
        throw "Inno Setup compilation failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}
