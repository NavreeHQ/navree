#Requires -Version 5.1
<#
.SYNOPSIS
    Real database servers for Navree's integration tests, on 127.0.0.1 only.

.DESCRIPTION
    Three user-level, portable servers (no Windows services, no admin rights),
    each on a non-default port so nothing else on the machine is affected:

      postgres  PostgreSQL 17 + TimescaleDB   127.0.0.1:55432  database navree_test
      mysql     MySQL 8.4 LTS                 127.0.0.1:53306  database navree_test
      mongodb   MongoDB 8.0, replica set rs0  127.0.0.1:57017  (transactions work)

    Binaries live in target/databases/<server>, data in target/databases/data,
    logs in target/databases/logs and generated configuration in
    target/databases/config. `cargo clean` deletes all of it: re-extract the
    archives kept in target/databases/downloads (see target/databases/README.txt)
    and run `start` again.

    Commands:
      start    Initializes any missing data directory, starts each server
               detached (closing the terminal leaves it running), waits until it
               accepts connections, makes sure navree_test exists, and writes
               target/test-databases.env.
      stop     Shuts each server down cleanly (forcefully after a timeout).
      status   Shows each server's state; exits 1 unless all are ready.
      restart  stop, then start.

    Name servers after the command to act on a subset.

.EXAMPLE
    pwsh -File scripts/dev-databases.ps1 start
    pwsh -File scripts/dev-databases.ps1 status
    pwsh -File scripts/dev-databases.ps1 stop mongodb

    # then, from Git Bash:
    source target/test-databases.env
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('start', 'stop', 'status', 'restart')]
    [string] $Command = 'status',

    [Parameter(Position = 1, ValueFromRemainingArguments = $true)]
    [ValidateSet('postgres', 'mysql', 'mongodb')]
    [string[]] $Server = @('postgres', 'mysql', 'mongodb')
)

Set-StrictMode -Version 3.0
$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Layout and credentials (throwaway test servers; localhost only)
# ---------------------------------------------------------------------------

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Base = Join-Path $RepoRoot 'target\databases'
$DataRoot = Join-Path $Base 'data'
$LogDir = Join-Path $Base 'logs'
$ConfDir = Join-Path $Base 'config'
$EnvFile = Join-Path $RepoRoot 'target\test-databases.env'
$TestDb = 'navree_test'
$Password = 'navree'

$Pg = @{
    Home = Join-Path $Base 'postgresql-17'
    Data = Join-Path $DataRoot 'postgres'
    Log  = Join-Path $LogDir 'postgres.log'
    Port = 55432
    User = 'postgres'
}
$Pg.Bin = Join-Path $Pg.Home 'bin'
$Pg.Url = "postgres://$($Pg.User):$Password@127.0.0.1:$($Pg.Port)/$TestDb"

$My = @{
    Home       = Join-Path $Base 'mysql-8.4'
    Data       = Join-Path $DataRoot 'mysql'
    Log        = Join-Path $LogDir 'mysql.err'
    Ini        = Join-Path $ConfDir 'my.ini'
    ClientConf = Join-Path $ConfDir 'mysql-client.cnf'
    Port       = 53306
    User       = 'root'
}
$My.Bin = Join-Path $My.Home 'bin'
$My.Pid = Join-Path $My.Data 'mysqld.pid'
$My.Marker = Join-Path $My.Data 'navree-bootstrapped'
$My.Url = "mysql://$($My.User):$Password@127.0.0.1:$($My.Port)/$TestDb"

$Mongo = @{
    Home  = Join-Path $Base 'mongodb-8.0'
    Shell = Join-Path $Base 'mongosh\bin\mongosh.exe'
    Data  = Join-Path $DataRoot 'mongodb'
    Log   = Join-Path $LogDir 'mongod.log'
    Conf  = Join-Path $ConfDir 'mongod.cfg'
    Port  = 57017
    ReplicaSet = 'rs0'
}
$Mongo.Mongod = Join-Path $Mongo.Home 'bin\mongod.exe'
$Mongo.Url = "mongodb://127.0.0.1:$($Mongo.Port)/$($TestDb)?replicaSet=$($Mongo.ReplicaSet)"
$Mongo.Direct = "mongodb://127.0.0.1:$($Mongo.Port)/admin?directConnection=true"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

