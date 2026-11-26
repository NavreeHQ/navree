# Packs the Windows client as NuGet packages: one for each library under src/ (Navree.Core, Navree.Protocol,
# Navree.Runtime, Navree.Design, Navree.Platform, Navree.Presence, Navree.Controls and Navree.Shell), all at one version.
#
#   pwsh scripts/pack.ps1 -Version 1.6.0 -Out <folder>
#
# Nothing is compiled here. The libraries are built first, in Release, with the core's DLL in native/win-x64 (the README
# says how): `dotnet build src/Navree.Shell/Navree.Shell.csproj -c Release -p:Version=<version>` builds all eight, and so
# does a build of the whole solution.
#
# Writes into <folder>:
#
#   packages/<id>.<version>.nupkg            the eight packages
#   navree-windows-<version>.zip, .sha256    the same eight in one file, for the release
#
# Every package is then read back, and the script stops when one is not what an app can use: its id and version, the
# licence, the addresses and the page nuget.org shows, its library, each of the project's references to another library
# as a dependency at this same version, the resources of a library on WinUI, and in Navree.Core the core's DLL for
# win-x64. Nothing is signed and nothing is sent anywhere: the release's `nuget` job pushes the packages.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$Out
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression.FileSystem

if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$') { throw "not a version: $Version" }

$here = Split-Path -Parent $PSScriptRoot
$projects = @(Get-ChildItem -LiteralPath (Join-Path $here 'src') -Directory | Sort-Object Name | ForEach-Object {
        $project = Join-Path $_.FullName ($_.Name + '.csproj')
        if (Test-Path -LiteralPath $project) { Get-Item -LiteralPath $project }
    })
if ($projects.Count -eq 0) { throw "no project under $(Join-Path $here 'src')" }

New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path -LiteralPath $Out).Path
$packages = Join-Path $Out 'packages'
if (Test-Path -LiteralPath $packages) { Remove-Item -LiteralPath $packages -Recurse -Force }
New-Item -ItemType Directory -Path $packages | Out-Null

Push-Location -LiteralPath $here
try {
    foreach ($project in $projects) {
        & dotnet pack $project.FullName --nologo -c Release --no-build "-p:Version=$Version" -o $packages
        if ($LASTEXITCODE -ne 0) { throw "dotnet pack did not pack $($project.BaseName)" }
    }
}
finally {
    Pop-Location
}

