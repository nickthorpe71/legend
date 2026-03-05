$ErrorActionPreference = "Stop"

$Repo = "nickthorpe71/legend"
$Binary = "legend-windows-x86_64.exe"
$InstallDir = if ($env:LEGEND_INSTALL_DIR) { $env:LEGEND_INSTALL_DIR } else { "$env:USERPROFILE\.local\bin" }

Write-Host "Fetching latest release..."
$Release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
$Tag = $Release.tag_name

if (-not $Tag) {
    Write-Error "Could not determine latest release."
    exit 1
}

Write-Host "Latest release: $Tag"

$Url = "https://github.com/$Repo/releases/download/$Tag/$Binary"

if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$Dest = Join-Path $InstallDir "legend.exe"
Write-Host "Downloading $Binary..."
Invoke-WebRequest -Uri $Url -OutFile $Dest -UseBasicParsing

Write-Host "Installed legend $Tag to $Dest"

# Check if install dir is in PATH
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    Write-Host ""
    Write-Host "Adding $InstallDir to your user PATH..."
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
    Write-Host "Done. Restart your terminal for PATH changes to take effect."
}
