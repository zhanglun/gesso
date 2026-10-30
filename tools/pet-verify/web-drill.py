#!/usr/bin/env python3
"""Gesso GPUI↔Web 双入口并发演练（自启动沙箱实例，全程无人值守）。

用法：python3 tools/pet-verify/web-drill.py [gesso 二进制]   （默认 target/debug/gesso）
前置：cargo build -p gesso-app（钩子仅 debug 构建存在）。

机制：GESSO_PET_GPUI_DRILL 按秒向引擎队列注入 GPUI 侧 EngineAction（与领养
小窗/设置页同一通道，见 main.rs parse_drill_step），本脚本同时作为 Web actor
走 HTTP 写通道——双入口真并发。实机桌面会跑约 25s：会短暂打开领养小窗、
指派样例壁纸并抢一次焦点，属演练预期（同 GESSO_PET_ADOPT 实践）。

断言（不变量而非具体交错）：
  - Web 侧确定性 409（陈旧 revision）→ 刷新 → 重试成功；
  - 双入口交叉修改同一领养会话（GPUI reroll/select，Web 覆盖选择）状态一致；
  - GPUI confirm → adopted，且宠物即最后确认的候选；
  - Web reset.request → GPUI reset 清除 → not_adopted；非待确认态 confirm 拒绝；
  - adoption_revision 全程单调不减；进程不崩；退出后端口关闭、存档落盘一致。
"""

import json
import os
import queue
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid
from http.cookiejar import CookieJar

BIN = sys.argv[1] if len(sys.argv) > 1 else "target/debug/gesso"
# GPUI 侧时间轴：2 开窗 / 3 开始 / 10 reroll / 12 选 1 / 14 确认 / 18 确认重置
SCHEDULE = "2:open,3:start,10:reroll,12:select:1,14:confirm,18:reset_confirm"

PASS = 0
FAIL = 0
MONO = [0]  # 观测到的最大 adoption_revision
MONO_VIOLATIONS = []


def check(name, cond, detail=""):
    global PASS, FAIL
    PASS += bool(cond)
    FAIL += not cond
    print(("PASS" if cond else "FAIL"), name, detail)


def launch(home):
    env = dict(
        os.environ,
        HOME=home,
        GESSO_LOCK="drill",
        GESSO_PET_WEB="1",
        GESSO_PET_WEB_ECHO="1",
        GESSO_PET_WEB_NO_OPEN="1",
        GESSO_PET_GPUI_DRILL=SCHEDULE,
    )
    proc = subprocess.Popen(
        [BIN], env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True
    )
    lines = queue.Queue()
    threading.Thread(target=lambda: [lines.put(l) for l in proc.stdout], daemon=True).start()
    base = None
    deadline = time.time() + 30
    while time.time() < deadline:
        if proc.poll() is not None:
            break
        try:
            line = lines.get(timeout=1)
        except queue.Empty:
            continue
        if "bootstrap url:" in line:
            base = line.split("bootstrap url:", 1)[1].strip()
            break
    return proc, lines, base


JAR = CookieJar()  # 单会话演练：全局唯一（web-e2e 教训：空 jar falsy，必须 is None 判断）


def req(method, path, body=None, jar=None, retries=3):
    if jar is None:
        jar = JAR
    r = urllib.request.Request(
        BASE + path,
        method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Content-Type": "application/json", "Origin": BASE},
    )
    # loopback 流量绝不走系统代理（代理环境变量会拦 127.0.0.1 → 502）
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}), urllib.request.HTTPCookieProcessor(jar)
    )
    last = (-1, b"no-attempt")
    for attempt in range(retries):
        try:
            resp = opener.open(r, timeout=3)
            return resp.status, resp.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()
        except Exception as e:  # 启动时序偶发：退避重试
            last = (-1, f"{type(e).__name__}".encode())
            time.sleep(0.4 * (attempt + 1))
    return last


def state():
    status, body = req("GET", "/api/state")
    snap = json.loads(body)
    rev = snap.get("adoption_revision", 0)
    if rev < MONO[0]:
        MONO_VIOLATIONS.append(f"{rev} < {MONO[0]}")
    MONO[0] = max(MONO[0], rev)
    return snap


def op(payload, revision):
    return req(
        "POST",
        "/api/adoption/op",
        {"request_id": str(uuid.uuid4()), "revision": revision, **payload},
    )


def wait_pred(pred, seconds=15):
    for _ in range(seconds * 2):
        snap = state()
        if pred(snap):
            return snap
        time.sleep(0.5)
    return state()


def wait_status(target, seconds=15):
    return wait_pred(lambda s: s["adoption"].get("status") == target, seconds)


