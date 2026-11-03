// Gesso Pet Studio — 前端逻辑（Preact + HTM，免构建静态文件）
//
// 与 Rust 引擎的契约（见 docs/design/桌面宠物 Web 控制台技术方案.md §6）：
// 1.5s 轮询 /api/state 读快照；写操作 POST /api/adoption/op，
// 携带 adoption_revision 乐观锁与 request_id 去重，返回 operation_id，
// 实际执行在 Gesso 主循环，结果经下一轮快照反映。
// 预览帧来自 /api/pet/preview（服务端生成的受控 PNG）。
// bootstrap code 换发：托盘打开时 URL 带 #code=，换会话 cookie 后清除。

import { h, render } from 'preact';
import { useState, useEffect, useRef } from 'preact/hooks';
import htm from 'htm';

const html = htm.bind(h);

// ---------- API 层 ----------

/**
 * @returns {Promise<{state_revision:number, adoption_revision:number,
 *   adoption:{status:string, species?:string, reroll_generation?:number, selected?:number|null,
 *     candidates?:Array<{index:number, candidate_id:string, name:{zh:string,en:string},
 *       appearance:{body:number, coat_color:number, marking:number, accent:number}}>},
 *   pet: null|{name:{zh:string,en:string}, species:string, behavior:string, visible:boolean,
 *     activity:string, size:string, stats:{companion_seconds:number, touch_count:number, poke_count:number}}}>}
 */
async function getState() {
  const response = await fetch('/api/state', { credentials: 'include' });
  if (!response.ok) throw new Error(`state_${response.status}`);
  return response.json();
}

/**
 * 提交领养写操作（异步）：服务器校验 adoption_revision 乐观锁并去重 request_id，
 * 返回 operation_id；实际执行在 Gesso 主循环，结果经 /api/state 轮询反映。
 */
async function postOp(adoptionRevision, op, payload = {}) {
  const response = await fetch('/api/adoption/op', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({
      request_id: crypto.randomUUID(),
      revision: adoptionRevision,
      op,
      ...payload,
    }),
  });
  if (!response.ok) throw new Error(`op_${response.status}`);
  return response.json();
}

/** @param {'idle'|'sleep'|'blink'} state @param {string} [candidateId] 领养候选取 id */
function previewUrl(state, candidateId) {
  const suffix = candidateId ? `&candidate=${encodeURIComponent(candidateId)}` : '';
  return `/api/pet/preview?state=${encodeURIComponent(state)}${suffix}`;
}

/** 托盘打开时 URL 带 #code=，换会话 cookie 后清除 */
async function exchangeBootstrapCode() {
  const hash = window.location.hash;
  if (!hash.startsWith('#code=')) return false;
  const code = decodeURIComponent(hash.slice('#code='.length));
  const response = await fetch('/api/session/exchange', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    credentials: 'include',
    body: JSON.stringify({ code }),
  });
  if (!response.ok) throw new Error(`exchange_${response.status}`);
  window.history.replaceState(null, '', window.location.pathname + window.location.search);
  return true;
}

// ---------- 常量与词典 ----------

const NAV = [
  ['overview', '概览', 'OVERVIEW'],
  ['adoption', '领养', 'ADOPTION'],
  ['lab', '宠物实验室', 'LAB'],
  ['settings', '设置', 'SETTINGS'],
];
const PREVIEW_STATES = [['idle', '待机'], ['sleep', '睡着'], ['blink', '眨眼']];
const LAB_BACKGROUNDS = [['dark', '深色'], ['light', '浅色'], ['checker', '棋盘格']];
const SIZE_OPTIONS = ['small', 'medium', 'large'];
const ACTIVITY_OPTIONS = ['quiet', 'normal', 'lively'];
/** 模拟事件：一次性动作，无持久选中态 */
const NOTICE_OPTIONS = [['new', '新结果'], ['attention', '需要关注'], ['warning', '警告']];

