# Gesso Windows 宠物窗口自动化回归套件（开发机用，需真实桌面会话）
# 用法：powershell -File regress.ps1 -Phase <render|poke|touch|drag|gaze|flicker|walksleep|wake|bubble|menu>
# 坐标约定：全部按窗口 116x164pt 比例换算（k = 窗宽px/116），不硬编码 DPI。
param(
    [Parameter(Mandatory = $true)][string]$Phase,
    [string]$SavePath = "$env:USERPROFILE\.gesso\pet-save-v1.json",
    [string]$PrintDir = "C:/Users/zhang/AppData/Local/Temp/pet-verify"
)

$ErrorActionPreference = "Stop"
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class PV {
  [DllImport("user32.dll")] public static extern IntPtr FindWindowW([MarshalAs(UnmanagedType.LPWStr)] string cls, string title);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
  [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr v);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr wp, IntPtr lp);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  public struct R { public int L, T, Rt, B; }
}
"@
Add-Type -AssemblyName System.Drawing
[void][PV]::SetProcessDpiAwarenessContext([IntPtr]-4)

function Get-Pet {
    $h = [PV]::FindWindowW("GessoPetWindow", $null)
    if ($h -eq [IntPtr]::Zero) { throw "GessoPetWindow 不存在" }
    $r = New-Object PV+R
    [void][PV]::GetWindowRect($h, [ref]$r)
    return @{ hwnd = $h; L = $r.L; T = $r.T; W = $r.Rt - $r.L; Ht = $r.B - $r.T }
}

function Print-Pet([IntPtr]$h, [int]$w, [int]$ht) {
    $bmp = New-Object System.Drawing.Bitmap($w, $ht)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    [void][PV]::PrintWindow($h, $hdc, 2)
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    return $bmp
}

# pt 坐标 → 物理像素（窗口实际宽度即 k 的度量）
function PX([double]$ptX, [double]$ptY, $pet) {
    $k = $pet.W / 116.0
    return @([int]($pet.L + $ptX * $k), [int]($pet.T + $ptY * $k))
}

function Count-Band($bmp, [double]$x0, [double]$y0, [double]$x1, [double]$y1, [scriptblock]$pred) {
    $k = $bmp.Width / 116.0
    $n = 0
    $ax0 = [int]($x0 * $k); $ax1 = [int]($x1 * $k)
    $ay0 = [int]($y0 * $k); $ay1 = [int]($y1 * $k)
    for ($y = $ay0; $y -lt $ay1; $y++) {
        for ($x = $ax0; $x -lt $ax1; $x++) {
            if (& $pred $bmp.GetPixel($x, $y)) { $n++ }
        }
    }
    return $n
}

function Band-AvgR($bmp, [double]$x0, [double]$y0, [double]$x1, [double]$y1) {
    $k = $bmp.Width / 116.0
    $sum = 0.0; $n = 0
    for ($y = [int]($y0 * $k); $y -lt [int]($y1 * $k); $y++) {
        for ($x = [int]($x0 * $k); $x -lt [int]($x1 * $k); $x++) {
            $sum += $bmp.GetPixel($x, $y).R; $n++
        }
    }
    return $sum / [Math]::Max(1, $n)
}

function Band-Diff($a, $b, [double]$x0, [double]$y0, [double]$x1, [double]$y1) {
    $k = $a.Width / 116.0
    $n = 0
    for ($y = [int]($y0 * $k); $y -lt [int]($y1 * $k); $y++) {
        for ($x = [int]($x0 * $k); $x -lt [int]($x1 * $k); $x++) {
            $ca = $a.GetPixel($x, $y); $cb = $b.GetPixel($x, $y)
            if ([Math]::Abs($ca.R - $cb.R) -gt 30 -or [Math]::Abs($ca.G - $cb.G) -gt 30) { $n++ }
        }
    }
    return $n
}

function Read-Save {
    return (Get-Content $SavePath -Raw | ConvertFrom-Json)
}

