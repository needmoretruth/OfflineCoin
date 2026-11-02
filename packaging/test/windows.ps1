# Runs the real installer silently, starts the app from its Start menu shortcut, mines, and removes
# the app, which must leave the wallet behind.
param(
    [Parameter(Mandatory)][string]$Setup,
    [Parameter(Mandatory)][string]$Terminal,
    [Parameter(Mandatory)][string]$Shots
)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$mesa = '26.2.5'
$waitSeconds = 60

Add-Type -AssemblyName System.Drawing, System.Windows.Forms

$manifest = Select-String -Path $Setup -SimpleMatch 'level="asInvoker"' -Quiet
if (-not $manifest) { throw 'The installer asks for more than an ordinary user has.' }

$log = Join-Path $Shots 'setup.log'
$run = Start-Process -FilePath $Setup -PassThru -Wait `
    -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$log`""
if ($run.ExitCode -ne 0) { throw "The installer ended with code $($run.ExitCode)." }

$folder = Join-Path $env:LOCALAPPDATA 'Programs\OLC Core'
$exe = Join-Path $folder 'olc-core.exe'
$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'OLC Core.lnk'
$desk = Join-Path ([Environment]::GetFolderPath('Desktop')) 'OLC Core.lnk'
foreach ($path in @($exe, (Join-Path $folder 'espflash.exe'), $menu, $desk)) {
    if (-not (Test-Path $path)) { throw "Not installed: $path" }
}
$shell = New-Object -ComObject WScript.Shell
foreach ($link in @($menu, $desk)) {
    if ($shell.CreateShortcut($link).TargetPath -ne $exe) { throw "$link does not lead to the app." }
}
foreach ($program in @($exe, (Join-Path $folder 'espflash.exe'))) {
    if (Select-String -Path $program -SimpleMatch 'VCRUNTIME140' -Encoding ascii -Quiet) {
        throw "$program needs a runtime a new Windows may not have."
    }
}

# This computer has no GPU and Windows alone offers OpenGL 1.1. Mesa's software OpenGL beside the
# program stands in for a graphics driver, here and nowhere in the package.
$archive = Join-Path $env:RUNNER_TEMP 'mesa.7z'
$unpacked = Join-Path $env:RUNNER_TEMP 'mesa'
$address = "https://github.com/pal1000/mesa-dist-win/releases/download/$mesa/mesa3d-$mesa-release-msvc.7z"
Invoke-WebRequest -Uri $address -OutFile $archive
& 7z x $archive "-o$unpacked" 'x64/*' -y | Out-Null
Copy-Item (Join-Path $unpacked 'x64\opengl32.dll'), (Join-Path $unpacked 'x64\libgallium_wgl.dll') $folder
$env:GALLIUM_DRIVER = 'llvmpipe'

function Wait-Window {
    for ($waited = 0; $waited -lt $waitSeconds; $waited++) {
        $process = Get-Process olc-core -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowTitle -eq 'OLC Core' }
        if ($process) { return $process }
        Start-Sleep -Seconds 1
    }
    $why = Join-Path $Shots 'errors.txt'
    $trial = Start-Process -FilePath $exe -PassThru -RedirectStandardError $why
    Start-Sleep -Seconds 10
    if (-not $trial.HasExited) { $trial.Kill() }
    Get-Content $why -ErrorAction SilentlyContinue
    throw "No window after $waitSeconds seconds."
}

function Save-Screen([string]$Name) {
    $area = [System.Windows.Forms.SystemInformation]::VirtualScreen
    $picture = New-Object System.Drawing.Bitmap $area.Width, $area.Height
    $drawing = [System.Drawing.Graphics]::FromImage($picture)
    $drawing.CopyFromScreen($area.Location, [System.Drawing.Point]::Empty, $area.Size)
    $picture.Save((Join-Path $Shots "$Name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    $drawing.Dispose()
    $picture.Dispose()
}

function Close-App($Process) {
    [void]$Process.CloseMainWindow()
    if (-not $Process.WaitForExit(20000)) { throw 'The app did not close.' }
}

Start-Process $menu
$app = Wait-Window
Start-Sleep -Seconds 12
Save-Screen 'first-start'
Close-App $app

$unpackedTerminal = Join-Path $env:RUNNER_TEMP 'olc'
Expand-Archive -Path $Terminal -DestinationPath $unpackedTerminal -Force
$olc = Get-ChildItem -Path $unpackedTerminal -Recurse -Filter 'olc.exe' | Select-Object -First 1
& python (Join-Path $here 'mine.py') $exe $olc.FullName $Shots
if ($LASTEXITCODE -ne 0) { throw 'Mining failed.' }

# The app's own Start mining, already pressed.
$data = Join-Path $env:APPDATA 'OLC'
New-Item -ItemType Directory -Force $data | Out-Null
[IO.File]::WriteAllText((Join-Path $data 'core.json'), '{"setup_done":true,"mine_on_start":true,"hash_cap":0}')
Start-Process $menu
$app = Wait-Window
Start-Sleep -Seconds 30
Save-Screen 'mining'
Close-App $app

$wallet = Join-Path $data 'node\wallet.json'
if (-not (Test-Path $wallet)) { throw 'The app kept no wallet to protect.' }
Remove-Item (Join-Path $folder 'opengl32.dll'), (Join-Path $folder 'libgallium_wgl.dll')
Start-Process -FilePath (Join-Path $folder 'unins000.exe') -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'
for ($waited = 0; (Test-Path $exe) -and ($waited -lt $waitSeconds); $waited++) { Start-Sleep -Seconds 1 }
foreach ($path in @($exe, $menu, $desk)) {
    if (Test-Path $path) { throw "Still there after removing the app: $path" }
}
if (-not (Test-Path $wallet)) { throw 'Removing the app took the wallet with it.' }
'Removed the app, and the wallet stayed.' | Tee-Object -FilePath (Join-Path $Shots 'removed.txt')
