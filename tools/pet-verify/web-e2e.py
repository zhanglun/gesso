#!/usr/bin/env python3
"""Gesso 桌面宠物 Web 控制台 E2E 回归（HTTP 驱动，跨平台）。

用法：
  1) 沙箱模式（推荐，不碰真实存档）：
       HOME=<临时目录> GESSO_LOCK=e2e GESSO_PET_WEB=1 \
       GESSO_PET_WEB_ECHO=1 GESSO_PET_WEB_NO_OPEN=1 ./target/debug/gesso
  2) python3 tools/pet-verify/web-e2e.py <bootstrap-url>
     （URL 从应用日志 [pet-web] bootstrap url: 行取）

覆盖：exchange / 领养五动作 / 冲突恢复（陈旧 revision → 409 → 刷新重试）/
设置档位 / 显隐 / 两阶段重置 / 候选预览 PNG / 安全负路径。
只断言 HTTP 契约与公开快照；不点 UI、不截图（UI 动线见技术方案 §9.2）。
"""

import json
import sys
import time
import urllib.error
import urllib.request
import uuid
from http.cookiejar import CookieJar

BASE, FRAGMENT = sys.argv[1].strip().split("/#")
CODE = FRAGMENT[len("code=") :]
PASS = 0
FAIL = 0


def check(name, cond, detail=""):
    global PASS, FAIL
    PASS += bool(cond)
    FAIL += not cond
    print(("PASS" if cond else "FAIL"), name, detail)


def req(method, path, body=None, session=None, origin=True, retries=2):
    # 注意：CookieJar 有 __len__，空 jar 是 falsy——必须用 is None 判断
    jar = JAR if session is None else session
    r = urllib.request.Request(
        BASE + path,
        method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={
            "Content-Type": "application/json",
            **({"Origin": BASE} if origin else {}),
        },
    )
    # loopback 流量不走系统代理（代理环境变量拦截 127.0.0.1 → 间歇 502）
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
        except Exception as e:  # 启动时序偶发：指数退避重试
            last = (-attempt - 1, f"{type(e).__name__}".encode())
            time.sleep(0.4 * (attempt + 1))
    return last


JAR = CookieJar()


def state():
    status, body = req("GET", "/api/state")
    return json.loads(body)


def op(payload, revision, jar=None):
    return req(
        "POST",
        "/api/adoption/op",
        {"request_id": str(uuid.uuid4()), "revision": revision, **payload},
    )


def wait_pred(pred, seconds=15):
    """轮询快照直到 pred(snap) 为真；状态已匹配时也要等动作真正落账。"""
    for _ in range(seconds * 2):
        snap = state()
        if pred(snap):
            return snap
        time.sleep(0.5)
    return state()


def wait_status(target, seconds=15):
    return wait_pred(lambda s: s["adoption"]["status"] == target, seconds)