function Click-At([int]$x, [int]$y) {
    [void][PV]::SetCursorPos($x, $y)
    Start-Sleep -Milliseconds 450  # 等一拍 sync 清除 WS_EX_TRANSPARENT
    [PV]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 50
    [PV]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 2600  # 延迟确认(300ms)+引擎节拍+写盘节流
}

$waxPred = { param($c) $c.R -ge 230 -and $c.G -ge 200 -and $c.B -ge 160 -and $c.R -gt $c.B -and $c.R -gt 100 }
$flamePred = { param($c) $c.R -ge 245 -and $c.G -ge 100 -and $c.B -le 90 }

switch ($Phase) {
    "render" {
        $pet = Get-Pet
        $bmp = Print-Pet $pet.hwnd $pet.W $pet.Ht
        $bmp.Save("$PrintDir/verify-render.png")
        $wax = Count-Band $bmp 40 110 90 150 $waxPred
        $flame = Count-Band $bmp 40 66 90 92 $flamePred
        Write-Host "RESULT wax=$wax flame=$flame"
        if ($wax -lt 500 -or $flame -lt 10) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "poke" {
        $before = (Read-Save).stats.poke_count
        $pet = Get-Pet
        $c = PX 58 127 $pet
        Click-At $c[0] $c[1]
        $after = (Read-Save).stats.poke_count
        Write-Host "RESULT poke $before -> $after"
        if ($after -ne ($before + 1)) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "touch" {
        $s = Read-Save
        $bp = $s.stats.poke_count; $bt = $s.stats.touch_count
        $pet = Get-Pet
        $c = PX 58 127 $pet
        [void][PV]::SetCursorPos($c[0], $c[1])
        Start-Sleep -Milliseconds 450
        foreach ($i in 1..2) {
            [PV]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 50
            [PV]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 110
        }
        Start-Sleep -Milliseconds 2800
        $s2 = Read-Save
        Write-Host ("RESULT poke {0}->{1} touch {2}->{3}" -f $bp, $s2.stats.poke_count, $bt, $s2.stats.touch_count)
        if ($s2.stats.touch_count -ne ($bt + 1)) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "drag" {
        $pet = Get-Pet
        $c = PX 58 127 $pet
        # 拖到右上方（像素位移）
        [void][PV]::SetCursorPos($c[0], $c[1])
        Start-Sleep -Milliseconds 450
        [PV]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 80
        foreach ($step in 1..12) {
            [void][PV]::SetCursorPos(($c[0] + $step * 18), ($c[1] - $step * 10))
            Start-Sleep -Milliseconds 35
        }
        [PV]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 2600  # 回报 + 写回 + 收敛
        $pet2 = Get-Pet
        $moved = [Math]::Abs($pet2.L - $pet.L) + [Math]::Abs($pet2.T - $pet.T)
        $pos = (Read-Save).position
        $hasPos = $null -ne $pos
        Write-Host "RESULT moved=${moved}px positionSaved=$hasPos"
        if ($moved -lt 150 -or -not $hasPos) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "gaze" {
        $pet = Get-Pet
        $shots = @{}
        foreach ($side in @("L", "R")) {
            $off = if ($side -eq "L") { 42 } else { 84 }
            $arr = @()
            foreach ($i in 1..3) {
                $p = PX $off 120 $pet
                [void][PV]::SetCursorPos($p[0], $p[1])
                [void][PV]::SetCursorPos(($p[0] + 4), $p[1])  # 微动触发 MouseMoved
                Start-Sleep -Milliseconds 220
                $arr += (Print-Pet $pet.hwnd $pet.W $pet.Ht)
                Start-Sleep -Milliseconds 60
            }
            $shots[$side] = $arr
        }
        # 同侧取最小差异对（滤掉偶发眨眼帧），跨侧取最大差异对
        $same = 999999
        for ($i = 0; $i -lt 3; $i++) {
            for ($j = $i + 1; $j -lt 3; $j++) {
                $same = [Math]::Min($same, (Band-Diff $shots.L[$i] $shots.L[$j] 40 95 80 112))
                $same = [Math]::Min($same, (Band-Diff $shots.R[$i] $shots.R[$j] 40 95 80 112))
            }
        }
        $cross = 0
        for ($i = 0; $i -lt 3; $i++) {
            for ($j = 0; $j -lt 3; $j++) {
                $cross = [Math]::Max($cross, (Band-Diff $shots.L[$i] $shots.R[$j] 40 95 80 112))
            }
        }
        Write-Host "RESULT same-side-min=$same cross-max=$cross"
        if ($same -gt 20 -or $cross -lt 15) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "flicker" {
        $pet = Get-Pet
        [void][PV]::SetCursorPos(8, 8)  # 光标离场（非 Watching/交互态）
        $frames = @()
        foreach ($i in 1..14) {
            $frames += (Print-Pet $pet.hwnd $pet.W $pet.Ht)
            Start-Sleep -Milliseconds 120
        }
        $distinct = 1
        for ($i = 1; $i -lt $frames.Count; $i++) {
            if ((Band-Diff $frames[0] $frames[$i] 40 66 90 92) -gt 8) { $distinct++ }
        }
        $frames[0].Save("$PrintDir/verify-flicker.png")
        Write-Host "RESULT distinct-flame-frames=$distinct"
        if ($distinct -lt 2) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "walksleep" {
        # 自锚定：先在身上抖动光标重置 idle（等穿透清除后），此后时间轴可控。
        # 安静档：30s 走动（3s）/ 45s 入睡。阈值按几何稀释标定（band 含透明区）：
        # 清醒蜡身 avgR≈150+，入睡 ×0.72 后 ≈85。
        $pet = Get-Pet
        $p = PX 58 127 $pet
        [void][PV]::SetCursorPos($p[0], $p[1])
        Start-Sleep -Milliseconds 450
        [void][PV]::SetCursorPos(($p[0] + 5), $p[1])
        Start-Sleep -Milliseconds 300
        [void][PV]::SetCursorPos(8, 8)  # 光标离场，idle 开始累计
        Start-Sleep -Milliseconds 28500
        $xs = @()
        foreach ($i in 1..32) {  # 8s 采样窗覆盖 30~33s 的走动段
            $q = Get-Pet
            $xs += $q.L
            Start-Sleep -Milliseconds 250
        }
        $m = ($xs | Measure-Object -Maximum -Minimum)
        $walkRange = $m.Maximum - $m.Minimum
        Write-Host "RESULT walk-x-range=${walkRange}px"
        Start-Sleep -Milliseconds 12000  # idle 45s+ 后必入睡
        $pet2 = Get-Pet
        $bmp = Print-Pet $pet2.hwnd $pet2.W $pet2.Ht
        $bmp.Save("$PrintDir/verify-sleep.png")
        $avg = Band-AvgR $bmp 45 115 90 145
        Write-Host "RESULT sleeping-wax-avgR=$avg"
        if ($walkRange -lt 20 -or $avg -gt 100) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "wake" {
        # 前置：walksleep 已入睡。先落座等穿透清除（450ms），再抖动唤醒。
        $pet = Get-Pet
        $p = PX 58 127 $pet
        [void][PV]::SetCursorPos($p[0], $p[1])
        Start-Sleep -Milliseconds 450
        [void][PV]::SetCursorPos(($p[0] + 5), $p[1])
        Start-Sleep -Milliseconds 700
        $bmp = Print-Pet $pet.hwnd $pet.W $pet.Ht
        $bmp.Save("$PrintDir/verify-wake.png")
        $avg = Band-AvgR $bmp 45 115 90 145
        Write-Host "RESULT awake-wax-avgR=$avg"
        if ($avg -lt 100) { Write-Host "FAIL"; exit 1 }  # 标定：清醒≈115 / 入睡≈82
        Write-Host "PASS"
    }
    "bubble" {
        # 前置：实例带 GESSO_PET_NOTICE 启动，通知在 ~3s 注入、气泡约 4s
        Start-Sleep -Milliseconds 4200
        $pet = Get-Pet
        $bmp = Print-Pet $pet.hwnd $pet.W $pet.Ht
        $bmp.Save("$PrintDir/verify-bubble.png")
        $bg = Count-Band $bmp 20 23 96 48 { param($c) $c.R -ge 240 -and $c.G -ge 240 -and $c.B -ge 240 }
        $ink = Count-Band $bmp 20 23 96 48 { param($c) $c.R -ge 8 -and $c.R -le 95 -and $c.G -le 95 -and $c.B -le 105 }
        Write-Host "RESULT bubble-bg=$bg text-ink=$ink"
        if ($bg -lt 400 -or $ink -lt 40) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "menuuia" {
        # UIA 路线：#32768 菜单窗 → AutomationElement 树 → 按序号找
        # 「模拟事件」(第4项) → 点击 → 子菜单第1项（新结果）→ 点击。
        # 结果由 bash 侧 grep 日志（菜单命令：30）。
        $pet = Get-Pet
        $c = PX 58 127 $pet
        [void][PV]::SetCursorPos($c[0], $c[1])
        Start-Sleep -Milliseconds 450
        [PV]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [PV]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 700
        Add-Type -AssemblyName UIAutomationClient
        $menu = [PV]::FindWindowW("#32768", $null)
        Write-Host "RESULT menu-hwnd=$menu"
        if ($menu -eq [IntPtr]::Zero) { Write-Host "FAIL(no-menu)"; exit 1 }
        $root = [System.Windows.Automation.AutomationElement]::FromHandle($menu)
        $items = $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.Condition]::TrueCondition)
        $names = @()
        foreach ($it in $items) { $names += $it.Current.Name }
        Write-Host ("RESULT items: " + ($names -join " | "))
        if ($items.Count -lt 4) { Write-Host "FAIL(few-items)"; exit 1 }
        $target = $items[3]  # 第4项 = 模拟事件
        $pt = $target.GetClickablePoint()
        [void][PV]::SetCursorPos([int]$pt.X, [int]$pt.Y)
        Start-Sleep -Milliseconds 120
        [PV]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 40
        [PV]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 600
        # 子菜单：新的 #32768（最近创建的）；点第一项
        $sub = [PV]::FindWindowW("#32768", $null)
        $sroot = [System.Windows.Automation.AutomationElement]::FromHandle($sub)
        $sitems = $sroot.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.Condition]::TrueCondition)
        Write-Host ("RESULT sub-items: " + (($sitems | ForEach-Object { $_.Current.Name }) -join " | "))
        if ($sitems.Count -lt 1) { Write-Host "FAIL(no-sub)"; exit 1 }
        $pt2 = $sitems[0].GetClickablePoint()
        [void][PV]::SetCursorPos([int]$pt2.X, [int]$pt2.Y)
        Start-Sleep -Milliseconds 120
        [PV]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 40
        [PV]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
        Write-Host "PASS(clicked)"
    }
    "menugeo" {
        # 几何法：菜单窗矩形按 8 行（7 项+分隔线）均分，点第 4 行（模拟事件），
        # 子菜单开在上层 #32768，点其第 1 行（新结果）。
        $pet = Get-Pet
        $c = PX 58 127 $pet
        [void][PV]::SetCursorPos($c[0], $c[1])
        Start-Sleep -Milliseconds 450
        [PV]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [PV]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 700
        $menu = [PV]::FindWindowW("#32768", $null)
        if ($menu -eq [IntPtr]::Zero) { Write-Host "FAIL(no-menu)"; exit 1 }
        $mr = New-Object PV+R
        [void][PV]::GetWindowRect($menu, [ref]$mr)
        $rows = 8.0
        $itemH = ($mr.B - $mr.T) / $rows
        $x = [int](($mr.L + $mr.Rt) / 2)
        $y = [int]($mr.T + 3.5 * $itemH)
        Write-Host "RESULT menu rect=$($mr.L),$($mr.T)-$($mr.Rt)x$($mr.B) click=($x,$y)"
        [void][PV]::SetCursorPos($x, $y)
        Start-Sleep -Milliseconds 150
        [PV]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 40
        [PV]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 700
        $sub = [PV]::FindWindowW("#32768", $null)
        $sr = New-Object PV+R
        [void][PV]::GetWindowRect($sub, [ref]$sr)
        $sx = [int](($sr.L + $sr.Rt) / 2)
        $sy = [int]($sr.T + (($sr.B - $sr.T) / 3.0))
        Write-Host "RESULT sub rect=$($sr.L),$($sr.T)-$($sr.Rt)x$($sr.B) click=($sx,$sy)"
        [void][PV]::SetCursorPos($sx, $sy)
        Start-Sleep -Milliseconds 150
        [PV]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 40
        [PV]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 800
        Write-Host "PASS(clicked)"
    }
    "bubblecap" {
        # 立即抓取（配合 menu 阶段：选中通知项后 4s 内气泡应在场）
        $pet = Get-Pet
        $bmp = Print-Pet $pet.hwnd $pet.W $pet.Ht
        $bmp.Save("$PrintDir/verify-menu-bubble.png")
        $bg = Count-Band $bmp 20 23 96 48 { param($c) $c.R -ge 240 -and $c.G -ge 240 -and $c.B -ge 240 }
        $ink = Count-Band $bmp 20 23 96 48 { param($c) $c.R -ge 8 -and $c.R -le 95 -and $c.G -le 95 -and $c.B -le 105 }
        Write-Host "RESULT bubble-bg=$bg text-ink=$ink"
        if ($bg -lt 400 -or $ink -lt 40) { Write-Host "FAIL"; exit 1 }
        Write-Host "PASS"
    }
    "menu" {
        # 真实右键打开菜单（授予前台权），向 #32768 菜单窗投递键盘消息选择
        # 「模拟事件 → 新结果」。结果由 bash 侧 grep 应用日志（菜单命令：30）。
        $pet = Get-Pet
        $c = PX 58 127 $pet
        [void][PV]::SetCursorPos($c[0], $c[1])
        Start-Sleep -Milliseconds 450
        [PV]::mouse_event(0x0008, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 60
        [PV]::mouse_event(0x0010, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 600
        $menu = [PV]::FindWindowW("#32768", $null)
        Write-Host "RESULT menu-hwnd=$menu"
        if ($menu -eq [IntPtr]::Zero) { Write-Host "FAIL(no-menu)"; exit 1 }
        # VK: DOWN=0x28(scan 0x50) RIGHT=0x27(0x4D) ENTER=0x0D(0x1C)
        foreach ($vk in @(@(0x28, 0x50), @(0x28, 0x50), @(0x28, 0x50), @(0x27, 0x4D), @(0x0D, 0x1C))) {
            $lp = [IntPtr](0x1 -bor ([int]$vk[1] -shl 16))
            [void][PV]::PostMessageW($menu, 0x0100, [IntPtr]$vk[0], $lp)
            Start-Sleep -Milliseconds 120
            [void][PV]::PostMessageW($menu, 0x0101, [IntPtr]$vk[0], $lp)
            Start-Sleep -Milliseconds 120
        }
        Start-Sleep -Milliseconds 500
        # 兜底 ESC 清场（若选择未生效菜单仍开着）
        [void][PV]::PostMessageW($menu, 0x0100, [IntPtr]0x1B, [IntPtr](0x1 -bor (0x01 -shl 16)))
        Write-Host "PASS(posted)"
    }
    default { Write-Host "未知阶段 $Phase"; exit 2 }
}
