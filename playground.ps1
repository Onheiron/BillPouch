# ═══════════════════════════════════════════════════════════════════════════════
# BillPouch Playground — interactive P2P network simulator (Windows PowerShell)
# ═══════════════════════════════════════════════════════════════════════════════

$COMPOSE = "docker compose -f docker-compose.playground.yml"
$USERS   = @("carlo", "marco", "lucia", "elena", "paolo")

function Banner {
    Write-Host ""
    Write-Host "  ╔══════════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
    Write-Host "  ║  " -ForegroundColor Cyan -NoNewline
    Write-Host "BillPouch Playground" -ForegroundColor White -NoNewline
    Write-Host " — P2P network simulator               " -NoNewline
    Write-Host "║" -ForegroundColor Cyan
    Write-Host "  ╚══════════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
    Write-Host ""
}

function Usage {
    Banner
    Write-Host "  Usage:  .\playground.ps1 <command>" -ForegroundColor White
    Write-Host ""
    Write-Host "  up              " -ForegroundColor Green -NoNewline; Write-Host "Pull and start the network (5 users + you)"
    Write-Host "  down            " -ForegroundColor Green -NoNewline; Write-Host "Stop and remove all containers"
    Write-Host "  enter [user]    " -ForegroundColor Green -NoNewline; Write-Host "Enter a node interactively (default: you)"
    Write-Host "  status          " -ForegroundColor Green -NoNewline; Write-Host "Show what each node sees (flock view)"
    Write-Host "  logs [user]     " -ForegroundColor Green -NoNewline; Write-Host "Show logs for a node (default: all)"
    Write-Host "  network         " -ForegroundColor Green -NoNewline; Write-Host "Show network topology overview"
    Write-Host "  exec <user> ... " -ForegroundColor Green -NoNewline; Write-Host "Run a bp command on a user's node"
    Write-Host ""
    Write-Host "  Users: carlo (pouch+bill), marco (post+pouch),"
    Write-Host "         lucia (bill+post), elena (pouch×2), paolo (bill)"
    Write-Host "         me (you — interactive)"
    Write-Host ""
    Write-Host "  Examples:"
    Write-Host "    .\playground.ps1 up"
    Write-Host "    .\playground.ps1 enter"
    Write-Host "    .\playground.ps1 enter carlo"
    Write-Host "    .\playground.ps1 exec marco bp flock"
    Write-Host "    .\playground.ps1 status"
    Write-Host "    .\playground.ps1 down"
    Write-Host ""
}

function Cmd-Up {
    Banner
    Write-Host "▸ Pulling BillPouch image from Docker Hub..." -ForegroundColor Yellow
    Invoke-Expression "$COMPOSE pull"
    Write-Host ""
    Write-Host "▸ Starting 5 simulated users + your interactive node..." -ForegroundColor Yellow
    Invoke-Expression "$COMPOSE up -d"
    Write-Host ""
    Write-Host "Network is starting!" -ForegroundColor Green
    Write-Host ""
    Write-Host "  Users coming online:"
    Write-Host "    carlo  — pouch + bill  (storage + personal I/O)"
    Write-Host "    marco  — post + pouch  (relay + storage)"
    Write-Host "    lucia  — bill + post   (personal I/O + relay)"
    Write-Host "    elena  — pouch × 2    (heavy storage)"
    Write-Host "    paolo  — bill          (personal I/O)"
    Write-Host "    me     — (you)         waiting for your commands"
    Write-Host ""
    Write-Host "  Wait ~30s for peers to discover each other, then:" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "    .\playground.ps1 enter        — join the network yourself"
    Write-Host "    .\playground.ps1 status       — see what everyone sees"
    Write-Host "    .\playground.ps1 network      — topology overview"
    Write-Host ""
}

function Cmd-Down {
    Write-Host "▸ Tearing down playground..." -ForegroundColor Yellow
    Invoke-Expression "$COMPOSE down -v"
    Write-Host "Done." -ForegroundColor Green
}

function Cmd-Enter {
    param([string]$User = "me")
    $Container = "bp-$User"

    $running = docker ps --format '{{.Names}}' | Where-Object { $_ -eq $Container }
    if (-not $running) {
        Write-Host "Container '$Container' is not running." -ForegroundColor Red
        Write-Host "  Run '.\playground.ps1 up' first."
        exit 1
    }

    if ($User -eq "me") {
        Write-Host "  ╔══════════════════════════════════════════════════════════════╗" -ForegroundColor Cyan
        Write-Host "  ║  Entering playground as yourself                            ║" -ForegroundColor Cyan
        Write-Host "  ║  Network: playground                                        ║" -ForegroundColor Cyan
        Write-Host "  ╠══════════════════════════════════════════════════════════════╣" -ForegroundColor Cyan
        Write-Host "  ║  Commands:                                                  ║" -ForegroundColor Cyan
        Write-Host "  ║    bp flock                  — see the network              ║" -ForegroundColor Cyan
        Write-Host "  ║    bp hatch pouch --network playground                      ║" -ForegroundColor Cyan
        Write-Host "  ║    bp hatch bill  --network playground                      ║" -ForegroundColor Cyan
        Write-Host "  ║    bp hatch post  --network playground                      ║" -ForegroundColor Cyan
        Write-Host "  ║    bp farewell <id>          — stop a service               ║" -ForegroundColor Cyan
        Write-Host "  ║    exit                      — leave (node keeps running)   ║" -ForegroundColor Cyan
        Write-Host "  ╚══════════════════════════════════════════════════════════════╝" -ForegroundColor Cyan
        Write-Host ""
    } else {
        Write-Host "Entering $($User)'s node..." -ForegroundColor Cyan
        Write-Host "  (use bp flock, bp status, etc.)"
        Write-Host ""
    }

    docker exec -it $Container bash
}