const SIZE_LABEL = { small: '小', medium: '标准', large: '大' };
const ACTIVITY_LABEL = { quiet: '安静', normal: '正常', lively: '活泼' };
/** 行为状态机的 API 值 → 面向用户的中文；未知值原样回退 */
const BEHAVIOR_LABEL = { idle: '待机', sleep: '睡着', walking: '散步', watching: '注视', blink: '眨眼' };
const behaviorLabel = (value) => BEHAVIOR_LABEL[value] ?? value;
const osdTitle = (id) => NAV.find(([nid]) => nid === id)?.[2] ?? '';
const osdName = (id) => NAV.find(([nid]) => nid === id)?.[1] ?? '';
/** 陪伴时长 → 「Xh Ym / Ym」；新宠物不显示冷冰冰的 0h */
const formatCompanion = (seconds) => {
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  return h > 0 ? `${h}h ${m}m` : `${m}m`;
};
/** 连接/请求错误 → 用户语言；不向界面泄漏内部状态码 */
const describeError = (cause) => {
  if (cause instanceof TypeError) return 'Gesso 未运行或已退出';
  if (cause instanceof Error) {
    if (cause.message === 'state_404') return 'Gesso 服务不可用，请确认 Gesso 正在运行';
    if (cause.message.startsWith('state_') || cause.message.startsWith('exchange_')) return '连接本机 Gesso 失败';
    return cause.message;
  }
  return '连接失败';
};

// ---------- 应用 ----------

