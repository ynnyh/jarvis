//! avatar 走丢守护：每 5s 巡检小人窗口中心是否落在任一显示器的可见工作区内，
//! 不在则贴回最近显示器的工作区底边（任务栏上缘）。
//!
//! 为什么放 Rust：前端漫游引擎靠 rAF/定时器驱动，小人一旦沉到屏幕外，WebView
//! 被 Chromium 判定遮挡 —— rAF 停摆、定时器被节流，决策循环死锁，小人永久消失
//! （真机实锤：窗口中心 cy 恒卡在屏外同一值，重启应用才能找回）。OS 线程不经过
//! WebView，不受任何前端节流影响，是最后一道兜底；前端侧修复见
//! usePetRoamer.ts（漫游全程物理坐标 + setTimeout 步进）。
//!
//! 只在「中心落在所有屏幕工作区之外」时出手：正常漫游（中心在屏内，窗口其余
//! 部分悬出屏外是常态）零干扰；工具窗故意隐藏小人（is_visible=false）时跳过；
//! 与拖拽/漫游并发无冲突——拖拽中中心恒在屏内，守护不触发。

use std::thread;
use std::time::Duration;

use tauri::{Manager, PhysicalPosition};

const CHECK_INTERVAL_SECS: u64 = 5;
/// config.petSizePx 缺失时的兜底（与前端默认一致）
const DEFAULT_PET_SIZE_PX: f64 = 96.0;

pub fn spawn(app: tauri::AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(CHECK_INTERVAL_SECS));
        if let Err(e) = tick(&app) {
            log::warn!("[avatar-guard] tick 失败: {e}");
        }
    });
}

fn tick(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(win) = app.get_webview_window("avatar") else {
        return Ok(());
    };
    if !win.is_visible().map_err(|e| e.to_string())? {
        return Ok(());
    }
    let pos = win.outer_position().map_err(|e| e.to_string())?;
    let size = win.outer_size().map_err(|e| e.to_string())?;
    let center_x = pos.x + size.width as i32 / 2;
    let center_y = pos.y + size.height as i32 / 2;

    let monitors = win.available_monitors().map_err(|e| e.to_string())?;
    let inside = monitors.iter().any(|m| {
        let wa = m.work_area();
        center_x >= wa.position.x
            && center_x < wa.position.x + wa.size.width as i32
            && center_y >= wa.position.y
            && center_y < wa.position.y + wa.size.height as i32
    });
    if inside {
        return Ok(());
    }

    // 沉到屏外：挑中心欧氏距离最近的一屏，把小人贴回其工作区底边
    let Some(target) = monitors.iter().min_by_key(|m| {
        let wa = m.work_area();
        let mx = wa.position.x + wa.size.width as i32 / 2;
        let my = wa.position.y + wa.size.height as i32 / 2;
        let dx = (center_x - mx) as i64;
        let dy = (center_y - my) as i64;
        dx * dx + dy * dy
    }) else {
        return Ok(());
    };
    let wa = target.work_area();
    let pet_half = (load_pet_size_px() / 2.0 * target.scale_factor()).round() as i32;
    let new_cx = center_x.clamp(
        wa.position.x + pet_half,
        wa.position.x + wa.size.width as i32 - pet_half,
    );
    let new_cy = wa.position.y + wa.size.height as i32 - pet_half;
    let top_x = new_cx - size.width as i32 / 2;
    let top_y = new_cy - size.height as i32 / 2;
    win.set_position(PhysicalPosition::new(top_x, top_y))
        .map_err(|e| e.to_string())?;
    log::info!(
        "[avatar-guard] 小人中心 ({center_x},{center_y}) 落在屏外，已贴回 ({new_cx},{new_cy})"
    );
    Ok(())
}

fn load_pet_size_px() -> f64 {
    crate::settings::load_raw_config()
        .and_then(|cfg| cfg.get("petSizePx").and_then(|v| v.as_f64()))
        .unwrap_or(DEFAULT_PET_SIZE_PX)
}
