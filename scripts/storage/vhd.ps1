# VHD lifecycle for the W38 storage-volume tests. No Hyper-V needed: diskpart
# creates/attaches/detaches the VHD, the built-in Storage module partitions
# and formats it, and a dummy file fills it. Everything lives under
# scripts/storage/ so nothing real is touched.
#
# Usage:
#   .\vhd.ps1 new [-FileSystem NTFS|FAT32] [-Letter W] [-SizeMB 128]
#   .\vhd.ps1 mount [-Letter W]
#   .\vhd.ps1 dismount [-Letter W]
#   .\vhd.ps1 remove [-Letter W]
#   .\vhd.ps1 free [-Letter W]
#   .\vhd.ps1 fill [-LeaveBytes N] [-Letter W]   # grow W:\dummy.dat until free <= N
#   .\vhd.ps1 unfill [-Letter W]
param(
    [Parameter(Position = 0)][string]$cmd,
    [ValidateSet('NTFS', 'FAT32')][string]$FileSystem = 'NTFS',
    [string]$Letter = 'W',
    [int]$SizeMB = 128,
    [long]$LeaveBytes = 4096
)
$ErrorActionPreference = 'Stop'

$vhd = Join-Path $PSScriptRoot "rbf-test-$Letter.vhd"

function Invoke-Diskpart([string]$scriptText) {
    $scriptFile = Join-Path $PSScriptRoot 'diskpart.tmp.txt'
    [System.IO.File]::WriteAllText($scriptFile, $scriptText)
    $out = & diskpart /s $scriptFile 2>&1
    $code = $LASTEXITCODE
    Remove-Item -Force $scriptFile -ErrorAction SilentlyContinue
    if ($code -ne 0) { throw "diskpart failed: $($out -join '; ')" }
    return $out
}

function New-TestVhd {
    if (Test-Path $vhd) { Remove-Item -Force $vhd }
    Invoke-Diskpart @"
create vdisk file="$vhd" maximum=$SizeMB type=fixed
select vdisk file="$vhd"
attach vdisk
"@ | Out-Null
    Start-Sleep -Seconds 2
    # The freshly attached disk is the smallest one.
    $disk = Get-Disk | Sort-Object Size | Select-Object -First 1
    if (-not $disk) { throw 'no disk after attach' }
    $n = $disk.Number
    if ($disk.PartitionStyle -eq 'RAW') {
        Initialize-Disk -Number $n -PartitionStyle MBR | Out-Null
    }
    New-Partition -DiskNumber $n -UseMaximumSize -DriveLetter $Letter | Out-Null
    Format-Volume -DriveLetter $Letter -FileSystem $FileSystem -NewFileSystemLabel "RBFSTOR-$Letter" -Confirm:$false | Out-Null
    Write-Host "VHD mounted at ${Letter}: ($FileSystem, disk $n)"
}

function Mount-TestVhd {
    if (-not (Test-Path $vhd)) { throw "vhd missing: $vhd" }
    Invoke-Diskpart @"
select vdisk file="$vhd"
attach vdisk
"@ | Out-Null
    Start-Sleep -Seconds 2
    $disk = Get-Disk | Sort-Object Size | Select-Object -First 1
    if (-not $disk) { throw 'no disk after attach' }
    $n = $disk.Number
    if ($disk.PartitionStyle -eq 'RAW') {
        Initialize-Disk -Number $n -PartitionStyle MBR | Out-Null
    }
    $part = Get-Partition -DiskNumber $n | Select-Object -First 1
    if ($part -and -not $part.DriveLetter) {
        Set-Partition -PartitionNumber $part.PartitionNumber -DiskNumber $n -NewDriveLetter $Letter
    }
    if (-not (Get-Volume -DriveLetter $Letter -ErrorAction SilentlyContinue)) {
        throw "volume ${Letter}: did not come back after attach"
    }
    Write-Host "VHD re-mounted at ${Letter}: (disk $n)"
}

function Dismount-TestVhd {
    if (-not (Test-Path $vhd)) { return }
    # diskpart detach refuses while the volume has open handles (the harness
    # holds the store open), so take the disk OFFLINE first — that force-
    # disconnects the volume and invalidates the handles — then detach.
    $disk = Get-Disk | Sort-Object Size | Select-Object -First 1
    if ($disk) {
        Set-Disk -Number $disk.Number -IsOffline $true -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 2
    }
    Invoke-Diskpart @"
select vdisk file="$vhd"
detach vdisk
"@ | Out-Null
    Start-Sleep -Seconds 2
    if (Get-Volume -DriveLetter $Letter -ErrorAction SilentlyContinue) {
        Write-Warning "volume ${Letter}: still present after detach"
    } else {
        Write-Host "VHD dismounted (${Letter}: gone)"
    }
}

function Remove-TestVhd {
    # Detach and delete every VHD under scripts/storage (robust against
    # leftovers from earlier naming schemes).
    Get-ChildItem (Join-Path $PSScriptRoot '*.vhd') -ErrorAction SilentlyContinue | ForEach-Object {
        try {
            Invoke-Diskpart "select vdisk file=$($_.FullName)`ndetach vdisk" | Out-Null
            Start-Sleep -Seconds 1
        } catch {
            Write-Warning "detach of $($_.FullName) failed: $($_.Exception.Message)"
        }
        Remove-Item -Force $_.FullName -ErrorAction SilentlyContinue
        Write-Host "VHD removed ($($_.FullName))"
    }
    Dismount-TestVhd
}

function Get-TestFree {
    $vol = Get-Volume -DriveLetter $Letter -ErrorAction SilentlyContinue
    if (-not $vol) { return -1 }
    return $vol.SizeRemaining
}

function Fill-TestVolume {
    $vol = Get-Volume -DriveLetter $Letter
    if (-not $vol) { throw "no volume ${Letter}:" }
    $dummy = "${Letter}:\dummy.dat"
    $fs = [System.IO.File]::Open($dummy, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
    try {
        # Allocate in one SetLength (fast), then converge on the target in
        # coarse steps. NTFS reserves ~135KB for metadata (an unavoidable
        # floor); FAT32 can be driven much closer to zero.
        $free = $vol.SizeRemaining
        if ($free -gt $LeaveBytes) {
            $fs.SetLength($free - $LeaveBytes - 2MB)
            $fs.Flush($true)
        }
        for ($i = 0; $i -lt 40; $i++) {
            $free = (Get-Volume -DriveLetter $Letter).SizeRemaining
            if ($free -le $LeaveBytes + 131072) { break }
            $fs.SetLength($fs.Length + ($free - $LeaveBytes - 131072))
            $fs.Flush($true)
        }
    } finally {
        $fs.Close()
    }
    $free = (Get-Volume -DriveLetter $Letter).SizeRemaining
    $fsName = (Get-Volume -DriveLetter $Letter).FileSystem
    Write-Host "filled to $free bytes free (leave=$LeaveBytes, fs=$fsName)"
}

function Unfill-TestVolume {
    Remove-Item "${Letter}:\dummy.dat" -Force -ErrorAction SilentlyContinue
    Write-Host "dummy removed; free=$((Get-Volume -DriveLetter $Letter).SizeRemaining)"
}

switch ($cmd) {
    'new' { New-TestVhd }
    'mount' { Mount-TestVhd }
    'dismount' { Dismount-TestVhd }
    'remove' { Remove-TestVhd }
    'free' { Write-Output (Get-TestFree) }
    'fill' { Fill-TestVolume }
    'unfill' { Unfill-TestVolume }
    default { throw "unknown cmd: $cmd" }
}