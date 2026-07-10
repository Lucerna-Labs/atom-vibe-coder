$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Exe = Join-Path $Engine "target\release\math-atoms-native.exe"
$TestStoreDir = Join-Path ([System.IO.Path]::GetTempPath()) ("math-atoms-idle-cpu-" + [Guid]::NewGuid().ToString("N"))
$OriginalStoreDir = $env:MATH_ATOMS_STORE_DIR
. (Join-Path $PSScriptRoot "Native-Process.ps1")
$proc = $null

if (-not ("MathAtomsIdleCursor" -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class MathAtomsIdleCursor {
    [StructLayout(LayoutKind.Sequential)]
    private struct RECT { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")]
    public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    private static extern bool GetWindowRect(IntPtr window, out RECT rect);
    [DllImport("user32.dll")]
    private static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr GetProp(IntPtr window, string name);
    [DllImport("user32.dll")]
    public static extern bool ShowWindow(IntPtr window, int command);
    [DllImport("user32.dll")]
    public static extern bool IsIconic(IntPtr window);

    public static bool MoveOutside(IntPtr window) {
        RECT rect;
        if (!GetWindowRect(window, out rect)) return false;
        int left = GetSystemMetrics(76);
        int top = GetSystemMetrics(77);
        int right = left + GetSystemMetrics(78) - 1;
        int bottom = top + GetSystemMetrics(79) - 1;
        int[,] candidates = { { left, top }, { right, top }, { left, bottom }, { right, bottom } };
        for (int index = 0; index < candidates.GetLength(0); index++) {
            int x = candidates[index, 0];
            int y = candidates[index, 1];
            if (x < rect.Left || x >= rect.Right || y < rect.Top || y >= rect.Bottom) {
                return SetCursorPos(x, y);
            }
        }
        return false;
    }

    public static long RenderCount(IntPtr window) {
        long encoded = GetProp(window, "MathAtomsRenderCount").ToInt64();
        return encoded == 0 ? -1 : encoded - 1;
    }
}
'@
}

try {
    $env:MATH_ATOMS_STORE_DIR = $TestStoreDir
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
    Push-Location $Engine
    try {
        $env:RUSTFLAGS = "-D warnings"
        cargo build -p math-atoms-native --release
        if ($LASTEXITCODE -ne 0) { throw "native PMRE app build failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }

    $proc = Start-AtomNativeProcess -FilePath $Exe -WorkingDirectory $Engine
    $deadline = [DateTime]::UtcNow.AddSeconds(20)
    do {
        Start-Sleep -Milliseconds 250
        $proc = Get-Process -Id $proc.Id -ErrorAction Stop
        $windowHandle = Get-AtomNativeWindowHandle -Process $proc
    } while ($windowHandle -eq 0 -and [DateTime]::UtcNow -lt $deadline)
    if ($windowHandle -eq 0) { throw "native idle CPU gate did not find the app window" }

    [MathAtomsIdleCursor]::ShowWindow($windowHandle, 6) | Out-Null
    $minimizeDeadline = [DateTime]::UtcNow.AddSeconds(5)
    while (-not [MathAtomsIdleCursor]::IsIconic($windowHandle) -and [DateTime]::UtcNow -lt $minimizeDeadline) {
        Start-Sleep -Milliseconds 50
    }
    if (-not [MathAtomsIdleCursor]::IsIconic($windowHandle)) {
        throw "native idle CPU gate could not minimize the app window"
    }
    Start-Sleep -Seconds 2
    $proc = Get-Process -Id $proc.Id -ErrorAction Stop
    $beforeRenders = [MathAtomsIdleCursor]::RenderCount($windowHandle)
    if ($beforeRenders -lt 1) { throw "native idle CPU gate could not read render telemetry" }
    $before = $proc.TotalProcessorTime.TotalSeconds
    $sampleSeconds = 5
    Start-Sleep -Seconds $sampleSeconds
    $proc = Get-Process -Id $proc.Id -ErrorAction Stop
    $cpuSeconds = $proc.TotalProcessorTime.TotalSeconds - $before
    $title = Get-AtomNativeWindowTitle -Process $proc
    $afterRenders = [MathAtomsIdleCursor]::RenderCount($windowHandle)
    $renderDelta = $afterRenders - $beforeRenders
    if ($renderDelta -ne 0) {
        throw "native idle repaint gate rendered $renderDelta uncached frames during the idle sample. Title: $title"
    }
    if ($cpuSeconds -gt 1.5) {
        throw "native idle CPU exceeded budget: $([Math]::Round($cpuSeconds, 3)) CPU seconds over $sampleSeconds wall seconds. Title: $title"
    }
    Write-Host "native idle CPU ok: $([Math]::Round($cpuSeconds, 3)) CPU seconds over $sampleSeconds wall seconds renders=$renderDelta title=$title"
}
finally {
    if ($null -ne $proc) {
        Get-Process -Id $proc.Id -ErrorAction SilentlyContinue | Stop-Process -Force
    }
    $env:MATH_ATOMS_STORE_DIR = $OriginalStoreDir
    Remove-Item -LiteralPath $TestStoreDir -Recurse -Force -ErrorAction SilentlyContinue
}
