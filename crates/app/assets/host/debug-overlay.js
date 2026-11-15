// TEMP-DEBUG: 效果栈诊断悬浮层（阶段 B 联调用，验证后移除）
(function () {
  window.__logs = [];
  var _l = console.log, _e = console.error;
  console.log = function () {
    var s = Array.prototype.slice.call(arguments).join(" ");
    window.__logs.push("LOG " + s);
    _l.apply(console, arguments);
  };
  console.error = function () {
    var s = Array.prototype.slice.call(arguments).join(" ");
    window.__logs.push("ERR " + s);
    _e.apply(console, arguments);
  };
  window.addEventListener("error", function (ev) {
    window.__logs.push("UNCAUGHT " + ev.message + " @" + ev.lineno);
  });
  var el = document.createElement("div");
  el.style.cssText = "position:fixed;top:4px;left:4px;z-index:9999;color:#0f0;font:bold 13px monospace;background:rgba(0,0,0,.75);padding:4px 6px;white-space:pre;max-width:1400px;";
  document.body.appendChild(el);
  var NL = String.fromCharCode(10);
  setInterval(function () {
    var s = window.__sceneDbg ? window.__sceneDbg() : "no hook";
    el.textContent = JSON.stringify(s) + NL + (window.__logs || []).slice(-8).join(NL);
  }, 400);
})();
