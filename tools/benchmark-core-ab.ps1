param(
    [Parameter(Mandatory)][string]$BaselineExecutable,
    [Parameter(Mandatory)][string]$CandidateExecutable,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [ValidateRange(1, 20)][int]$Rounds = 3,
    [ValidateSet('All', 'Runtime', 'SmallHint', 'HintRegressions', 'Search', 'Gate', 'LabelWidths')][string]$Mode = 'All',
    [long]$Affinity = 4,
    [ValidateSet('Normal', 'AboveNormal')][string]$BenchmarkPriority = 'Normal',
    [ValidateRange(0, 100)][double]$MaxRegressionPercent = 3,
    [ValidateRange(0, 1000000)][double]$NoiseFloorNanoseconds = 2
)

$ErrorActionPreference = 'Stop'
$taskBins = @{
    baseline = (Resolve-Path -LiteralPath $BaselineExecutable).Path
    candidate = (Resolve-Path -LiteralPath $CandidateExecutable).Path
}
New-Item -ItemType Directory -Force $OutputDirectory | Out-Null
$taskOutput = (Resolve-Path -LiteralPath $OutputDirectory).Path
if (Get-ChildItem -LiteralPath $taskOutput -Force | Select-Object -First 1) {
    throw 'Use an empty output directory so results from different runs cannot mix.'
}
$taskProcess = [Diagnostics.Process]::GetCurrentProcess()
$taskProcess.ProcessorAffinity = [IntPtr]$Affinity
$taskProcess.PriorityClass = [Diagnostics.ProcessPriorityClass]::AboveNormal
function Invoke-CoreBenchmark([string]$Executable, [string]$Argument, [string]$OutputPath) {
    $taskStart = [Diagnostics.ProcessStartInfo]::new($Executable, $Argument)
    $taskStart.UseShellExecute = $false
    $taskStart.CreateNoWindow = $true
    $taskStart.RedirectStandardOutput = $true
    $taskChild = [Diagnostics.Process]::new()
    $taskChild.StartInfo = $taskStart
    $taskStarted = $false
    try {
        [void]$taskChild.Start()
        $taskStarted = $true
        if (!$taskChild.HasExited) { $taskChild.PriorityClass = $BenchmarkPriority }
        & {
            while ($null -ne ($taskLine = $taskChild.StandardOutput.ReadLine())) {
                Write-Output $taskLine
            }
        } | Tee-Object -FilePath $OutputPath
        $taskChild.WaitForExit()
        if ($taskChild.ExitCode -ne 0) { throw "benchmark failed: $Executable" }
    } finally {
        if ($taskStarted -and !$taskChild.HasExited) { $taskChild.Kill(); $taskChild.WaitForExit() }
        $taskChild.Dispose()
    }
}
@{
    utc = [DateTime]::UtcNow.ToString('o')
    processor = (Get-ItemProperty 'HKLM:/HARDWARE/DESCRIPTION/System/CentralProcessor/0' -Name ProcessorNameString).ProcessorNameString.Trim()
    logical_processors = [Environment]::ProcessorCount
    affinity = $Affinity
    timer_resolution_ns = 1e9 / [Diagnostics.Stopwatch]::Frequency
    runner_priority = $taskProcess.PriorityClass.ToString()
    benchmark_priority = $BenchmarkPriority
    mode = $Mode
    binaries = @($taskBins.GetEnumerator() | ForEach-Object {
        @{ name = $_.Key; path = $_.Value; sha256 = (Get-FileHash -LiteralPath $_.Value -Algorithm SHA256).Hash }
    })
} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $taskOutput 'environment.json')

for ($round = 1; $round -le $Rounds; $round++) {
    $order = if ($round % 2 -eq 1) { @('baseline', 'candidate') } else { @('candidate', 'baseline') }
    foreach ($name in $order) {
        Write-Output "round=$round version=$name"
        if ($Mode -in @('HintRegressions', 'Search', 'Gate', 'LabelWidths')) {
            $taskArgument = if ($Mode -eq 'LabelWidths') { '--label-widths' } elseif ($Mode -eq 'Search') { '--search-regressions' } else { '--regressions' }
            Invoke-CoreBenchmark $taskBins[$name] $taskArgument (Join-Path $taskOutput "$round-$name-hint.txt")
            if ($Mode -ne 'Gate') { continue }
        }
        if ($Mode -ne 'Runtime') {
            if ($Mode -in @('SmallHint', 'Gate')) {
                Invoke-CoreBenchmark $taskBins[$name] '--hint-small' (Join-Path $taskOutput "$round-$name-core.txt")
            } else {
                Invoke-CoreBenchmark $taskBins[$name] '' (Join-Path $taskOutput "$round-$name-core.txt")
            }
        }
        if ($Mode -ne 'SmallHint') {
            Invoke-CoreBenchmark $taskBins[$name] '--runtime' (Join-Path $taskOutput "$round-$name-runtime.txt")
        }
    }
}

& python (Join-Path $PSScriptRoot 'compare-core-benchmarks.py') $taskOutput `
    --max-regression-percent $MaxRegressionPercent --noise-floor-ns $NoiseFloorNanoseconds
if ($LASTEXITCODE -ne 0) {
    throw 'Performance comparison needs review; see comparison.json and retained raw rounds.'
}
