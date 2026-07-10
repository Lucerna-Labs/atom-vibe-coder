if (-not ("AtomNativeWindowText" -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class AtomNativeWindowText {
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr state);
    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr state);
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextLengthW(IntPtr hWnd);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int count);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassNameW(IntPtr hWnd, StringBuilder text, int count);

    public static IntPtr FindVisibleWindow(int processId) {
        IntPtr found = IntPtr.Zero;
        EnumWindows((hWnd, state) => {
            uint owner;
            GetWindowThreadProcessId(hWnd, out owner);
            if (owner == (uint)processId && IsWindowVisible(hWnd)) {
                var className = new StringBuilder(128);
                GetClassNameW(hWnd, className, className.Capacity);
                if (className.ToString() == "math_atoms_native_window") {
                    found = hWnd;
                    return false;
                }
                if (found == IntPtr.Zero) found = hWnd;
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
}

function Start-AtomNativeProcess {
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [string]$StdOutLog = "",
        [string]$StdErrLog = ""
    )

    foreach ($log in @($StdOutLog, $StdErrLog)) {
        if (-not [string]::IsNullOrWhiteSpace($log)) {
            $parent = Split-Path -Parent $log
            if (-not [string]::IsNullOrWhiteSpace($parent)) {
                New-Item -ItemType Directory -Path $parent -Force | Out-Null
            }
            [System.IO.File]::WriteAllText($log, "")
        }
    }

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FilePath
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    if (-not $process.Start()) {
        throw "Native process did not start: $FilePath"
    }
    return $process
}

function Get-AtomNativeWindowTitle {
    param([Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process)

    $handle = Get-AtomNativeWindowHandle -Process $Process
    if ($handle -eq [IntPtr]::Zero) {
        return ""
    }
    $length = [AtomNativeWindowText]::GetWindowTextLengthW($handle)
    $text = [System.Text.StringBuilder]::new([Math]::Max(2, $length + 1))
    [void][AtomNativeWindowText]::GetWindowTextW($handle, $text, $text.Capacity)
    return $text.ToString()
}

function Get-AtomNativeWindowHandle {
    param([Parameter(Mandatory = $true)][System.Diagnostics.Process]$Process)

    $Process.Refresh()
    return [AtomNativeWindowText]::FindVisibleWindow($Process.Id)
}
