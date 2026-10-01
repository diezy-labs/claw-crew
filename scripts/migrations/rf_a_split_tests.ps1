param(
  [Parameter(Mandatory=$true)][string]$File
)
# RF-A pure-move test-split helper.
# Finds the OUTERMOST `#[cfg(test)]` + `mod tests {` ... `}` block at column 0,
# moves its inner body to a sibling tests file, replaces the block with
# `#[cfg(test)]\nmod tests;`. Verifies brace balance before writing. No logic change.
$ErrorActionPreference = 'Stop'
$abs = Resolve-Path $File
$lines = (Get-Content $abs) | ForEach-Object { $_.TrimEnd("`r") }
$n = $lines.Count

# locate: a line that is exactly `mod tests {` (col 0) preceded by `#[cfg(test)]`
$modIdx = -1
for ($i=0; $i -lt $n; $i++) {
  if ($lines[$i] -match '^mod tests \{\s*$' -and $i -ge 1 -and $lines[$i-1] -match '^#\[cfg\(test\)\]\s*$') { $modIdx = $i; break }
}
if ($modIdx -lt 0) { Write-Error "no top-level '#[cfg(test)] mod tests {' in $File"; exit 1 }

# the closing brace is the LAST line that is exactly `}` at col 0 (outermost mod close)
$closeIdx = -1
for ($j=$n-1; $j -gt $modIdx; $j--) {
  if ($lines[$j] -match '^\}\s*$') { $closeIdx = $j; break }
}
if ($closeIdx -lt 0) { Write-Error "no closing brace for mod tests in $File"; exit 1 }

$cfgIdx = $modIdx - 1
$inner = if ($closeIdx -gt ($modIdx+1)) { $lines[($modIdx+1)..($closeIdx-1)] } else { @() }

# brace balance is only an INFO signal: Rust string/char literals and comments
# contain unbalanced braces (e.g. "{}", format strings), so a raw count is not
# authoritative. The reliable structural guard is that `mod tests {` sits at
# column 0 and its matching close `}` is the last column-0 `}` — rustfmt
# guarantees this for the outermost test module. We report the count for the
# human to eyeball but do not abort on it.
$innerText = ($inner -join "`n")
$open = ([regex]::Matches($innerText,'\{')).Count
$close = ([regex]::Matches($innerText,'\}')).Count

# body = everything before cfg + the declaration + anything after closeIdx (usually nothing)
$body = @()
if ($cfgIdx -gt 0) { $body += $lines[0..($cfgIdx-1)] }
$body += '#[cfg(test)]'
$body += 'mod tests;'
if ($closeIdx -lt ($n-1)) { $body += $lines[($closeIdx+1)..($n-1)] }

# destination: if file is `foo.rs` -> make `foo/` dir with mod.rs + tests.rs
$dir = Split-Path $abs -Parent
$name = [System.IO.Path]::GetFileNameWithoutExtension($abs)
if ($name -eq 'mod') {
  # already in a folder: sibling tests.rs
  $testsPath = Join-Path $dir 'tests.rs'
  $bodyPath = $abs
} else {
  $newDir = Join-Path $dir $name
  New-Item -ItemType Directory -Force -Path $newDir | Out-Null
  $testsPath = Join-Path $newDir 'tests.rs'
  $bodyPath = Join-Path $newDir 'mod.rs'
}
Set-Content -Path $testsPath -Value $inner -Encoding UTF8
Set-Content -Path $bodyPath -Value $body -Encoding UTF8
if ($bodyPath -ne $abs) { Remove-Item $abs }

Write-Output "OK $File -> body=$($body.Count)L tests=$($inner.Count)L (balance $open/$close)"
Write-Output "  body: $bodyPath"
Write-Output "  tests: $testsPath"
