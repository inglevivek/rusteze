# start-rust-server.ps1
# Builds and runs the d3-graph-bench Rust backend

$projectRoot = "C:\Home\Projects\Mainstream\rusteze\d3-graph-bench"

# Navigate to project root if not already there
if ((Get-Location).Path -ne $projectRoot) {
    Write-Host "[Rust Server] Navigating to $projectRoot ..." -ForegroundColor Cyan
    Set-Location $projectRoot
} else {
    Write-Host "[Rust Server] Already in $projectRoot" -ForegroundColor Cyan
}

Write-Host "[Rust Server] Running cargo run --bin d3-graph-bench ..." -ForegroundColor Green
cargo run --bin d3-graph-bench
