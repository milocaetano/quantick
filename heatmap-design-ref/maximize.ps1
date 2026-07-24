# maximize.ps1 — maximiza a janela do app para dar mais resolucao a captura.
param([string]$ProcessName = "quantick-app")
$sig = @'
using System;
using System.Runtime.InteropServices;
public class Win2 {
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
}
'@
Add-Type -TypeDefinition $sig
$p = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1
if ($p) {
    [Win2]::ShowWindow($p.MainWindowHandle, 3) | Out-Null   # 3 = SW_MAXIMIZE
    Write-Output "maximized pid=$($p.Id)"
} else {
    Write-Output "not found"
}