def main():
    # ── 认证 ────────────────────────────────────────────────
    status, _ = req("POST", "/api/session/exchange", {"code": CODE})
    check("exchange", status == 200, str(status))
    status, _ = req("GET", "/api/state", session=CookieJar())
    check("无 cookie 401", status == 401, str(status))

    # ── 领养到确认 ──────────────────────────────────────────
    snap = state()
    check("初始未领养", snap["adoption"]["status"] == "not_adopted")
    if snap["adoption"]["status"] != "not_adopted":
        print("（实例已有宠物，跳过领养段）")
        return

    op({"op": "adoption.start"}, snap["adoption_revision"])
    snap = wait_status("candidates_visible")
    check("候选可见", snap["adoption"]["status"] == "candidates_visible")
    check("三个候选", len(snap["adoption"]["candidates"]) == 3)

    # 候选预览 PNG（candidate_id 公开查找）
    cid = snap["adoption"]["candidates"][0]["candidate_id"]
    status, body = req("GET", f"/api/pet/preview?state=idle&candidate={cid}")
    check("候选预览 PNG", status == 200 and body[:4] == b"\x89PNG", str(status))

    # ── 冲突恢复演练：陈旧 revision → 409 → 刷新重试 ────────
    stale = snap["adoption_revision"]
    gen_before = snap["adoption"]["reroll_generation"]
    op({"op": "adoption.reroll"}, snap["adoption_revision"])  # 另一 actor 改状态
    snap = wait_pred(lambda s: s["adoption"]["reroll_generation"] == gen_before + 1)
    check("reroll 换代", snap["adoption"]["reroll_generation"] == gen_before + 1)
    status, _ = op({"op": "adoption.select", "index": 1}, stale)
    check("陈旧 revision 409", status == 409, str(status))

    # 刷新后用新 revision 完成领养
    snap = state()
    op({"op": "adoption.select", "index": 2}, snap["adoption_revision"])
    time.sleep(0.8)
    snap = state()
    check("选中候选 2", snap["adoption"]["selected"] == 2)
    op({"op": "adoption.confirm"}, snap["adoption_revision"])
    snap = wait_status("adopted")
    check("领养完成", snap["adoption"]["status"] == "adopted" and snap["pet"])
    check("快照无 seed/路径", "seed" not in json.dumps(snap) and "/tmp/" not in json.dumps(snap))

    # 同 request_id 重放幂等（用 reroll 验证非幂等操作的去重）
    rid = str(uuid.uuid4())
    rev = snap["adoption_revision"]
    # adopted 态 reroll 是 no-op，但去重语义与操作结果无关：两次同 id 应同一 operation_id
    s1, b1 = req("POST", "/api/adoption/op", {"request_id": rid, "revision": rev, "op": "pet.set_visible", "visible": False})
    op_id = json.loads(b1).get("operation_id")
    s2, b2 = req("POST", "/api/adoption/op", {"request_id": rid, "revision": rev, "op": "pet.set_visible", "visible": False})
    check(
        "request_id 重放幂等",
        s1 == 202 and s2 == 200 and json.loads(b2).get("operation_id") == op_id,
        f"{s1}/{s2}",
    )
    time.sleep(0.8)
    snap = state()
    check("显隐已切换", snap["pet"]["visible"] is False)

    # ── 设置档位 ────────────────────────────────────────────
    op({"op": "pet.set_size", "size": "large"}, snap["adoption_revision"])
    time.sleep(0.8)
    snap = state()
    check("尺寸 large", snap["pet"]["size"] == "large")
    status, _ = op({"op": "pet.set_size", "size": "huge"}, snap["adoption_revision"])
    check("非法档位 400", status == 400, str(status))

    # ── 模拟事件（一次性动作；快照无可见变化，断言 202→operation applied）──
    s1, b1 = op({"op": "pet.notify", "kind": "attention"}, snap["adoption_revision"])
    check("模拟事件 202", s1 == 202, str(s1))
    notify_op_id = json.loads(b1).get("operation_id")
    applied = False
    for _ in range(10):
        _, bb = req("GET", f"/api/operations/{notify_op_id}")
        if json.loads(bb).get("status") == "applied":
            applied = True
            break
        time.sleep(0.3)
    check("模拟事件已执行", applied)
    status, _ = op({"op": "pet.notify", "kind": "bogus"}, snap["adoption_revision"])
    check("非法 kind 400", status == 400, str(status))

    # ── 两阶段重置 ──────────────────────────────────────────
    status, _ = op({"op": "pet.reset.confirm"}, snap["adoption_revision"])
    check("未请求就 confirm 409", status == 409, str(status))
    op({"op": "pet.reset.request"}, snap["adoption_revision"])
    snap = wait_status("reset_confirm")
    check("待确认态", snap["adoption"]["status"] == "reset_confirm")
    check("宠物仍在（未破坏）", snap["pet"] is not None)
    op({"op": "pet.reset.cancel"}, snap["adoption_revision"])
    snap = wait_status("adopted")
    check("取消回退", snap["adoption"]["status"] == "adopted")
    op({"op": "pet.reset.request"}, snap["adoption_revision"])
    snap = wait_status("reset_confirm")
    op({"op": "pet.reset.confirm"}, snap["adoption_revision"])
    snap = wait_status("not_adopted")
    check("重置完成", snap["adoption"]["status"] == "not_adopted" and snap["pet"] is None)

    print(f"\n== {PASS} passed, {FAIL} failed ==")
    sys.exit(1 if FAIL else 0)


main()
