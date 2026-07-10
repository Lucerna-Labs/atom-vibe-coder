param(
    [switch]$Build,
    [switch]$Restart
)

$ErrorActionPreference = "Stop"
if ($PSVersionTable.PSVersion.Major -ge 7) {
    $PSNativeCommandUseErrorActionPreference = $true
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$Engine = Join-Path $Root "atom-rendering-engine-main"
$Exe = Join-Path $Engine "target\release\math-atoms-native.exe"
. (Join-Path $PSScriptRoot "Native-Process.ps1")

if (-not ("AtomDetachedProcess" -as [type])) {
    Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

public static class AtomDetachedProcess {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct STARTUPINFO {
        public int cb;
        public string lpReserved;
        public string lpDesktop;
        public string lpTitle;
        public int dwX;
        public int dwY;
        public int dwXSize;
        public int dwYSize;
        public int dwXCountChars;
        public int dwYCountChars;
        public int dwFillAttribute;
        public int dwFlags;
        public short wShowWindow;
        public short cbReserved2;
        public IntPtr lpReserved2;
        public IntPtr hStdInput;
        public IntPtr hStdOutput;
        public IntPtr hStdError;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct PROCESS_INFORMATION {
        public IntPtr hProcess;
        public IntPtr hThread;
        public int dwProcessId;
        public int dwThreadId;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateProcessW(
        string applicationName,
        StringBuilder commandLine,
        IntPtr processAttributes,
        IntPtr threadAttributes,
        bool inheritHandles,
        uint creationFlags,
        IntPtr environment,
        string currentDirectory,
        ref STARTUPINFO startupInfo,
        out PROCESS_INFORMATION processInformation);

    [DllImport("kernel32.dll")]
    private static extern bool CloseHandle(IntPtr handle);

    public static int Start(string executable, string workingDirectory) {
        const uint DETACHED_PROCESS = 0x00000008;
        const uint CREATE_NEW_PROCESS_GROUP = 0x00000200;
        const uint CREATE_UNICODE_ENVIRONMENT = 0x00000400;
        const uint CREATE_BREAKAWAY_FROM_JOB = 0x01000000;
        var startup = new STARTUPINFO();
        startup.cb = Marshal.SizeOf(typeof(STARTUPINFO));
        PROCESS_INFORMATION process;
        var flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP |
                    CREATE_UNICODE_ENVIRONMENT | CREATE_BREAKAWAY_FROM_JOB;
        var command = new StringBuilder("\"" + executable + "\"");
        var created = CreateProcessW(executable, command, IntPtr.Zero, IntPtr.Zero, false,
                                     flags, IntPtr.Zero, workingDirectory, ref startup, out process);
        var firstError = created ? 0 : Marshal.GetLastWin32Error();
        if (!created && firstError == 5) {
            flags &= ~CREATE_BREAKAWAY_FROM_JOB;
            command = new StringBuilder("\"" + executable + "\"");
            created = CreateProcessW(executable, command, IntPtr.Zero, IntPtr.Zero, false,
                                     flags, IntPtr.Zero, workingDirectory, ref startup, out process);
        }
        if (!created) {
            var finalError = Marshal.GetLastWin32Error();
            throw new Win32Exception(finalError,
                "Could not create a detached Atom Vibe Coder process; first Win32 error=" + firstError);
        }
        try {
            return process.dwProcessId;
        }
        finally {
            CloseHandle(process.hThread);
            CloseHandle(process.hProcess);
        }
    }
}
'@
}

if ($Restart) {
    Get-Process -Name math-atoms-native -ErrorAction SilentlyContinue | Stop-Process -Force
}

if ($Build -or -not (Test-Path -LiteralPath $Exe)) {
    Push-Location $Engine
    try {
        $env:RUSTFLAGS = "-D warnings"
        cargo build -p math-atoms-native --release
        if ($LASTEXITCODE -ne 0) { throw "native PMRE app build failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }
}

$NativePid = [AtomDetachedProcess]::Start($Exe, $Engine)
$WindowDeadline = [DateTime]::UtcNow.AddSeconds(20)
do {
    Start-Sleep -Milliseconds 250
    $proc = Get-Process -Id $NativePid -ErrorAction SilentlyContinue
    if ($null -eq $proc) { continue }
    $windowHandle = Get-AtomNativeWindowHandle -Process $proc
} while (($null -eq $proc -or $windowHandle -eq 0 -or -not $proc.Responding) -and [DateTime]::UtcNow -lt $WindowDeadline)
if ($null -eq $proc -or $windowHandle -eq 0) {
    throw "Native app launched without a main window handle after 20s"
}
if (-not $proc.Responding) {
    throw "Native app is not responding after launch"
}

$title = Get-AtomNativeWindowTitle -Process $proc
Write-Host "native app launched: pid=$($proc.Id) title=$title"
