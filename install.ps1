$ver = (Invoke-RestMethod 'https://api.github.com/repos/clawcrew-labs/clawcrew/releases/latest').tag_name.TrimStart('v')
$dst = "$env:USERPROFILE\.clawcrew\bin"
$exe = "$dst\clawcrew.exe"

$current = if (Test-Path $exe) {
    ((& $exe --version 2>$null) | Select-String -Pattern '\d+\.\d+\.\d+').Matches.Value
} else { '' }

if ($current -ne $ver) {
    $url = "https://github.com/clawcrew-labs/clawcrew/releases/download/v$ver/clawcrew-x86_64-pc-windows-msvc.zip"
    New-Item -ItemType Directory -Force -Path $dst | Out-Null
    Invoke-WebRequest -Uri $url -OutFile "$env:TEMP\clawcrew.zip" -UseBasicParsing
    Expand-Archive -Force -Path "$env:TEMP\clawcrew.zip" -DestinationPath $dst
}

$environment = [Environment]
$userPath = $environment::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $dst) {
    $environment::SetEnvironmentVariable('Path', "$dst;$userPath", 'User')
}
if (($env:Path -split ';') -notcontains $dst) {
    $env:Path = "$dst;$env:Path"
}