function App() {
  const [active, setActive] = useState('overview');
  const [status, setStatus] = useState('connecting');
  const [error, setError] = useState('');
  const [snapshot, setSnapshot] = useState(null);
  const [theme, setTheme] = useState(() =>
    window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light');
  const [previewState, setPreviewState] = useState('idle');
  const [autoplay, setAutoplay] = useState(false);
  const [blinkOn, setBlinkOn] = useState(false);
  const [labBg, setLabBg] = useState('dark');
  const [opBusy, setOpBusy] = useState(false);
  const [opError, setOpError] = useState('');
  const [opFlash, setOpFlash] = useState(false);
  /** 领养成功的一次性欢迎（规格 §3.2）：确认当轮展示，刷新页面不复播 */
  const [welcome, setWelcome] = useState(null);
  const seenSnapshotRef = useRef(false);
  const adoptedRef = useRef(false);
  /** 模拟事件的 Web 端回执：瞬时触发也要给「已发送」反馈 */
  const [notifyHint, setNotifyHint] = useState('');
  /**
   * 乐观档位：点击立即呈现目标态，快照确认后清除，1.5s 超时回退
   * （否则选中态跟快照走，点完要等 POST→引擎→轮询回来才变色 = 卡顿感）
   */
  const [pending, setPending] = useState(null);
  const pendingTimer = useRef();
  const optimistic = (kind, value) => {
    setPending({ kind, value });
    window.clearTimeout(pendingTimer.current);
    pendingTimer.current = window.setTimeout(() => setPending(null), 1500);
  };
  // 浏览器减少动态：只影响 Web 预览（自动播放锁），不是宠物存档设置
  const autoDisabled = useRef(window.matchMedia('(prefers-reduced-motion: reduce)').matches).current;

  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);

  // 实验室自动播放：接近真实眨眼节奏（4s 周期 / 短暂闭合）；
  // reduced-motion 下强制静态（仅影响 Web 预览，不影响桌面宠物）
  useEffect(() => {
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    const blinkTimer = window.setInterval(() => {
      setBlinkOn(!reducedMotion && Date.now() % 4000 < 300);
    }, 100);
    return () => window.clearInterval(blinkTimer);
  }, []);

  async function refresh() {
    try {
      const next = await getState();
      setSnapshot(next);
      setStatus('connected');
      setError('');
    } catch (cause) {
      setStatus('error');
      setError(describeError(cause));
    }
  }

  useEffect(() => {
    /** @type {number | undefined} */
    let timer;
    (async () => {
      try {
        await exchangeBootstrapCode();
        await refresh();
      } catch (cause) {
        // exchange 失败不阻断轮询：状态展示交给 refresh 的错误横幅
        await refresh();
      }
      timer = window.setInterval(refresh, 1500);
    })();
    return () => window.clearInterval(timer);
  }, []);

  function toggleTheme() {
    setTheme((t) => (t === 'light' ? 'dark' : 'light'));
  }

  async function runOp(op, payload = {}) {
    if (!snapshot || opBusy) return;
    setOpBusy(true);
    setOpError('');
    try {
      await postOp(snapshot.adoption_revision ?? 0, op, payload);
      setOpFlash(true);
      window.setTimeout(() => setOpFlash(false), 650);
      window.setTimeout(refresh, 250);
    } catch (cause) {
      setOpError(cause instanceof Error && cause.message === 'op_409'
        ? '状态已在其他窗口变化，已刷新，请重试'
        : '操作未完成，请重试');
      refresh();
    } finally {
      setOpBusy(false);
    }
  }

  const pet = snapshot?.pet ?? null;
  const adoption = snapshot?.adoption ?? { status: 'not_adopted' };
  const displayState = autoplay && blinkOn && previewState === 'idle' ? 'blink' : previewState;
  const connected = status === 'connected';
  // 断连/未就绪时禁用全部写 CTA（runOp 内还有 snapshot 守卫，这里让按钮先说明问题）
  const engineReady = !!snapshot && connected;
  // 设置页乐观呈现值：pending 优先，快照兜底
  const shownVisible = pending?.kind === 'visible' ? pending.value : !!pet?.visible;
  const shownSize = pending?.kind === 'size' ? pending.value : pet?.size;
  const shownActivity = pending?.kind === 'activity' ? pending.value : pet?.activity;
  // 头部连接语义：三态（已连接 / 未运行 / 连接中），不再永远谎报「连接中」
  const headNote = connected ? '本机已连接' : status === 'error' ? 'Gesso 未运行' : '连接中…';
  const headLedLabel = connected ? '已连接' : status === 'error' ? '未运行' : '连接中';

  // 领养成功检测：上一轮未领养 → 本轮 adopted，注入一次性欢迎并回到概览
  useEffect(() => {
    if (!snapshot) return;
    const adopted = snapshot.adoption?.status === 'adopted' || !!snapshot.pet;
    if (seenSnapshotRef.current && adopted && !adoptedRef.current && snapshot.pet) {
      setWelcome(snapshot.pet.name.zh);
      setActive('overview');
    }
    seenSnapshotRef.current = true;
    adoptedRef.current = adopted;
  }, [snapshot]);

  useEffect(() => {
    if (!welcome) return;
    const timer = window.setTimeout(() => setWelcome(null), 12000);
    return () => window.clearTimeout(timer);
  }, [welcome]);

  useEffect(() => {
    if (!notifyHint) return;
    const timer = window.setTimeout(() => setNotifyHint(''), 2500);
    return () => window.clearTimeout(timer);
  }, [notifyHint]);

  // 快照确认乐观值：引擎已应用即清除，回到真源驱动
  useEffect(() => {
    if (!pending || !pet) return;
    const current = pending.kind === 'visible' ? pet.visible : pet[pending.kind];
    if (current === pending.value) setPending(null);
  }, [snapshot]);

  // ---------- 各页视图 ----------

  const overviewView = pet
    ? html`
      <section class="dex-screen dex-screen--stage" aria-label="桌面宠物预览">
        <div class="lcd-scan" aria-hidden="true"></div>
        <div class="dex-osd">
          <span>${osdTitle('overview')} <span class="zh">· ${osdName('overview')}</span></span>
          <span>OBSERVE · 观测中</span>
        </div>
        <div class="stage">
          <span class="tick" aria-hidden="true"></span>
          <div class="stage-halo" aria-hidden="true"></div>
          <div class="stage-floor" aria-hidden="true"></div>
          <img class="pet-img" src=${previewUrl('idle')} alt=${pet.name.zh} />
          <span class="stage-hint">MAIN DISPLAY · 主屏右下角</span>
        </div>
        <div class=${'readout' + (opFlash ? ' readout--flash' : '')}>
          <span class="ptag">${behaviorLabel(pet.behavior)} · ${pet.behavior}</span>
          <span class="lcd-note">它正在桌面上安静地生活。</span>
          <span class="lcd-faint">${pet.visible ? '显示中' : '已隐藏'} · ${SIZE_LABEL[pet.size] ?? pet.size} · ${ACTIVITY_LABEL[pet.activity] ?? pet.activity}</span>
        </div>
      </section>`
    : html`
      <section class="page pg-empty">
        <div class="empty">
          <div class="slot" aria-hidden="true"></div>
          <h2>还没有一位桌面居民</h2>
          <p>选择一个角色，生成三位候选，确认后它才会来到你的桌面。</p>
          <button class="pbtn pbtn--accent" disabled=${!engineReady} onclick=${() => setActive('adoption')}>开始领养</button>
        </div>
      </section>`;

  const adoptionView = adoption.candidates?.length
    ? html`
      <section class="page">
        <h2 class="page-title">让一位小生命住进桌面</h2>
        <p class="page-sub">候选只存在于当前领养会话中，确认前不会写入存档。</p>
        <div class="adopt-grid">
          ${adoption.candidates.map((candidate) => html`
            <article key=${candidate.candidate_id} class=${'card' + (adoption.selected === candidate.index ? ' sel' : '')}>
              <div class="card-screen">
                <span class="card-no">No.0${candidate.index + 1}</span>
                <img src=${previewUrl('idle', candidate.candidate_id)} alt=${candidate.name.zh} />
              </div>
              <div class="card-body">
                <div class="card-meta"><strong>${candidate.name.zh}</strong><span>${candidate.name.en}</span></div>
                <button
                  class=${'pbtn card-pick' + (adoption.selected === candidate.index ? ' pbtn--accent' : '')}
                  disabled=${opBusy || adoption.status === 'persisting'}
                  onclick=${() => runOp('adoption.select', { index: candidate.index })}
                >
                  ${adoption.selected === candidate.index ? '已选择' : '选择'}
                </button>
              </div>
            </article>`)}
        </div>
      </section>`
    : adoption.status === 'adopted'
      ? html`
        <section class="page pg-empty">
          <div class="empty">
            <div class="slot" aria-hidden="true"></div>
            <h2>已经有了一位桌面居民</h2>
            <p>${pet ? `当前居民：${pet.name.zh}。重置后才能重新领养。` : '领养已完成，请回到概览查看当前宠物。'}</p>
            <button class="pbtn" onclick=${() => setActive('overview')}>返回概览</button>
          </div>
        </section>`
      : html`
        <section class="page pg-empty">
          <div class="empty">
            <div class="slot" aria-hidden="true"></div>
            <h2>准备遇见一位新朋友</h2>
            <p>从首发角色「小蜡烛」开始，会为你生成三位候选，确认前不会写入存档。</p>
            <button class="pbtn pbtn--accent" disabled=${opBusy || !engineReady} onclick=${() => runOp('adoption.start')}>开始领养小蜡烛</button>
          </div>
        </section>`;

  const labView = html`
    <section class="page">
      <div class="lab">
        <div class=${'dex-screen lab-screen' + (labBg === 'light' ? ' lab-screen--light' : '') + (labBg === 'checker' ? ' lab-screen--checker' : '')}>
          <div class="lcd-scan" aria-hidden="true"></div>
          ${pet
            ? html`<img src=${previewUrl(displayState)} alt=${`${behaviorLabel(displayState)}动作预览`} />`
            : html`<span class="lcd-note">领养宠物后可预览动作</span>`}
        </div>
        <div class="lab-panel">
          <h2>宠物实验室</h2>
          <span class="lcd-note">预览不会改变桌面宠物的真实状态。</span>
            <div class="ctrl">
              <span class="ctrl-label">动作</span>
              <div class="pseg">
                ${PREVIEW_STATES.map(([value, label]) => html`
                <button key=${value} class=${previewState === value ? 'on' : ''} aria-pressed=${String(previewState === value)} onclick=${() => setPreviewState(value)}>${label}</button>`)}
              </div>
            </div>
          <div class="ctrl">
            <span class="ctrl-label">自动播放</span>
            <div class="pseg">
              <button class=${autoplay ? 'on' : ''} aria-pressed=${String(autoplay)} disabled=${autoDisabled} onclick=${() => setAutoplay((v) => !v)}>${autoplay ? '开' : '关'}</button>
            </div>
            ${autoDisabled && html`<span class="pnote">系统已开启减少动态，保持静态预览</span>`}
          </div>
          <div class="ctrl">
            <span class="ctrl-label">背景</span>
            <div class="pseg">
              ${LAB_BACKGROUNDS.map(([id, label]) => html`
                <button key=${id} class=${labBg === id ? 'on' : ''} aria-pressed=${String(labBg === id)} onclick=${() => setLabBg(id)}>${label}</button>`)}
            </div>
          </div>
          <div class="lab-status">
            <span class="ptag">${behaviorLabel(pet?.behavior ?? 'idle')} · ${pet?.behavior ?? 'idle'}</span>
            <span class="pnote">MVP 预览 · 三态受控</span>
          </div>
        </div>
      </div>
    </section>`;

  const settingsView = html`
    <section class="page">
      <div class="set">
        <div class="set-row">
          <span>显示宠物</span>
          <div class="pseg">
            <button class=${shownVisible ? 'on' : ''} aria-pressed=${String(shownVisible)} disabled=${!engineReady} onclick=${() => { optimistic('visible', true); pet && runOp('pet.set_visible', { visible: true }); }}>显示中</button>
            <button class=${!shownVisible ? 'on' : ''} aria-pressed=${String(!shownVisible)} disabled=${!engineReady} onclick=${() => { optimistic('visible', false); pet && runOp('pet.set_visible', { visible: false }); }}>已隐藏</button>
          </div>
        </div>
        <div class="set-row">
          <span>尺寸</span>
          <div class="pseg">
            ${SIZE_OPTIONS.map((option) => html`
              <button key=${option} class=${shownSize === option ? 'on' : ''} aria-pressed=${String(shownSize === option)} disabled=${!engineReady} onclick=${() => { optimistic('size', option); runOp('pet.set_size', { size: option }); }}>${SIZE_LABEL[option]}</button>`)}
          </div>
        </div>
        <div class="set-row">
          <span>活跃度</span>
          <div class="pseg">
            ${ACTIVITY_OPTIONS.map((option) => html`
              <button key=${option} class=${shownActivity === option ? 'on' : ''} aria-pressed=${String(shownActivity === option)} disabled=${!engineReady} onclick=${() => { optimistic('activity', option); runOp('pet.set_activity', { activity: option }); }}>${ACTIVITY_LABEL[option]}</button>`)}
          </div>
        </div>
        <div class="set-row">
          <span>模拟事件</span>
          <div class="pseg">
            ${NOTICE_OPTIONS.map(([kind, label]) => html`
              <button key=${kind} disabled=${!engineReady} onclick=${() => { setNotifyHint(label); runOp('pet.notify', { kind }); }}>${label}</button>`)}
          </div>
          ${notifyHint && html`<span class="ptag">已发送 · ${notifyHint} — 看桌面宠物</span>`}
          <span class="pnote">气泡出现在桌面宠物身上（主屏右下角），按优先级规则显示，正文不持久化。</span>
        </div>
        <div class="set-row">
          <span>连接状态</span>
          <span class="ptag">${connected ? '正常' : '等待连接'}</span>
        </div>
        ${adoption.status === 'reset_confirm'
          ? html`
            <div class="set-danger" role="alert">
              <strong>重置待确认</strong>
              <p>宠物数据将被清除且不可恢复：个体、外观与互动统计全部清零。</p>
              <div style="display:flex;gap:10px;flex-wrap:wrap;">
                <button class="pbtn pbtn--danger" onclick=${() => runOp('pet.reset.confirm')}>确认重置（不可恢复）</button>
                <button class="pbtn" onclick=${() => runOp('pet.reset.cancel')}>取消</button>
              </div>
            </div>`
          : pet
            ? html`
              <div class="set-danger">
                <strong>高级操作</strong>
                <p>清除当前宠物个体并回到未领养状态，需要两步确认。</p>
                <button class="pbtn pbtn--danger" onclick=${() => runOp('pet.reset.request')}>重置宠物数据…</button>
              </div>`
            : null}
      </div>
    </section>`;

  // ---------- 底部条 ----------

  const foot = active === 'overview' && pet
    ? html`
      <div class="plaques">
        <span class="plaque"><span>陪伴时间</span><b>${formatCompanion(pet.stats.companion_seconds)}</b></span>
        <span class="plaque"><span>被摸</span><b>${pet.stats.touch_count}</b></span>
        <span class="plaque"><span>被戳</span><b>${pet.stats.poke_count}</b></span>
      </div>
      <div class="foot-actions">
        <button class="pbtn" onclick=${() => runOp('pet.set_visible', { visible: !pet.visible })}>
          ${pet.visible ? '显示中 · 点击隐藏' : '已隐藏 · 点击显示'}
        </button>
        <button class="pbtn pbtn--accent" onclick=${() => setActive('lab')}>预览动作</button>
      </div>`
    : active === 'adoption' && adoption.candidates?.length
      ? html`
        <span class="foot-note">
          ${adoption.status === 'persisting' ? '正在保存领养资料…' : adoption.status === 'candidate_selected' ? '已选择一位候选' : '选择你喜欢的外观'}
        </span>
        <div class="foot-actions">
          <button class="pbtn" disabled=${opBusy || adoption.status === 'persisting'} onclick=${() => runOp('adoption.back')}>返回角色选择</button>
          <button class="pbtn" disabled=${opBusy || adoption.status === 'persisting'} onclick=${() => runOp('adoption.reroll')}>重新抽取</button>
          <button class="pbtn pbtn--accent" disabled=${opBusy || adoption.selected == null || adoption.status === 'persisting'} onclick=${() => runOp('adoption.confirm')}>确认领养</button>
        </div>`
      : active === 'settings'
        ? html`<span class="foot-note">档位即改即存，与桌面窗口同一真源</span>`
        : null;

  // ---------- 整页 ----------

  return html`
    <div class="scene">
      <div class="folio">
        <header class="folio-head">
          <span class="stamp" aria-hidden="true">G</span>
          <span class="brand"><b>Gesso</b><i>PET STUDIO · 桌面生物图鉴</i></span>
          <span class=${connected ? 'led' : 'led led--off'} role="img" aria-label=${headLedLabel}></span>
          <span class="head-note">${headNote}</span>
          <span class="head-tools">
            <button class="tool" onclick=${refresh} aria-label="刷新状态" title="刷新状态">
              <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><path d="M13 8a5 5 0 1 1-1.5-3.6" /><path d="M13 1.5v3h-3" /></svg>
            </button>
            <button class="tool" onclick=${toggleTheme} aria-label="切换主题" title="切换主题">
              <svg viewBox="0 0 16 16" aria-hidden="true"><rect x="2" y="2" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2" /><rect x="8" y="2" width="6" height="12" fill="currentColor" /></svg>
            </button>
          </span>
        </header>

        <nav class="dex-tabs" aria-label="主导航">
          ${NAV.map(([id, label]) => html`
            <button key=${id} class="dex-tab" aria-current=${active === id ? 'page' : undefined} onclick=${() => setActive(id)}>${label}</button>`)}
        </nav>

        ${error
          ? html`
            <div class="notice--paper" role="alert">
              ${error}
              <button class="pbtn" onclick=${refresh}>重试</button>
            </div>`
          : opError
            ? html`<div class="notice--paper" role="alert">${opError}</div>`
            : welcome
              ? html`
                <div class="welcome" role="status">
                  <span class="welcome-text"><b>欢迎来到桌面，${welcome}。</b><i>它已经出现在主屏右下角，正在适应新家。</i></span>
                  <span class="foot-actions">
                    <button class="pbtn" onclick=${() => setWelcome(null)}>关闭</button>
                    <button class="pbtn pbtn--accent" onclick=${() => { setWelcome(null); setActive('settings'); }}>前往设置</button>
                  </span>
                </div>`
              : null}

        ${active === 'overview' ? overviewView : active === 'adoption' ? adoptionView : active === 'lab' ? labView : settingsView}

        <footer class="folio-foot">${foot}</footer>
      </div>
    </div>`;
}

const target = document.getElementById('app');
if (!target) throw new Error('missing_app_mount');
render(html`<${App} />`, target);
