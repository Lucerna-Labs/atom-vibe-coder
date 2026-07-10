$ErrorActionPreference = "Stop"

function Convert-ToAtomManifestField([string]$Value) {
    if ($null -eq $Value) { return "" }
    return ($Value -replace '[\t\r\n]+', ' ').Trim()
}

function Update-AtomArtifactManifest {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Status,
        [string]$Output = "",
        [string]$Source = "",
        [string]$Exe = "",
        [string]$Artifact = ""
    )

    $fullPath = [System.IO.Path]::GetFullPath($Path)
    $directory = Split-Path -Parent $fullPath
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $lockPath = "$fullPath.lock"
    $lock = $null
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ($null -eq $lock -and [DateTime]::UtcNow -lt $deadline) {
        try {
            $lock = [System.IO.FileStream]::new(
                $lockPath,
                [System.IO.FileMode]::OpenOrCreate,
                [System.IO.FileAccess]::ReadWrite,
                [System.IO.FileShare]::None
            )
        }
        catch [System.IO.IOException] {
            Start-Sleep -Milliseconds 25
        }
    }
    if ($null -eq $lock) {
        throw "Timed out locking artifact manifest: $fullPath"
    }

    $temp = "$fullPath.$PID.$([Guid]::NewGuid().ToString('N')).tmp"
    $backup = "$fullPath.$PID.$([Guid]::NewGuid().ToString('N')).bak"
    try {
        $header = "name`tstatus`toutput`tsource`texe`tartifact"
        $safeName = Convert-ToAtomManifestField $Name
        if ([string]::IsNullOrWhiteSpace($safeName)) {
            throw "Artifact manifest name cannot be empty"
        }
        $lines = [System.Collections.Generic.List[string]]::new()
        $lines.Add($header)
        if (Test-Path -LiteralPath $fullPath) {
            foreach ($line in [System.IO.File]::ReadAllLines($fullPath)) {
                if ([string]::IsNullOrWhiteSpace($line) -or $line -eq $header) { continue }
                $existingName = ($line -split "`t", 2)[0]
                if ($existingName -ne $safeName) { $lines.Add($line) }
            }
        }
        $fields = @(
            $safeName,
            (Convert-ToAtomManifestField $Status),
            (Convert-ToAtomManifestField $Output),
            (Convert-ToAtomManifestField $Source),
            (Convert-ToAtomManifestField $Exe),
            (Convert-ToAtomManifestField $Artifact)
        )
        $lines.Add($fields -join "`t")
        [System.IO.File]::WriteAllLines($temp, $lines)
        if (Test-Path -LiteralPath $fullPath) {
            [System.IO.File]::Replace($temp, $fullPath, $backup, $true)
            Remove-Item -LiteralPath $backup -Force
        }
        else {
            [System.IO.File]::Move($temp, $fullPath)
        }
    }
    finally {
        Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $backup -Force -ErrorAction SilentlyContinue
        $lock.Dispose()
    }
}
