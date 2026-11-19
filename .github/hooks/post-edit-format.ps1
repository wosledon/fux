# Fux PostToolUse hook: run `rustfmt --check` on the Rust file that was just edited.
# Advisory only: never modifies files, never blocks. Any error or unknown input exits 0.
# NOTE: keep this file ASCII-only. Windows PowerShell 5.1 reads BOM-less scripts as ANSI,
# which corrupts non-ASCII characters and breaks parsing.
$ErrorActionPreference = 'SilentlyContinue'

function Get-Field($obj, $names) {
  foreach ($n in $names) {
    $p = $obj.PSObject.Properties[$n]
    if ($p -and $p.Value -is [string] -and $p.Value.Trim()) { return $p.Value }
  }
  return $null
}

try {
  $raw = [Console]::In.ReadToEnd()
  if ([string]::IsNullOrWhiteSpace($raw)) { exit 0 }
  $payload = $raw | ConvertFrom-Json

  $file = $null
  foreach ($container in @($payload.tool_input, $payload.toolInput, $payload)) {
    if ($null -eq $container) { continue }
    $file = Get-Field $container @('file_path', 'filePath', 'path', 'file')
    if ($file) { break }
  }

  if (-not $file) { exit 0 }
  if ($file -notmatch '\.rs$') { exit 0 }
  if ($file -notmatch '[\\/](src|tests)[\\/]') { exit 0 }
  if (-not (Test-Path -LiteralPath $file)) { exit 0 }
  if (-not (Get-Command rustfmt -ErrorAction SilentlyContinue)) { exit 0 }

  & rustfmt --edition 2024 --check -- "$file" *> $null
  if ($LASTEXITCODE -eq 0) { exit 0 }

  $msg = "rustfmt found formatting differences in $file. Run 'cargo fmt' before continuing."
  @{ systemMessage = $msg } | ConvertTo-Json -Compress | Write-Output
  exit 0
} catch {
  exit 0
}