function ConvertTo-ForwardSlash([string] $Path) { $Path -replace '\\', '/' }

function Write-TextFile([string] $Path, [string] $Text) {
    # UTF-8 without BOM, LF line endings (the env file is sourced by Git Bash).
    $encoding = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($Path, ($Text -replace "`r`n", "`n"), $encoding)
}

function Invoke-Native {
    # Runs a native program and captures exit code and combined output without
    # letting stderr turn into a terminating error (Windows PowerShell 5.1).
    param([Parameter(Mandatory)] [string] $Exe, [string[]] $Arguments = @())
    $previous = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & $Exe @Arguments 2>&1 | ForEach-Object { "$_" }
        [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = (@($output) -join "`n").Trim() }
    } finally {
        $ErrorActionPreference = $previous
    }
}

function Start-Detached {
    # Launches a process outside this shell's process tree and job object (via
    # WMI), so it keeps running after the terminal or the calling tool exits.
    # Falls back to Start-Process when WMI is unavailable.
    param([Parameter(Mandatory)] [string] $Exe, [string] $Arguments = '')
    $commandLine = "`"$Exe`" $Arguments"
    try {
        $result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
            CommandLine      = $commandLine
            CurrentDirectory = $Base
        } -ErrorAction Stop
        if ($result.ReturnValue -ne 0) { throw "Win32_Process.Create returned $($result.ReturnValue)" }
        return [int] $result.ProcessId
    } catch {
        Write-Verbose "WMI launch failed ($_); using Start-Process"
        $process = Start-Process -FilePath $Exe -ArgumentList $Arguments -WorkingDirectory $Base `
            -WindowStyle Hidden -PassThru
        return $process.Id
    }
}

function Get-Listener([int] $Port) {
    @(Get-NetTCPConnection -State Listen -LocalPort $Port -ErrorAction SilentlyContinue)
}

function Wait-Until {
    param([Parameter(Mandatory)] [scriptblock] $Condition, [int] $TimeoutSeconds = 60)
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ((Get-Date) -lt $deadline) {
        if (& $Condition) { return $true }
        Start-Sleep -Milliseconds 500
    }
    return [bool] (& $Condition)
}

function Wait-ProcessExit([int[]] $ProcessIds, [int] $TimeoutSeconds = 60) {
    Wait-Until -TimeoutSeconds $TimeoutSeconds -Condition {
        -not (Get-Process -Id $ProcessIds -ErrorAction SilentlyContinue)
    }
}

function Get-LogTail([string] $Path, [int] $Lines = 25) {
    if (Test-Path $Path) { (Get-Content -Path $Path -Tail $Lines) -join "`n" } else { "(no log at $Path)" }
}

function Assert-PortFree([int] $Port, [string] $Name) {
    $listener = Get-Listener $Port | Select-Object -First 1
    if ($listener) {
        $owner = Get-Process -Id $listener.OwningProcess -ErrorAction SilentlyContinue
        $ownerName = if ($owner) { $owner.ProcessName } else { 'another process' }
        throw "$Name cannot start: port $Port is already in use by $ownerName (pid $($listener.OwningProcess))."
    }
}

function Assert-Installed([string] $Path, [string] $Name) {
    if (-not (Test-Path $Path)) {
        throw "$Name is not installed: $Path is missing. See target/databases/README.txt."
    }
}

function Write-EnvFile {
    $text = @"
# Navree integration-test databases, written by scripts/dev-databases.ps1.
# Start them with: pwsh -File scripts/dev-databases.ps1 start
# Load into Git Bash with: source target/test-databases.env
export NAVREE_TEST_POSTGRES_URL='$($Pg.Url)'
export NAVREE_TEST_MYSQL_URL='$($My.Url)'
export NAVREE_TEST_MONGODB_URL='$($Mongo.Url)'

"@
    Write-TextFile $EnvFile $text
}

