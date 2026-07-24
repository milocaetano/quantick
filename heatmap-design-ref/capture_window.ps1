# capture_window.ps1 — captura a janela de um app rodando e salva PNG.
# Usa PrintWindow com PW_RENDERFULLCONTENT (0x2) para pegar conteúdo
# renderizado por GPU (egui/glow/wgpu), que um BitBlt comum não captura.
#
# Uso:
#   powershell -File capture_window.ps1 -TitleMatch quantick -OutPath shot.png
param(
    [string]$ProcessName = "quantick-app",
    [string]$OutPath = "shot.png"
)

Add-Type -AssemblyName System.Drawing

$sig = @'
using System;
using System.Runtime.InteropServices;
public class WinCap {
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr hWnd, out RECT lpRect);
    [DllImport("user32.dll")]
    public static extern bool PrintWindow(IntPtr hWnd, IntPtr hdcBlt, uint nFlags);
    [StructLayout(LayoutKind.Sequential)]
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
'@
Add-Type -TypeDefinition $sig

$proc = Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
    Where-Object { $_.MainWindowHandle -ne 0 } |
    Select-Object -First 1

if (-not $proc) {
    Write-Output "NOT_FOUND: processo '$ProcessName' com janela nao encontrado (o app esta rodando?)"
    exit 2
}

$h = $proc.MainWindowHandle
$r = New-Object WinCap+RECT
[WinCap]::GetWindowRect($h, [ref]$r) | Out-Null
$w = $r.Right - $r.Left
$ht = $r.Bottom - $r.Top
if ($w -le 0 -or $ht -le 0) { Write-Output "BAD_RECT: ${w}x${ht}"; exit 3 }

$bmp = New-Object System.Drawing.Bitmap $w, $ht
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc()
$ok = [WinCap]::PrintWindow($h, $hdc, 2)   # 2 = PW_RENDERFULLCONTENT
$g.ReleaseHdc($hdc)
$g.Dispose()

$bmp.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "SAVED: $OutPath (${w}x${ht}) printwindow_ok=$ok title='$($proc.MainWindowTitle)'"