# The names of a package's files, with forward slashes, and its nuspec.
function Read-Package([string]$Path) {
    $zip = [System.IO.Compression.ZipFile]::OpenRead($Path)
    try {
        $names = @($zip.Entries | ForEach-Object { $_.FullName.Replace('\', '/') })
        $entry = $zip.Entries | Where-Object { $_.FullName -notmatch '[\\/]' -and $_.FullName -like '*.nuspec' } | Select-Object -First 1
        if (-not $entry) { throw "$(Split-Path -Leaf $Path) holds no nuspec" }
        $nuspec = New-Object System.Xml.XmlDocument
        $stream = $entry.Open()
        try { $nuspec.Load($stream) } finally { $stream.Dispose() }
    }
    finally { $zip.Dispose() }
    return @{ Names = $names; Nuspec = $nuspec }
}

# What an element of the nuspec's metadata says, whatever the namespace the nuspec is written in: its text, or one of its
# attributes when one is named. An element that is not there says nothing (an empty string).
function Read-Metadata([xml]$Nuspec, [string]$Name, [string]$Attribute = '') {
    $node = $Nuspec.SelectSingleNode("/*[local-name()='package']/*[local-name()='metadata']/*[local-name()='$Name']")
    if (-not $node) { return '' }
    if ($Attribute) { return [string]$node.GetAttribute($Attribute) }
    return [string]$node.InnerText
}

$wrong = @()
foreach ($project in $projects) {
    $id = $project.BaseName
    $file = Join-Path $packages "$id.$Version.nupkg"
    if (-not (Test-Path -LiteralPath $file)) { $wrong += "$id was not packed as $id.$Version.nupkg"; continue }
    $package = Read-Package $file
    $names = $package.Names
    $nuspec = $package.Nuspec

    if ((Read-Metadata $nuspec 'id') -ne $id) { $wrong += "${id}: the package's id is not $id" }
    if ((Read-Metadata $nuspec 'version') -ne $Version) { $wrong += "${id}: the package's version is not $Version" }
    if ((Read-Metadata $nuspec 'license' 'type') -ne 'expression' -or (Read-Metadata $nuspec 'license') -ne 'MIT OR Apache-2.0') {
        $wrong += "${id}: the licence is not the expression MIT OR Apache-2.0"
    }
    if ((Read-Metadata $nuspec 'projectUrl').TrimEnd('/') -ne 'https://navree.com') { $wrong += "${id}: the project's address is not https://navree.com" }
    if ((Read-Metadata $nuspec 'repository' 'url') -ne 'https://github.com/NavreeHQ/navree') {
        $wrong += "${id}: the repository's address is not https://github.com/NavreeHQ/navree"
    }
    if ((Read-Metadata $nuspec 'description') -notlike 'Navree Native for Windows*') { $wrong += "${id}: the description is not the project's own" }
    if ((Read-Metadata $nuspec 'readme') -ne 'README.md' -or $names -notcontains 'README.md') {
        $wrong += "${id}: the page nuget.org shows (README.md) is not in the package"
    }

    if (-not ($names -match "^lib/[^/]+/$([regex]::Escape($id))\.dll$")) { $wrong += "${id}: the package holds no $id.dll" }

    # Each of the project's references to another library is a dependency on that library's package, at this version.
    $source = [xml](Get-Content -LiteralPath $project.FullName -Raw)
    $dependencies = @{}
    foreach ($node in $nuspec.SelectNodes("//*[local-name()='dependency']")) { $dependencies[$node.GetAttribute('id')] = $node.GetAttribute('version') }
    foreach ($node in $source.SelectNodes('//ProjectReference')) {
        $needs = [System.IO.Path]::GetFileNameWithoutExtension($node.GetAttribute('Include').Replace('\', '/'))
        if (-not $dependencies.ContainsKey($needs)) { $wrong += "${id}: the package does not depend on $needs" }
        elseif ($dependencies[$needs] -ne $Version) { $wrong += "${id}: the package depends on $needs $($dependencies[$needs]), not $Version" }
    }

    # A library on WinUI carries its resources, which hold its XAML.
    if ($source.SelectSingleNode("//UseWinUI[text()='true']") -and -not ($names -match "^lib/[^/]+/$([regex]::Escape($id))\.pri$")) {
        $wrong += "${id}: the package holds no $id.pri"
    }
    # The core's library, where a build for win-x64 takes it from.
    if ($id -eq 'Navree.Core' -and $names -notcontains 'runtimes/win-x64/native/navree_native_core.dll') {
        $wrong += "${id}: the package does not hold runtimes/win-x64/native/navree_native_core.dll"
    }

    "$id $Version"
    $names | Where-Object { $_ -notmatch '^(_rels/|package/|\[Content_Types\]\.xml$)' } | Sort-Object | ForEach-Object { "    $_" }
}
if ($wrong.Count -gt 0) {
    $wrong | ForEach-Object { "WRONG: $_" }
    throw "the packages are not what an app can use: $($wrong.Count) thing(s) wrong"
}

$archive = Join-Path $Out "navree-windows-$Version.zip"
foreach ($old in $archive, "$archive.sha256") { if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old -Force } }
[System.IO.Compression.ZipFile]::CreateFromDirectory($packages, $archive)
$hash = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
[System.IO.File]::WriteAllText("$archive.sha256", "$hash  navree-windows-$Version.zip`n")
"made $($projects.Count) packages in $packages ($Version) and $archive"
