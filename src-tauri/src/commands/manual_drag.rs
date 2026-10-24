//! Windows 自驱移窗（绕开 NC move loop 的顶边钳制）
//!
//! 原理：Rust 线程 ~1ms 轮询 GetCursorPos（物理像素）+
//! GetAsyncKeyState(VK_LBUTTON) 下降沿检测松手。按住期间 SetWindowPos(SWP_NOSENDCHANGING)
//! 把窗口设为「window_center = cursor - grab_offset」，让窗口中心永远追光标。
//! **完全不进 DefWindowProc 的 NC move loop** → Windows 内核"窗口顶边钳到工作区顶"
//! 的钳制不生效 → 小人可拖到屏幕任意位置，含上半屏顶。
//!
//! 跨多屏 DPI：每帧按窗口当前物理尺寸算半宽，窗口中心对齐光标，
//! 跨屏后 DWM 改尺寸也不飞。松手下降沿由本线程检测（靠前端会死锁）。
//!
//! 命令必须放在本模块顶层（不分子 mod），否则 Tauri 的 __cmd__ helper 无法被
//! `generate_handler![commands::manual_drag_start]` 解析到。平台差异用 cfg 内联区分。

#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "windows")]
use std::sync::OnceLock;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{HWND, POINT, RECT};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowRect, SetWindowPos, SET_WINDOW_POS_FLAGS, SWP_NOACTIVATE,
    SWP_NOSENDCHANGING, SWP_NOSIZE, SWP_NOZORDER,
};

#[cfg(target_os = "windows")]
struct DragState {
    hwnd_isize: isize,
    // grab 偏移：按下时光标相对窗口中心的物理偏移（确立一次，全程恒定的"点中小人哪里"）。
    // 跨多屏 DPI 时窗口物理尺寸被 DWM 改（同逻辑尺寸在不同 scale 屏物理像素不同），但
    // grab 是"光标在窗口中心的相对位置"，与窗口尺寸无关 → 跨屏仍准。每帧按当前物理半宽
    // 还原"光标 - grab = 窗口中心" → target_left = cur - half_w - grab_dx，跨屏不飞。
    grab_dx: f64,
    grab_dy: f64,
    grab_set: bool,
}

#[cfg(target_os = "windows")]
struct Sess {
    active: AtomicBool,
    state: std::sync::Mutex<Option<DragState>>,
}

#[cfg(target_os = "windows")]
static S_INSTANCE: OnceLock<Sess> = OnceLock::new();

#[cfg(target_os = "windows")]
fn sess() -> &'static Sess {
    S_INSTANCE.get_or_init(|| Sess {
        active: AtomicBool::new(false),
        state: std::sync::Mutex::new(None),
    })
}

#[cfg(target_os = "windows")]
fn poll_loop(window: tauri::WebviewWindow) {
    // 节奏 1ms：近似系统鼠标消息频率上限，验证尽量苛刻。
    // **松手必须由本线程主动检测结束会话**：不能全靠前端 onWindowMouseUp →
    // manual_drag_stop。因为本线程在松手后若不停止，会继续 SetWindowPos 把窗口追到光标
    // → 窗口持续移动 → onMoved 持续触发 → 前端 scheduleDragEnd 的防抖被无限刷新
    // → finishDrag 永不执行 → manual_drag_stop 永不调 → active 永真 → 死锁：松手后小人
    // 继续跟手、点不到任何东西。本线程是松手的唯一可靠哨兵。
    //
    // 下降沿判定（was_down → now up）：避免单帧采样噪声误停；按下→松开那帧才结束。
    // 松手即向前端广播 manual_drag_released：onMoved 防抖只能靠「窗口停止移动」间接
    // 猜松手，拖拽途中窗口移动事件断流 >200ms（用户暂停、跨屏 DPI 重排、渲染卡顿）
    // 就会误判松手把拖拽拆掉（实测「拖着走老是断掉」）——权威信号必须由这里推送。
    let mut was_down = true; // 会话由按下启动，初值即按下态
    while sess().active.load(Ordering::SeqCst) {
        let down = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 };
        if was_down && !down {
            // 真实松手：结束会话，前端经 manual_drag_released 事件立即 finishDrag
            use tauri::Emitter;
            let _ = window.emit("manual_drag_released", ());
            manual_drag_stop();
            break;
        }
        was_down = down;

        let mut g = sess().state.lock().unwrap();
        let st = match g.as_mut() {
            Some(st) => st,
            None => break,
        };
        unsafe {
            let mut cur = POINT::default();
            if GetCursorPos(&mut cur).is_err() {
                continue;
            }
            let hwnd = HWND(st.hwnd_isize as *mut _);
            let mut rect = RECT::default();
            if GetWindowRect(hwnd, &mut rect).is_err() {
                continue;
            }
            // 小人恒居窗口正中心。让窗口中心始终追上光标 → target_left = cur - 半宽物理。
            // 半宽每帧读窗口实际物理尺寸，跨多屏 DPI 时 DWM 会改窗口物理尺寸（同逻辑尺寸
            // 在不同 scale 屏物理像素不同），用恒定半宽会令光标跨屏后偏离小人中心 → "指针飞"。
            // 每帧按当前物理尺寸算半宽，窗口中心永远对齐光标 → 跨屏也不飞。
            // 命中点不一定是窗口正中心（用户可能点在偏左/右），故用按下时记录的 grab 偏移
            // 占半宽比例，跨屏后按新尺寸还原同一比例点对齐光标。
            let half_w = (rect.right - rect.left) as f64 / 2.0;
            let half_h = (rect.bottom - rect.top) as f64 / 2.0;
            let center_x = rect.left as f64 + half_w;
            let center_y = rect.top as f64 + half_h;
            // 光标相对窗口中心的偏移（按下时确立，全程恒定的"点中小人哪里"）。
            // 第一帧用此刻光标 vs 窗口中心确立 grab 偏移。
            if !st.grab_set {
                st.grab_dx = cur.x as f64 - center_x;
                st.grab_dy = cur.y as f64 - center_y;
                st.grab_set = true;
            }
            let target_left = (cur.x as f64 - half_w - st.grab_dx).round() as i32;
            let target_top = (cur.y as f64 - half_h - st.grab_dy).round() as i32;
            let flags: SET_WINDOW_POS_FLAGS =
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING;
            let _ = SetWindowPos(hwnd, None, target_left, target_top, 0, 0, flags);
        }
        drop(g);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[tauri::command]
pub fn manual_drag_start(
    #[allow(unused_variables)] window: tauri::WebviewWindow,
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // 重复触发先收尾，避免泄漏线程
        if sess().active.swap(true, Ordering::SeqCst) {
            manual_drag_stop();
            sess().active.store(true, Ordering::SeqCst);
        }

        let hwnd = window.hwnd().map_err(|e| e.to_string())?;
        // grab 偏移在 poll 第一帧用「光标 - 窗口当前中心」确立（此时窗口尺寸尚未跨屏，
        // 几何真实），无需在 start 读光标/几何。grab_set=false 让首帧确立。
        *sess().state.lock().unwrap() = Some(DragState {
            hwnd_isize: hwnd.0 as isize,
            grab_dx: 0.0,
            grab_dy: 0.0,
            grab_set: false,
        });
        std::thread::spawn(move || poll_loop(window));
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("manual_drag 仅 Windows 支持".into())
    }
}

#[tauri::command]
pub fn manual_drag_stop() {
    #[cfg(target_os = "windows")]
    {
        sess().active.store(false, Ordering::SeqCst);
        *sess().state.lock().unwrap() = None;
    }
}
