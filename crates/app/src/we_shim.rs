//! WE web API shim：注入到拷贝后用户入口 HTML 的 `<head>` 最前，让为
//! Wallpaper Engine 写的网页在 Gesso 宿主页的沙箱 iframe 里也能跑。
//!
//! 数据源：Gesso 宿主页以 postMessage 推送（暂停/fps/鼠标/时间）。音频在
//! M6 无真实来源——audio listener 收到静音数组（60 或 128 路 0）。
//! 参考：docs.wallpaperengine.io/en/web/api/propertylistener.html

/// 注入脚本（自执行，末尾带换行）。
pub fn shim_script() -> String {
    r#"<script>
(function () {
  "use strict";
  // —— WE 全局 API：注册型，页面可能在任意时刻赋值/调用 ——
  window.wallpaperRegisterAudioListener = function (cb) {
    window.__weAudioCb = cb;
    // 无音频源：推一次 60 路静音（WE 常用 60/128）
    try { cb(new Array(60).fill(0)); } catch (e) {}
  };
  window.wallpaperRegisterMediaStatusListener = function (cb) { window.__weMediaCb = cb; };
  window.wallpaperRegisterMediaPropertiesListener = function (cb) { window.__weMediaPropsCb = cb; };
  window.wallpaperMediaStatus = function () { return {}; };
  window.wallpaperMediaIntegrationPlayback = { state: "stopped", name: "", artist: "" };
  window.wallpaperAddEventListener = function () {};
  window.wallpaperRemoveEventListener = function () {};
  window.wallpaperRequestImage = function () {};
  window.wallpaperCancelImageRequests = function () {};
  window.wallpaperOpenURL = function () {};   // 沙箱禁导航：no-op
  window.wallpaperOpenWorkshop = function () {};
  window.wallpaperUrl = function () { return ""; };
  window.wallpaperPluginListener = {};
  window.wallpaperPlugin = function () { return {}; };
  window.wallpaperColorManagement = {};

  // —— 接收 Gesso 宿主页消息（父窗口 postMessage）——
  function call(method) {
    var l = window.wallpaperPropertyListener;
    var args = Array.prototype.slice.call(arguments, 1);
    if (l && typeof l[method] === "function") {
      try { l[method].apply(l, args); } catch (e) {}
    }
  }
  window.addEventListener("message", function (e) {
    var d = e.data;
    if (!d || d.__gesso === undefined) return;
    switch (d.__gesso) {
      case "pause":  call("setPaused", true);  break;
      case "resume": call("setPaused", false); break;
      case "fps":    call("applyGeneralProperties", { fps: d.fps }); break;
      case "mouse":
        // WE 无统一鼠标回调；转发为自定义事件，供需要的页面使用
        try {
          window.dispatchEvent(new CustomEvent("wallpaperMouse", { detail: d }));
        } catch (err) {}
        break;
      case "time":
        try {
          window.dispatchEvent(new CustomEvent("wallpaperTime", { detail: d.t }));
        } catch (err) {}
        break;
    }
  });
})();
</script>
"#
    .to_string()
}

/// 把 shim 注入到入口 HTML：插在 `<head>` 之后最前；无 head 则前插到文档开头。
pub fn inject(html: &str) -> String {
    let shim = shim_script();
    let lower = html.to_ascii_lowercase();
    match lower.find("<head") {
        Some(head_start) => {
            // 找该 <head ...> 标签的结束 '>'
            if let Some(gt) = html[head_start..].find('>') {
                let at = head_start + gt + 1;
                let mut out = String::with_capacity(html.len() + shim.len());
                out.push_str(&html[..at]);
                out.push_str(&shim);
                out.push_str(&html[at..]);
                return out;
            }
        }
        None => {}
    }
    format!("{shim}{html}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injects_into_head() {
        let html = "<html><head><title>x</title></head><body></body></html>";
        let out = inject(html);
        let head_end = out.to_ascii_lowercase().find("<head").unwrap()
            + out[out.to_ascii_lowercase().find("<head").unwrap()..]
                .find('>')
                .unwrap()
            + 1;
        assert!(
            out[head_end..].starts_with("<script>"),
            "shim 紧贴 <head> 之后"
        );
        assert!(out.contains("wallpaperRegisterAudioListener"));
        assert!(out.contains("<title>x</title>"));
    }

    #[test]
    fn inject_prepends_without_head() {
        let out = inject("<html><body>hi</body></html>");
        assert!(out.starts_with("<script>"));
    }
}
