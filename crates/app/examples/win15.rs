//! win15 spike · Windows 原生视频壁纸（MF SourceReader + DXGI swapchain 直挂）
//!
//! B 方案：本地播放器同构的最短呈现路径——
//!   MF SourceReader 解码（BGRA，内置转换器缩放到主屏）→ UpdateSubresource 上载
//!   → flip-model swapchain（直挂 WorkerW 子窗口，Present(1) 锁 vsync）→ DWM 合成
//! 全程不经过 WebView2；视频流纯解码无音频管线，静音契约天然成立。
//!
//! 背景：WebView2 windowed hosting 呈现路径实测卡顿——v0.1.0 / e91e4bb / HEAD
//! 三代构建、gesso:// 与回环 HTTP 两种传输、4GB/133MB/内置样例三类素材全卡，
//! 而同文件本地播放器与 Edge 丝滑（2026-10-11 三方对照）；页侧探针全绿，病灶
//! 在呈现交接层。本机系统另被精简过（System32 缺 Windows.UI.Composition.dll
//! 等 WinRT UI 运行时，WinRT Compositor 激活 E_ACCESSDENIED）——故走经典
//! DXGI 路线，对精简系统免疫。
//!
//! 运行：cargo run -p gesso-app --example win15 -- <视频路径>
//! 验收：视频丝滑（对照 WebView2 路径的卡顿）；ESC 退出并触发桌面重绘。

#[cfg(target_os = "windows")]
mod imp {
    use std::process::ExitCode;
    use std::time::Duration;