function Cmd-Status {
    Banner
    Write-Host "Network Status — what each node sees:" -ForegroundColor White
    Write-Host ""

    $AllNodes = $USERS + @("me")
    foreach ($user in $AllNodes) {
        $Container = "bp-$user"
        $running = docker ps --format '{{.Names}}' | Where-Object { $_ -eq $Container }
        if (-not $running) {
            Write-Host "  ✗ $user — not running" -ForegroundColor Red
            continue
        }

        Write-Host "━━━ $user ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" -ForegroundColor Yellow
        docker exec $Container bp flock 2>&1
        Write-Host ""
    }
}

function Cmd-Logs {
    param([string]$User = "")
    if ($User) {
        Write-Host "Logs for $User:" -ForegroundColor Cyan
        docker logs "bp-$User" 2>&1 | Select-Object -Last 30
    } else {
        Write-Host "Logs for all nodes (last 10 lines each):" -ForegroundColor Cyan
        Write-Host ""
        $AllNodes = $USERS + @("me")
        foreach ($user in $AllNodes) {
            Write-Host "━━━ $user ━━━" -ForegroundColor Yellow
            docker logs "bp-$user" 2>&1 | Select-Object -Last 10
            Write-Host ""
        }
    }
}

function Cmd-Network {
    Banner
    Write-Host "Network Topology:" -ForegroundColor White
    Write-Host ""
    Write-Host "  ┌─────────────────────────────────────────────────────┐"
    Write-Host "  │              BillPouch Playground Network            │"
    Write-Host "  │                  network: playground                 │"
    Write-Host "  │                                                      │"
    Write-Host "  │   carlo ──── [pouch] [bill]                          │"
    Write-Host "  │   marco ──── [post]  [pouch]    ← relay + storage    │"
    Write-Host "  │   lucia ──── [bill]  [post]     ← I/O + relay        │"
    Write-Host "  │   elena ──── [pouch] [pouch]    ← heavy storage      │"
    Write-Host "  │   paolo ──── [bill]             ← file I/O only      │"
    Write-Host "  │   ★ me ──── (you)               ← interactive        │"
    Write-Host "  │                                                      │"
    Write-Host "  │   All connected via mDNS + gossipsub on bridge net   │"
    Write-Host "  └─────────────────────────────────────────────────────┘"
    Write-Host ""

    Write-Host "Live peer counts:" -ForegroundColor White
    $AllNodes = $USERS + @("me")
    foreach ($user in $AllNodes) {
        $Container = "bp-$user"
        $running = docker ps --format '{{.Names}}' | Where-Object { $_ -eq $Container }
        if (-not $running) {
            Write-Host "  ✗ $user`: not running" -ForegroundColor Red
            continue
        }
        $flock = docker exec $Container bp flock 2>&1
        $peers = ($flock | Select-String 'Known Peers\s+\((\d+)' | ForEach-Object { $_.Matches[0].Groups[1].Value }) ?? "?"
        $svcs  = ($flock | Select-String 'Local Services\s+\((\d+)' | ForEach-Object { $_.Matches[0].Groups[1].Value }) ?? "?"
        Write-Host "  ● $user`: $svcs service(s), $peers known peer(s)" -ForegroundColor Green
    }
    Write-Host ""
}

function Cmd-Exec {
    param([string]$User, [string[]]$Cmd)
    docker exec "bp-$User" @Cmd
}

# ── Main ─────────────────────────────────────────────────────────────────────
$Command = if ($args.Count -gt 0) { $args[0] } else { "help" }

switch ($Command) {
    "up"      { Cmd-Up }
    "down"    { Cmd-Down }
    "enter"   { Cmd-Enter ($args.Count -gt 1 ? $args[1] : "me") }
    "status"  { Cmd-Status }
    "logs"    { Cmd-Logs ($args.Count -gt 1 ? $args[1] : "") }
    "network" { Cmd-Network }
    "exec"    {
        if ($args.Count -lt 3) { Write-Host "Usage: .\playground.ps1 exec <user> <command...>" -ForegroundColor Red; exit 1 }
        Cmd-Exec $args[1] $args[2..($args.Count-1)]
    }
    default   { Usage }
}
