$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ScriptDir = $PSScriptRoot
$ProjectRoot = Split-Path -Parent $ScriptDir
$ProtoDir = Join-Path $ProjectRoot "proto"
$ProtoLockFile = Join-Path $ProtoDir "buf.lock"
$ProtoVendorDir = Join-Path $ProtoDir "vendor"

if (-not (Get-Command buf -ErrorAction SilentlyContinue)) {
  throw "buf is required to generate vendored Protobuf dependencies"
}
if (-not (Get-Command yq -ErrorAction SilentlyContinue)) {
  throw "yq is required to read $ProtoLockFile"
}

$StagingDir = $null
Push-Location $ProtoDir
try {
  # Validate that the checked-in lock file resolves the dependencies declared
  # by the Buf workspace before exporting commit-qualified module references.
  & buf dep graph --format json | Out-Null
  if ($LASTEXITCODE -ne 0) {
    throw "failed to resolve dependencies from $ProtoLockFile"
  }
  $DependencyRefs = @(& yq -r '.deps[] | .name + ":" + .commit' $ProtoLockFile)
  if ($LASTEXITCODE -ne 0) {
    throw "failed to read dependencies from $ProtoLockFile"
  }
  $DependencyRefs = @($DependencyRefs | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
  if ($DependencyRefs.Count -eq 0) {
    throw "no dependencies found in $ProtoLockFile"
  }

  $StagingDir = Join-Path $ProtoDir (".vendor." + [Guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Path $StagingDir | Out-Null

  foreach ($DependencyRef in $DependencyRefs) {
    # Keep upstream license and documentation files with the checked-in schemas.
    & buf export $DependencyRef --all --output $StagingDir
    if ($LASTEXITCODE -ne 0) {
      throw "failed to export Protobuf dependency $DependencyRef"
    }
  }

  foreach ($RequiredFile in @("google/api/annotations.proto", "google/api/http.proto")) {
    $RequiredPath = Join-Path $StagingDir $RequiredFile
    if (-not (Test-Path -LiteralPath $RequiredPath -PathType Leaf)) {
      throw "dependency export did not produce $RequiredFile"
    }
  }

  if (Test-Path -LiteralPath $ProtoVendorDir) {
    Remove-Item -LiteralPath $ProtoVendorDir -Recurse -Force
  }
  Move-Item -LiteralPath $StagingDir -Destination $ProtoVendorDir
  $StagingDir = $null
} finally {
  Pop-Location
  if ($null -ne $StagingDir -and (Test-Path -LiteralPath $StagingDir)) {
    Remove-Item -LiteralPath $StagingDir -Recurse -Force
  }
}

Write-Output "generated vendored Protobuf dependencies in $ProtoVendorDir"