    use windows::core::Interface as _;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDeviceAndSwapChain, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
        D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_MODE_SCALING_UNSPECIFIED,
        DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED, DXGI_RATIONAL, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::{
        IDXGISwapChain1, DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        DXGI_USAGE_RENDER_TARGET_OUTPUT,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateSolidBrush, InvalidateRect, RedrawWindow, RDW_ALLCHILDREN, RDW_INVALIDATE,
    };
    use windows::Win32::Media::MediaFoundation::{
        IMFSample, IMFSourceReader, MFCreateMediaType, MFCreateSourceReaderFromURL, MFStartup,
        MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READERF_ENDOFSTREAM,
        MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
    };
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, FindWindowW,
        GetSystemMetrics, PostQuitMessage, RegisterClassW, SendMessageTimeoutW, SetParent,
        SetWindowPos, ShowWindow, HWND_BOTTOM, SHOW_WINDOW_CMD, SMTO_NORMAL, SM_CXSCREEN,
        SM_CYSCREEN, SWP_NOACTIVATE, WM_DESTROY, WM_KEYDOWN, WNDCLASSW, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_POPUP,
    };

    const WM_PROGMAN_SPAWN_WORKERW: u32 = 0x052C;

    pub fn run(video: &str) -> windows::core::Result<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

            // 窗口：无边框 toolwindow，主屏物理像素，先挂 WorkerW 后建内容
            let hinstance = GetModuleHandleW(None)?;
            let class_name = w!("GessoWin15");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: hinstance.into(),
                lpszClassName: class_name,
                hbrBackground: CreateSolidBrush(COLORREF(0)),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            let hwnd = CreateWindowExW(
                WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class_name,
                w!("Gesso win15 spike"),
                WS_POPUP,
                0,
                0,
                sw,
                sh,
                None,
                None,
                Some(hinstance.into()),
                None,
            )?;
            mount_workerw(hwnd);
            let _ = ShowWindow(hwnd, SHOW_WINDOW_CMD(0x00000005)); // SW_SHOW

            let (device, context, swapchain) = create_swapchain(hwnd, sw, sh)?;

            MFStartup(MF_VERSION, 0)?;
            let reader = create_reader(video, sw, sh)?;

            println!(">> win15 已上屏，播放中（ESC 退出）");
            play_loop(&reader, &device, &context, &swapchain, sw, sh)?;

            let _ = DestroyWindow(hwnd);
            if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
                let _ = RedrawWindow(Some(progman), None, None, RDW_INVALIDATE | RDW_ALLCHILDREN);
            }
            let _ = InvalidateRect(None, None, false);
        }
        Ok(())
    }

    unsafe fn create_swapchain(
        hwnd: HWND,
        sw: i32,
        sh: i32,
    ) -> windows::core::Result<(ID3D11Device, ID3D11DeviceContext, IDXGISwapChain1)> {
        let desc = DXGI_SWAP_CHAIN_DESC {
            BufferDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_DESC {
                Width: sw as u32,
                Height: sh as u32,
                RefreshRate: DXGI_RATIONAL {
                    Numerator: 0,
                    Denominator: 0,
                },
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                ScanlineOrdering: DXGI_MODE_SCANLINE_ORDER_UNSPECIFIED,
                Scaling: DXGI_MODE_SCALING_UNSPECIFIED,
            },
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            OutputWindow: hwnd,
            Windowed: true.into(),
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            Flags: 0,
        };
        let mut device = None;
        let mut context = None;
        let mut swapchain = None;
        D3D11CreateDeviceAndSwapChain(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            windows::Win32::Foundation::HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            None,
            D3D11_SDK_VERSION,
            Some(&desc),
            Some(&mut swapchain),
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        Ok((
            device.unwrap(),
            context.unwrap(),
            swapchain.unwrap().cast::<IDXGISwapChain1>()?,
        ))
    }

    unsafe fn create_reader(
        video: &str,
        sw: i32,
        sh: i32,
    ) -> windows::core::Result<IMFSourceReader> {
        let wide: Vec<u16> = video.encode_utf16().chain(std::iter::once(0)).collect();
        // 缩放到主屏分辨率需要高级视频处理（基础模式只做解码器直出转换）
        let mut attrs_opt: Option<windows::Win32::Media::MediaFoundation::IMFAttributes> = None;
        windows::Win32::Media::MediaFoundation::MFCreateAttributes(&mut attrs_opt, 1)?;
        let attrs = attrs_opt.unwrap();
        attrs.SetUINT32(
            &windows::Win32::Media::MediaFoundation::MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING,
            1,
        )?;
        let reader = MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), &attrs)?;
        // 只开视频流；输出 BGRA、缩放到主屏尺寸（swapchain/窗同尺寸免二次缩放）
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, true)?;
        let mt = MFCreateMediaType()?;
        mt.SetGUID(
            &MF_MT_MAJOR_TYPE,
            &windows::Win32::Media::MediaFoundation::MFMediaType_Video,
        )?;
        mt.SetGUID(
            &MF_MT_SUBTYPE,
            &windows::Win32::Media::MediaFoundation::MFVideoFormat_ARGB32,
        )?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, ((sw as u64) << 32) | sh as u64)?;
        reader.SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32, None, &mt)?;
        Ok(reader)
    }

    unsafe fn play_loop(
        reader: &IMFSourceReader,
        _device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        swapchain: &IDXGISwapChain1,
        sw: i32,
        _sh: i32,
    ) -> windows::core::Result<()> {
        let backbuffer: ID3D11Texture2D = swapchain.GetBuffer(0)?;

        let mut qpf = 0i64;
        let _ = QueryPerformanceFrequency(&mut qpf);
        let mut first_ts: Option<i64> = None;
        let mut first_qpc = 0i64;
        let pv = windows::Win32::System::Com::StructuredStorage::PROPVARIANT::default();

        loop {
            if GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0 {
                println!(">> ESC，退出");
                break;
            }
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample: Option<IMFSample> = None;
            reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                None,
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            )?;
            if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 || sample.is_none() {
                // 循环：回到起点并重定基时间戳
                reader.SetCurrentPosition(&windows::core::GUID::zeroed(), &pv)?;
                first_ts = None;
                continue;
            }
            let sample = sample.unwrap();
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data = std::ptr::null_mut::<u8>();
            let mut max_len = 0u32;
            let mut len = 0u32;
            buffer.Lock(&mut data, Some(&mut max_len), Some(&mut len))?;

            // PTS 节拍：按内容时间轴对齐墙钟（QPC），早到才睡，迟到此帧即present
            let mut qpc = 0i64;
            let _ = QueryPerformanceCounter(&mut qpc);
            if first_ts.is_none() {
                first_ts = Some(ts);
                first_qpc = qpc;
            }
            let due = first_qpc + ((ts - first_ts.unwrap()) * qpf) / 10_000_000;
            if due - qpc > qpf / 1000 {
                let sleep_ms = ((due - qpc) * 1000 / qpf - 1).max(0);
                std::thread::sleep(Duration::from_millis(sleep_ms as u64));
                loop {
                    let mut n = 0i64;
                    let _ = QueryPerformanceCounter(&mut n);
                    if n >= due {
                        break;
                    }
                }
            }

            context.UpdateSubresource(&backbuffer, 0, None, data as *const _, sw as u32 * 4, 0);
            let _ = buffer.Unlock();
            swapchain.Present(1, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }

    fn mount_workerw(hwnd: HWND) {
        unsafe {
            if let Ok(progman) = FindWindowW(w!("Progman"), PCWSTR::null()) {
                let mut result = 0usize;
                let _ = SendMessageTimeoutW(
                    progman,
                    WM_PROGMAN_SPAWN_WORKERW,
                    WPARAM(0),
                    LPARAM(0),
                    SMTO_NORMAL,
                    1000,
                    Some(&mut result),
                );
            }
            // 常规布局下 SHELLDLL_DefView 是 Progman 的直接子窗口
            let defview = FindWindowExW(
                Some(FindWindowW(w!("Progman"), PCWSTR::null()).unwrap_or_default()),
                None,
                PCWSTR::null(),
                w!("SHELLDLL_DefView"),
            )
            .ok();
            if let Ok(worker) = FindWindowExW(None, defview, w!("WorkerW"), None) {
                let _ = SetParent(hwnd, Some(worker));
                let _ = SetWindowPos(
                    hwnd,
                    Some(HWND_BOTTOM),
                    0,
                    0,
                    GetSystemMetrics(SM_CXSCREEN),
                    GetSystemMetrics(SM_CYSCREEN),
                    SWP_NOACTIVATE,
                );
            } else {
                eprintln!(">> WorkerW 未找到（explorer 未就绪？），本轮贴顶层");
            }
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_KEYDOWN if wparam.0 as u32 == VK_ESCAPE.0 as u32 => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    pub fn main() -> ExitCode {
        let video = std::env::args()
            .nth(1)
            .unwrap_or_else(|| r"C:\Users\zhang\Downloads\2035327141\Terre.mp4".to_string());
        match run(&video) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!(">> win15 失败：{e}");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn main() -> std::process::ExitCode {
    imp::main()
}

#[cfg(not(target_os = "windows"))]
fn main() -> std::process::ExitCode {
    println!("win15 是 Windows 专用 spike");
    std::process::ExitCode::SUCCESS
}