# ---------------------------------------------------------------------------
# PostgreSQL 17 + TimescaleDB
# ---------------------------------------------------------------------------

function Invoke-Psql([string] $Database, [string[]] $Sql) {
    $arguments = @('-X', '-q', '-v', 'ON_ERROR_STOP=1', '-t', '-A',
        '-d', "postgresql://$($Pg.User):$Password@127.0.0.1:$($Pg.Port)/$Database")
    foreach ($statement in $Sql) { $arguments += @('-c', $statement) }
    Invoke-Native (Join-Path $Pg.Bin 'psql.exe') $arguments
}

function Test-PostgresReady {
    (Invoke-Native (Join-Path $Pg.Bin 'pg_isready.exe') @('-q', '-h', '127.0.0.1', '-p', "$($Pg.Port)")).ExitCode -eq 0
}

function Test-PostgresRunning {
    (Test-Path (Join-Path $Pg.Data 'PG_VERSION')) -and
    (Invoke-Native (Join-Path $Pg.Bin 'pg_ctl.exe') @('status', '-D', $Pg.Data)).ExitCode -eq 0
}

function Initialize-Postgres {
    if (Test-Path (Join-Path $Pg.Data 'PG_VERSION')) { return }
    Write-Host 'postgres: initializing a new cluster'
    $passwordFile = Join-Path $ConfDir 'postgres-superuser.tmp'
    Write-TextFile $passwordFile $Password
    try {
        $init = Invoke-Native (Join-Path $Pg.Bin 'initdb.exe') @('-D', $Pg.Data, '-U', $Pg.User,
            "--pwfile=$passwordFile", '--auth=scram-sha-256', '--encoding=UTF8', '--locale=C')
        if ($init.ExitCode -ne 0) { throw "initdb failed:`n$($init.Output)" }
    } finally {
        Remove-Item -Force $passwordFile -ErrorAction SilentlyContinue
    }
    # Later settings in postgresql.conf override the defaults above them.
    Add-Content -Path (Join-Path $Pg.Data 'postgresql.conf') -Value @"

# ---- Navree integration-test server (scripts/dev-databases.ps1) ----
listen_addresses = '127.0.0.1'
port = $($Pg.Port)
max_connections = 200
shared_buffers = 256MB
shared_preload_libraries = 'timescaledb'
timescaledb.telemetry_level = off
timescaledb.max_background_workers = 8
max_worker_processes = 16
# Throwaway test data: trade crash safety for speed.
fsync = off
synchronous_commit = off
full_page_writes = off
"@
}

function Start-Postgres {
    Assert-Installed (Join-Path $Pg.Bin 'postgres.exe') 'PostgreSQL'
    Assert-Installed (Join-Path $Pg.Home 'lib\timescaledb.dll') 'TimescaleDB'
    Initialize-Postgres
    if (Test-PostgresRunning) {
        Write-Host 'postgres: already running'
    } else {
        Assert-PortFree $Pg.Port 'postgres'
        Start-Detached (Join-Path $Pg.Bin 'pg_ctl.exe') "start -D `"$($Pg.Data)`" -l `"$($Pg.Log)`" -s" | Out-Null
    }
    if (-not (Wait-Until -TimeoutSeconds 60 -Condition { Test-PostgresReady })) {
        throw "postgres did not accept connections within 60s. Log tail:`n$(Get-LogTail $Pg.Log)"
    }
    $exists = Invoke-Psql 'postgres' @("SELECT 1 FROM pg_database WHERE datname = '$TestDb'")
    if ($exists.ExitCode -ne 0) { throw "postgres: query failed:`n$($exists.Output)" }
    if ($exists.Output -ne '1') {
        $create = Invoke-Psql 'postgres' @("CREATE DATABASE $TestDb")
        if ($create.ExitCode -ne 0) { throw "postgres: CREATE DATABASE failed:`n$($create.Output)" }
    }
    $extension = Invoke-Psql $TestDb @('SET client_min_messages = error', 'CREATE EXTENSION IF NOT EXISTS timescaledb')
    if ($extension.ExitCode -ne 0) { throw "postgres: CREATE EXTENSION timescaledb failed:`n$($extension.Output)" }
    Write-Host "postgres: ready on 127.0.0.1:$($Pg.Port)"
}

