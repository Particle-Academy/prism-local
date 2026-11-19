# Build committed HEAD from two distinct source paths with pinned Rust flags.
# This checks same-machine reproducibility; it is not a cross-OS binary match.
$prismRepoRoot = (Get-Location).Path
$prismScratch = Join-Path $prismRepoRoot ("target/repro-check/" + [Guid]::NewGuid().ToString('N'))
$prismSourceA = Join-Path $prismScratch 'source-a'
$prismSourceB = Join-Path $prismScratch 'source-b'
$prismArchive = Join-Path $prismScratch 'source.tar'
New-Item -ItemType Directory -Path $prismSourceA, $prismSourceB -Force -ErrorAction Stop | Out-Null
git archive --format=tar --output=$prismArchive HEAD
if ($LASTEXITCODE -ne 0) { throw 'git archive failed' }
foreach ($prismSource in @($prismSourceA, $prismSourceB)) {
    tar -xf $prismArchive -C $prismSource
    if ($LASTEXITCODE -ne 0) { throw 'source extraction failed' }
    $prismFlags = @("--remap-path-prefix=$prismSource=/source")
    if ($IsWindows) { $prismFlags += @('-C', 'link-arg=/Brepro') }
    $env:CARGO_ENCODED_RUSTFLAGS = [string]::Join([char]0x1f, [string[]]$prismFlags)
    Push-Location $prismSource
    try {
        cargo build -p prism-local --release --locked --offline
        if ($LASTEXITCODE -ne 0) { throw 'reproducible build failed' }
    } finally {
        Pop-Location
    }
}
$prismBinaryName = if ($IsWindows) { 'prism-local.exe' } else { 'prism-local' }
$prismBinaryA = [System.IO.File]::ReadAllBytes((Join-Path $prismSourceA "target/release/$prismBinaryName"))
$prismBinaryB = [System.IO.File]::ReadAllBytes((Join-Path $prismSourceB "target/release/$prismBinaryName"))
if (-not [System.Linq.Enumerable]::SequenceEqual[byte]($prismBinaryA, $prismBinaryB)) {
    throw 'relocated release binaries differ'
}
Write-Output "Relocated release binaries match ($($prismBinaryA.Length) bytes)."