def main():
    global BASE
    home = tempfile.mkdtemp(prefix="gesso-drill-")
    proc = lines = None
    try:
        proc, lines, url = launch(home)
        check("引导 URL 捕获", bool(url), url or "未捕获（30s 超时）")
        if not url:
            return
        BASE, fragment = url.split("/#")
        code = fragment[len("code="):]

        status, _ = req("POST", "/api/session/exchange", {"code": code})
        check("exchange", status == 200, str(status))

        # ── GPUI start@3 → 候选可见 ─────────────────────────────
        snap = wait_status("candidates_visible")
        check("GPUI start 生效", snap["adoption"]["status"] == "candidates_visible")

        # ── 阶段 A（GPUI 3–10s 静默窗）：Web 确定性 409 → 刷新恢复 ──
        r0 = snap["adoption_revision"]
        s, _ = op({"op": "adoption.select", "index": 0}, r0)
        check("web select 202", s == 202, str(s))
        snap = wait_pred(lambda s_: s_["adoption"].get("selected") == 0)
        check("选中候选 0", snap["adoption"].get("selected") == 0)
        s, _ = op({"op": "adoption.select", "index": 1}, r0)  # r0 已陈旧
        check("陈旧 revision 409", s == 409, str(s))
        snap = state()
        s, _ = op({"op": "adoption.select", "index": 1}, snap["adoption_revision"])
        check("刷新后重试成功", s in (200, 202), str(s))
        snap = wait_pred(lambda s_: s_["adoption"].get("selected") == 1)
        check("改选候选 1", snap["adoption"].get("selected") == 1)

        # ── 阶段 B：GPUI reroll@10 清空选择（交叉修改）──────────
        gen0 = snap["adoption"]["reroll_generation"]
        snap = wait_pred(
            lambda s_: s_["adoption"].get("reroll_generation", 0) > gen0
            and s_["adoption"].get("selected") is None
        )
        check("GPUI reroll 清空选择", snap["adoption"].get("selected") is None)
        # GPUI select:1@12 → Web 立即覆盖为候选 2（争用同一选择实体）
        snap = wait_pred(lambda s_: s_["adoption"].get("selected") == 1)
        check("GPUI 选中候选 1", snap["adoption"].get("selected") == 1)
        names = [c["name"]["zh"] for c in snap["adoption"]["candidates"]]
        s, _ = op({"op": "adoption.select", "index": 2}, snap["adoption_revision"])
        check("web 覆盖选择 202", s in (200, 202), str(s))
        snap = wait_pred(lambda s_: s_["adoption"].get("selected") == 2)
        check("最终选择候选 2", snap["adoption"].get("selected") == 2)

        # ── GPUI confirm@14 → adopted，且宠物即候选 2 ───────────
        snap = wait_status("adopted")
        check(
            "GPUI confirm → adopted",
            snap["adoption"]["status"] == "adopted" and snap["pet"],
        )
        check("宠物即最后确认的候选", snap["pet"]["name"]["zh"] == names[2],
              f"{snap['pet']['name']['zh']} ≠ {names[2]}")
        check("进程存活（前半场）", proc.poll() is None)

        # ── 阶段 C：Web reset.request → GPUI reset_confirm@18 清除 ──
        rev = snap["adoption_revision"]
        s, _ = op({"op": "pet.reset.request"}, rev)
        check("web reset.request 202", s in (200, 202), str(s))
        snap = wait_status("reset_confirm")
        check("待确认态", snap["adoption"]["status"] == "reset_confirm")
        s, b = op({"op": "pet.reset.confirm"}, rev)  # rev 已陈旧
        check("陈旧 confirm 409", s == 409, f"{s} {b.decode()}")
        snap = wait_status("not_adopted", seconds=12)
        check(
            "GPUI 重置 → not_adopted",
            snap["adoption"]["status"] == "not_adopted" and snap["pet"] is None,
        )
        s, b = op({"op": "pet.reset.confirm"}, snap["adoption_revision"])
        check(
            "非待确认态 confirm 拒绝",
            s == 409 and json.loads(b)["error"] == "reset_not_pending",
            f"{s} {b.decode()}",
        )
        check("adoption_revision 单调", not MONO_VIOLATIONS, str(MONO_VIOLATIONS))
        check("进程存活（全程）", proc.poll() is None)

        # ── 收尾：SIGTERM → 端口关闭 + 存档一致 ─────────────────
        proc.terminate()
        proc.wait(timeout=10)
        host, port = BASE.split("//", 1)[1].split(":")
        closed = False
        try:
            socket.create_connection((host, int(port)), timeout=1).close()
        except OSError:
            closed = True
        check("退出后端口关闭", closed)
        save_path = os.path.join(home, ".gesso", "pet-save-v1.json")
        saved = json.load(open(save_path)) if os.path.exists(save_path) else {}
        check("存档无领养残留", not saved.get("adoption"), save_path)

        print(f"\n== {PASS} passed, {FAIL} failed ==")
    finally:
        if proc and proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)
        shutil.rmtree(home, ignore_errors=True)
    sys.exit(1 if FAIL else 0)


main()