function Stop-Postgres {
    if (-not (Test-PostgresRunning)) { Write-Host 'postgres: not running'; return }
    $stop = Invoke-Native (Join-Path $Pg.Bin 'pg_ctl.exe') @('stop', '-D', $Pg.Data, '-m', 'fast', '-w', '-t', '60', '-s')
    if ($stop.ExitCode -ne 0) { throw "postgres: pg_ctl stop failed:`n$($stop.Output)" }
    Write-Host 'postgres: stopped'
}

# ---------------------------------------------------------------------------
# MySQL 8.4
# ---------------------------------------------------------------------------

function Write-MySqlConfig {
    $basedir = ConvertTo-ForwardSlash $My.Home
    $datadir = ConvertTo-ForwardSlash $My.Data
    Write-TextFile $My.Ini @"
# Navree integration-test server (generated by scripts/dev-databases.ps1).
[mysqld]
basedir="$basedir"
datadir="$datadir"
port=$($My.Port)
bind-address=127.0.0.1
# The X Protocol plugin would listen on 33060 on every interface.
mysqlx=OFF
pid-file="$(ConvertTo-ForwardSlash $My.Pid)"
log-error="$(ConvertTo-ForwardSlash $My.Log)"
character-set-server=utf8mb4
collation-server=utf8mb4_0900_ai_ci
max_connections=300
local_infile=OFF
# Throwaway test data: trade crash safety for speed.
skip-log-bin
innodb_buffer_pool_size=256M
innodb_flush_log_at_trx_commit=2

[client]
port=$($My.Port)
"@
    Write-TextFile $My.ClientConf @"
[client]
user=$($My.User)
password=$Password
host=127.0.0.1
port=$($My.Port)
"@
}

function Invoke-MySql([string[]] $Arguments) {
    Invoke-Native (Join-Path $My.Bin 'mysql.exe') (@("--defaults-extra-file=$($My.ClientConf)") + $Arguments)
}

function Get-MySqlPid {
    if (-not (Test-Path $My.Pid)) { return $null }
    $id = (Get-Content $My.Pid -ErrorAction SilentlyContinue | Select-Object -First 1)
    if ($id -and (Get-Process -Id ([int] $id) -ErrorAction SilentlyContinue | Where-Object ProcessName -eq 'mysqld')) {
        return [int] $id
    }
    return $null
}

function Test-MySqlReady {
    $ping = Invoke-Native (Join-Path $My.Bin 'mysqladmin.exe') @("--defaults-extra-file=$($My.ClientConf)",
        '--connect-timeout=2', 'ping')
    # mysqladmin ping exits 0 whenever the server answers, even "Access denied".
    $ping.ExitCode -eq 0
}

function Initialize-MySql {
    if (Test-Path (Join-Path $My.Data 'mysql')) { return }
    Write-Host 'mysql: initializing a new data directory'
    $init = Invoke-Native (Join-Path $My.Bin 'mysqld.exe') @("--defaults-file=$($My.Ini)", '--initialize-insecure')
    if ($init.ExitCode -ne 0) { throw "mysqld --initialize-insecure failed:`n$($init.Output)`n$(Get-LogTail $My.Log)" }
}

