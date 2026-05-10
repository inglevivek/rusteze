# start-embedding-sidecar.ps1
# Starts the BioLORD embedding sidecar

$projectRoot = "C:\Home\Projects\Mainstream\rusteze\d3-graph-bench"
$sidecarDir  = Join-Path $projectRoot "embedding-sidecar"

Write-Host "[Embedding Sidecar] Navigating to $sidecarDir ..." -ForegroundColor Cyan
Set-Location $sidecarDir

# Activate virtual environment if it exists
$venvActivate = Join-Path $sidecarDir "venv\Scripts\Activate.ps1"
if (Test-Path $venvActivate) {
    Write-Host "[Embedding Sidecar] Activating venv..." -ForegroundColor Cyan
    & $venvActivate
} else {
    Write-Host "[Embedding Sidecar] No venv found, using system Python." -ForegroundColor Yellow
}

Write-Host "[Embedding Sidecar] Starting FastAPI server on port 8000..." -ForegroundColor Green
python app.py
