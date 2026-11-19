param(
    [Parameter(Mandatory = $true)][string]$BinaryPath,
    [Parameter(Mandatory = $true)][string]$OutputPath
)

# These are synthetic requests only; no caller data is used or logged.
$prismRequests = @(
    '{"jsonrpc":"2.0","id":1,"method":"hello","params":{"threads":1,"deadlineMs":30000,"maxMemoryMb":4}}',
    '{"jsonrpc":"2.0","id":2,"method":"stats"}',
    '{"jsonrpc":"2.0","id":3,"method":"shutdown"}'
)
$prismExpected = @(
    '{"jsonrpc":"2.0","id":1,"result":{"threads":1,"deadlineMs":30000,"maxMemoryMb":4,"memoryScope":"managedPayload","processMemoryLimitMb":260,"processMemoryScope":"platformHardLimit","inferenceAvailable":false}}',
    '{"jsonrpc":"2.0","id":2,"result":{"threads":1,"deadlineMs":30000,"maxMemoryMb":4,"memoryScope":"managedPayload","processMemoryLimitMb":260,"processMemoryScope":"platformHardLimit","inferenceAvailable":false}}',
    '{"jsonrpc":"2.0","id":3,"result":null}'
)

function Write-PrismFrame([System.IO.Stream]$Stream, [string]$Value) {
    $payload = [System.Text.Encoding]::UTF8.GetBytes($Value)
    $length = [System.BitConverter]::GetBytes([uint32]$payload.Length)
    if ([System.BitConverter]::IsLittleEndian) { [array]::Reverse($length) }
    $Stream.Write($length, 0, $length.Length)
    $Stream.Write($payload, 0, $payload.Length)
}

$prismStart = [System.Diagnostics.ProcessStartInfo]::new()
$prismStart.FileName = [System.IO.Path]::GetFullPath($BinaryPath)
$prismStart.ArgumentList.Add('--max-memory-mb')
$prismStart.ArgumentList.Add('4')
$prismStart.UseShellExecute = $false
$prismStart.CreateNoWindow = $true
$prismStart.RedirectStandardInput = $true
$prismStart.RedirectStandardOutput = $true
$prismStart.RedirectStandardError = $true
$prismProcess = [System.Diagnostics.Process]::new()
$prismProcess.StartInfo = $prismStart
if (-not $prismProcess.Start()) { throw 'Could not start sidecar' }
try {
    foreach ($request in $prismRequests) {
        Write-PrismFrame $prismProcess.StandardInput.BaseStream $request
    }
    $prismProcess.StandardInput.Close()
    # The three expected responses fit comfortably within a pipe. A larger or
    # stalled response cannot block the gate indefinitely.
    if (-not $prismProcess.WaitForExit(10000)) {
        $prismProcess.Kill()
        $prismProcess.WaitForExit()
        throw 'Sidecar protocol fixture timed out'
    }
    if ($prismProcess.ExitCode -ne 0) { throw 'Sidecar protocol fixture failed' }
    $actual = [System.IO.MemoryStream]::new()
    $expected = [System.IO.MemoryStream]::new()
    try {
        $prismProcess.StandardOutput.BaseStream.CopyTo($actual)
        foreach ($response in $prismExpected) {
            Write-PrismFrame $expected $response
        }
        $actualBytes = $actual.ToArray()
        if (-not [System.Linq.Enumerable]::SequenceEqual[byte]($actualBytes, $expected.ToArray())) {
            throw 'Sidecar protocol fixture differed from expected bytes'
        }
        [System.IO.File]::WriteAllBytes([System.IO.Path]::GetFullPath($OutputPath), $actualBytes)
        Write-Output "Sidecar protocol fixture matched ($($actualBytes.Length) bytes)."
    } finally {
        $actual.Dispose()
        $expected.Dispose()
    }
} finally {
    if (-not $prismProcess.HasExited) {
        $prismProcess.Kill()
        $prismProcess.WaitForExit()
    }
    $prismProcess.Dispose()
}