function Start-MySql {
    Assert-Installed (Join-Path $My.Bin 'mysqld.exe') 'MySQL'
    Write-MySqlConfig
    Initialize-MySql
    if (Get-MySqlPid) {
        Write-Host 'mysql: already running'
    } else {
        Assert-PortFree $My.Port 'mysql'
        Start-Detached (Join-Path $My.Bin 'mysqld.exe') "--defaults-file=`"$($My.Ini)`"" | Out-Null
    }
    if (-not (Wait-Until -TimeoutSeconds 90 -Condition { Test-MySqlReady })) {
        throw "mysql did not accept connections within 90s. Log tail:`n$(Get-LogTail $My.Log)"
    }
    if (-not (Test-Path $My.Marker)) {
        # --initialize-insecure leaves root@localhost without a password.
        $bootstrap = Invoke-Native (Join-Path $My.Bin 'mysql.exe') @('--no-defaults', '-h', '127.0.0.1',
            '-P', "$($My.Port)", '-u', $My.User, '--skip-password',
            '-e', "ALTER USER 'root'@'localhost' IDENTIFIED BY '$Password'")
        if ($bootstrap.ExitCode -ne 0) { throw "mysql: setting the root password failed:`n$($bootstrap.Output)" }
        Write-TextFile $My.Marker "root password set by scripts/dev-databases.ps1`n"
    }
    $create = Invoke-MySql @('-e', "CREATE DATABASE IF NOT EXISTS $TestDb CHARACTER SET utf8mb4 COLLATE utf8mb4_0900_ai_ci")
    if ($create.ExitCode -ne 0) { throw "mysql: CREATE DATABASE failed:`n$($create.Output)" }
    Write-Host "mysql: ready on 127.0.0.1:$($My.Port)"
}

function Stop-MySql {
    $id = Get-MySqlPid
    if (-not $id) { Write-Host 'mysql: not running'; return }
    Invoke-Native (Join-Path $My.Bin 'mysqladmin.exe') @("--defaults-extra-file=$($My.ClientConf)", 'shutdown') | Out-Null
    if (-not (Wait-ProcessExit @($id) 60)) {
        Write-Warning 'mysql: clean shutdown timed out; stopping the process'
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    }
    Write-Host 'mysql: stopped'
}

# ---------------------------------------------------------------------------
# MongoDB 8.0 (single-node replica set, so transactions work)
# ---------------------------------------------------------------------------

function Write-MongoConfig {
    Write-TextFile $Mongo.Conf @"
# Navree integration-test server (generated by scripts/dev-databases.ps1).
systemLog:
  destination: file
  path: "$(ConvertTo-ForwardSlash $Mongo.Log)"
  logAppend: true
storage:
  dbPath: "$(ConvertTo-ForwardSlash $Mongo.Data)"
  wiredTiger:
    engineConfig:
      cacheSizeGB: 0.5
net:
  bindIp: 127.0.0.1
  port: $($Mongo.Port)
replication:
  replSetName: $($Mongo.ReplicaSet)
  oplogSizeMB: 1024
setParameter:
  diagnosticDataCollectionEnabled: false
"@
}

function Invoke-Mongosh([string] $Script) {
    Invoke-Native $Mongo.Shell @('--quiet', '--norc', $Mongo.Direct, '--eval', $Script)
}

function Get-MongoPid {
    $listener = Get-Listener $Mongo.Port | Select-Object -First 1
    if ($listener -and (Get-Process -Id $listener.OwningProcess -ErrorAction SilentlyContinue |
            Where-Object ProcessName -eq 'mongod')) {
        return [int] $listener.OwningProcess
    }
    return $null
}

function Test-MongoPrimary {
    $hello = Invoke-Mongosh 'print(db.hello().isWritablePrimary === true)'
    $hello.ExitCode -eq 0 -and $hello.Output -match 'true$'
}

function Start-MongoDb {
    Assert-Installed $Mongo.Mongod 'MongoDB'
    Assert-Installed $Mongo.Shell 'mongosh'
    New-Item -ItemType Directory -Force -Path $Mongo.Data | Out-Null
    Write-MongoConfig
    if (Get-MongoPid) {
        Write-Host 'mongodb: already running'
    } else {
        Assert-PortFree $Mongo.Port 'mongodb'
        Start-Detached $Mongo.Mongod "--config `"$($Mongo.Conf)`"" | Out-Null
    }
    $up = Wait-Until -TimeoutSeconds 60 -Condition {
        (Get-MongoPid) -and (Invoke-Mongosh 'db.adminCommand({ ping: 1 }).ok').ExitCode -eq 0
    }
    if (-not $up) { throw "mongodb did not accept connections within 60s. Log tail:`n$(Get-LogTail $Mongo.Log)" }
    # Initiate the replica set once (code 94: NotYetInitialized).
    $initiate = Invoke-Mongosh (@(
            'try { rs.status(); print(''initiated''); }'
            'catch (e) { if (e.code !== 94) throw e;'
            "  rs.initiate({ _id: '$($Mongo.ReplicaSet)', members: [{ _id: 0, host: '127.0.0.1:$($Mongo.Port)' }] });"
            '  print(''initiating''); }'
        ) -join ' ')
    if ($initiate.ExitCode -ne 0) { throw "mongodb: replica set initiation failed:`n$($initiate.Output)" }
    if (-not (Wait-Until -TimeoutSeconds 60 -Condition { Test-MongoPrimary })) {
        throw "mongodb did not become primary of $($Mongo.ReplicaSet) within 60s. Log tail:`n$(Get-LogTail $Mongo.Log)"
    }
    Write-Host "mongodb: ready on 127.0.0.1:$($Mongo.Port) (replica set $($Mongo.ReplicaSet), primary)"
}

function Stop-MongoDb {
    $id = Get-MongoPid
    if (-not $id) { Write-Host 'mongodb: not running'; return }
    # The server drops the connection as it exits, so the shell reports an error.
    Invoke-Mongosh 'db.adminCommand({ shutdown: 1, force: true })' | Out-Null
    if (-not (Wait-ProcessExit @($id) 60)) {
        Write-Warning 'mongodb: clean shutdown timed out; stopping the process'
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    }
    Write-Host 'mongodb: stopped'
}

# ---------------------------------------------------------------------------
# Status
# ---------------------------------------------------------------------------

function Get-ServerStatus([string] $Name) {
    switch ($Name) {
        'postgres' { $port = $Pg.Port; $url = $Pg.Url }
        'mysql' { $port = $My.Port; $url = $My.Url }
        'mongodb' { $port = $Mongo.Port; $url = $Mongo.Url }
    }
    $listeners = @(Get-Listener $port)
    $processId = $null; $listen = ''
    if ($listeners.Count -gt 0) {
        $processId = $listeners[0].OwningProcess
        $listen = (($listeners | ForEach-Object { "$($_.LocalAddress):$($_.LocalPort)" }) | Sort-Object -Unique) -join ', '
    }
    $ready = $false
    if ($processId) {
        switch ($Name) {
            'postgres' { $ready = Test-PostgresReady }
            'mysql' { $ready = Test-MySqlReady }
            'mongodb' { $ready = Test-MongoPrimary }
        }
    }
    [pscustomobject]@{
        Server = $Name
        State  = if ($ready) { 'ready' } elseif ($processId) { 'starting/unhealthy' } else { 'stopped' }
        Pid    = $processId
        Listen = $listen
        Url    = $url
    }
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

foreach ($dir in @($DataRoot, $LogDir, $ConfDir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }
$order = @('postgres', 'mysql', 'mongodb') | Where-Object { $Server -contains $_ }
$failed = $false

function Invoke-ForEachServer([string] $Verb) {
    foreach ($name in $order) {
        try {
            switch ("$Verb-$name") {
                'start-postgres' { Start-Postgres }
                'start-mysql' { Start-MySql }
                'start-mongodb' { Start-MongoDb }
                'stop-postgres' { Stop-Postgres }
                'stop-mysql' { Stop-MySql }
                'stop-mongodb' { Stop-MongoDb }
            }
        } catch {
            Write-Host "${name}: $($_.Exception.Message)" -ForegroundColor Red
            $script:failed = $true
        }
    }
}

switch ($Command) {
    'start' {
        Invoke-ForEachServer 'start'
        Write-EnvFile
        Write-Host "env: $EnvFile"
    }
    'stop' { Invoke-ForEachServer 'stop' }
    'restart' {
        Invoke-ForEachServer 'stop'
        Invoke-ForEachServer 'start'
        Write-EnvFile
    }
    'status' {
        $rows = foreach ($name in $order) { Get-ServerStatus $name }
        $rows | Format-Table -AutoSize | Out-String -Width 200 | Write-Host
        if (@($rows | Where-Object State -ne 'ready').Count -gt 0) { $failed = $true }
    }
}

if ($failed) { exit 1 }
exit 0
