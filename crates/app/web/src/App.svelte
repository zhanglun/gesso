<script>
  import { onMount } from 'svelte';
  import { exchangeBootstrapCode, getState, postOp, previewUrl } from './lib/api.js';

  let active = 'overview';
  let status = 'connecting';
  let error = '';
  /** @type {import('./lib/api.js').PetState | null} */
  let snapshot = null;
  let theme = 'light';
  /** @type {'idle'|'sleep'|'blink'} */
  let previewState = 'idle';
  /** @type {Array<'idle'|'sleep'|'blink'>} */
  const previewStates = ['idle', 'sleep', 'blink'];
  let autoplay = false;
  let blinkOn = false;
  let labBg = 'dark';
  /** @type {Array<[string, string]>} */
  const labBackgrounds = [
    ['dark', '深色'],
    ['light', '浅色'],
    ['checker', '棋盘格'],
  ];
  /** @type {Array<'small'|'medium'|'large'>} */
  const sizeOptions = ['small', 'medium', 'large'];
  /** @type {Array<'quiet'|'normal'|'lively'>} */
  const activityOptions = ['quiet', 'normal', 'lively'];
  /** @type {Array<[string, string]>} 模拟事件：一次性动作，无持久选中态 */
  const noticeOptions = [['new', '新结果'], ['attention', '需要关注'], ['warning', '警告']];

  const nav = [
    ['overview', '概览', 'OVERVIEW'],
    ['adoption', '领养', 'ADOPTION'],
    ['lab', '宠物实验室', 'LAB'],
    ['settings', '设置', 'SETTINGS'],
  ];
  const osdTitle = (id) => nav.find(([nid]) => nid === id)?.[2] ?? '';
  const osdName = (id) => nav.find(([nid]) => nid === id)?.[1] ?? '';
  /** 行为状态机的 API 值 → 面向用户的中文；未知值原样回退 */
  const behaviorLabel = (value) =>
    ({ idle: '待机', sleep: '睡着', walking: '散步', watching: '注视', blink: '眨眼' }[value] ?? value);
  const sizeLabel = { small: '小', medium: '标准', large: '大' };
  const activityLabel = { quiet: '安静', normal: '正常', lively: '活泼' };

  async function refresh() {
    try {
      snapshot = await getState();
      status = 'connected';
      error = '';
    } catch (cause) {
      status = 'error';
      error = cause instanceof Error ? cause.message : '连接失败';
    }
  }

  onMount(() => {
    theme = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    document.documentElement.dataset.theme = theme;
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    // 实验室自动播放：接近真实眨眼节奏（4s 周期 / 短暂闭合）；
    // reduced-motion 下强制静态（仅影响 Web 预览，不影响桌面宠物）
    const blinkTimer = window.setInterval(() => {
      blinkOn = !reducedMotion && Date.now() % 4000 < 300;
    }, 100);
    /** @type {number | undefined} */
    let timer;
    (async () => {
      try {
        await exchangeBootstrapCode();
        await refresh();
      } catch (cause) {
        status = 'error';
        error = cause instanceof Error ? cause.message : '连接失败';
      }
      timer = window.setInterval(refresh, 1500);
    })();
    return () => {
      window.clearInterval(timer);
      window.clearInterval(blinkTimer);
    };
  });

  $: pet = snapshot?.pet ?? null;
  $: adoption = snapshot?.adoption ?? { status: 'not_adopted' };
  $: displayState = autoplay && blinkOn && previewState === 'idle' ? 'blink' : previewState;
  $: autoDisabled = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

  function toggleTheme() {
    theme = theme === 'light' ? 'dark' : 'light';
    document.documentElement.dataset.theme = theme;
  }

  let opBusy = false;
  let opError = '';
  let opFlash = false;

  /** @param {'adoption.start'|'adoption.reroll'|'adoption.select'|'adoption.confirm'|'adoption.back'|'pet.set_visible'|'pet.set_size'|'pet.set_activity'|'pet.notify'|'pet.reset.request'|'pet.reset.confirm'|'pet.reset.cancel'} op */
  async function runOp(op, payload = {}) {
    if (!snapshot || opBusy) return;
    opBusy = true;
    opError = '';
    try {
      await postOp(snapshot.adoption_revision ?? 0, op, payload);
      opFlash = true;
      window.setTimeout(() => (opFlash = false), 650);
      window.setTimeout(refresh, 250);
    } catch (cause) {
      opError = cause instanceof Error && cause.message === 'op_409'
        ? '状态已在其他窗口变化，已刷新，请重试'
        : '操作未完成，请重试';
      refresh();
    } finally {
      opBusy = false;
    }
  }
</script>

<svelte:head>
  <title>Gesso Pet Studio</title>
</svelte:head>

<div class="scene">
  <div class="device">
    <header class="deck">
      <div class="plate">
        <span class="plate__logo" aria-hidden="true">G</span>
        <span class="plate__name"><b>Gesso</b><i>PET STUDIO</i></span>
      </div>
      <span class="led" class:led--off={status !== 'connected'} role="img" aria-label={status === 'connected' ? '已连接' : '连接中'}></span>
      <span class="deck__hint">{status === 'connected' ? '本机已连接' : '连接中…'}</span>
      <div class="deck__grille" aria-hidden="true"><i></i><i></i><i></i><i></i><i></i><i></i><i></i><i></i></div>
    </header>

    <div class="screen">
      <div class="screen__scan" aria-hidden="true"></div>
      <div class="osd">
        <span class="osd__link">
          <i class:off={status !== 'connected'} aria-hidden="true"></i>
          本机 · {status === 'connected' ? '已连接' : '等待连接'}
        </span>
        <span class="osd__page"><b>{osdTitle(active)}</b>· {osdName(active)}</span>
      </div>

      {#if error}
        <div class="notice" role="alert">
          无法读取 Gesso 状态：{error}
          <button class="s-btn" onclick={refresh}>重试</button>
        </div>
      {:else if opError}
        <div class="notice" role="alert">{opError}</div>
      {/if}

      <div class="view">
        {#if active === 'overview'}
          {#if pet}
            <section class="stage" aria-label="桌面宠物预览">
              <div class="stage__halo" aria-hidden="true"></div>
              <div class="stage__floor" aria-hidden="true"></div>
              <img class="pet-img" src={previewUrl('idle')} alt={pet.name.zh} />
              <span class="stage__hint">MAIN DISPLAY · 主屏右下角</span>
            </section>
            <div class="readout" class:readout__pulse={opFlash}>
              <span class="chip">{behaviorLabel(pet.behavior)} · {pet.behavior}</span>
              <span class="readout__note">它正在桌面上安静地生活。</span>
              <span class="screen-note">{pet.visible ? '显示中' : '已隐藏'} · {sizeLabel[pet.size] ?? pet.size} · {activityLabel[pet.activity] ?? pet.activity}</span>
            </div>
          {:else}
            <section class="pg-empty">
              <div class="empty">
                <div class="empty__slot" aria-hidden="true"></div>
                <h2>还没有一位桌面居民</h2>
                <p>选择一个角色，生成三位候选，确认后它才会来到你的桌面。</p>
                <button class="s-btn s-btn--amber" onclick={() => (active = 'adoption')}>开始领养</button>
              </div>
            </section>
          {/if}
        {:else if active === 'adoption'}
          <section class="adopt">
            {#if adoption.candidates?.length}
              <div class="adopt__head">
                <h2>让一位小生命住进桌面</h2>
                <p>候选只存在于当前领养会话中，确认前不会写入存档。</p>
              </div>
              <div class="adopt__grid">
                {#each adoption.candidates as candidate}
                  <article class="cand" class:sel={adoption.selected === candidate.index}>
                    <div class="cand__art">
                      <span class="cand__no">No.0{candidate.index + 1}</span>
                      <img src={previewUrl('idle', candidate.candidate_id)} alt={candidate.name.zh} />
                    </div>
                    <div class="cand__meta"><strong>{candidate.name.zh}</strong><span>{candidate.name.en}</span></div>
                    <button
                      class="s-btn cand__pick"
                      class:s-btn--amber={adoption.selected === candidate.index}
                      disabled={opBusy || adoption.status === 'persisting'}
                      onclick={() => runOp('adoption.select', { index: candidate.index })}
                    >
                      {adoption.selected === candidate.index ? '已选择' : '选择'}
                    </button>
                  </article>
                {/each}
              </div>
            {:else if adoption.status === 'adopted'}
              <section class="pg-empty">
                <div class="empty">
                  <div class="empty__slot" aria-hidden="true"></div>
                  <h2>已经有了一位桌面居民</h2>
                  <p>{pet ? `当前居民：${pet.name.zh}。重置后才能重新领养。` : '领养已完成，请回到概览查看当前宠物。'}</p>
                  <button class="s-btn" onclick={() => (active = 'overview')}>返回概览</button>
                </div>
              </section>
            {:else}
              <section class="pg-empty">
                <div class="empty">
                  <div class="empty__slot" aria-hidden="true"></div>
                  <h2>准备遇见一位新朋友</h2>
                  <p>从首发角色「小蜡烛」开始，会为你生成三位候选，确认前不会写入存档。</p>
                  <button class="s-btn s-btn--amber" disabled={opBusy} onclick={() => runOp('adoption.start')}>开始领养小蜡烛</button>
                </div>
              </section>
            {/if}
          </section>
        {:else if active === 'lab'}
          <section class="lab">
            <div class="lab__canvas" class:bg-light={labBg === 'light'} class:bg-checker={labBg === 'checker'}>
              {#if pet}
                <img src={previewUrl(displayState)} alt={`${displayState} preview`} />
              {:else}
                <span class="screen-note">领养宠物后可预览动作</span>
              {/if}
            </div>
            <div class="lab__panel">
              <h2>宠物实验室</h2>
              <span class="screen-note">预览不会改变桌面宠物的真实状态。</span>
              <div class="ctrl">
                <span class="ctrl__label">动作</span>
                <div class="s-seg">
                  {#each previewStates as state}
                    <button class:on={previewState === state} aria-pressed={previewState === state} onclick={() => (previewState = state)}>{state}</button>
                  {/each}
                </div>
              </div>
              <div class="ctrl">
                <span class="ctrl__label">自动播放</span>
                <div class="s-seg">
                  <button class:on={autoplay} aria-pressed={autoplay} disabled={autoDisabled} onclick={() => (autoplay = !autoplay)}>{autoplay ? '开' : '关'}</button>
                </div>
                {#if autoDisabled}
                  <span class="screen-note">系统已开启减少动态，保持静态预览</span>
                {/if}
              </div>
              <div class="ctrl">
                <span class="ctrl__label">背景</span>
                <div class="s-seg">
                  {#each labBackgrounds as [id, label]}
                    <button class:on={labBg === id} aria-pressed={labBg === id} onclick={() => (labBg = id)}>{label}</button>
                  {/each}
                </div>
              </div>
              <div class="lab__status">
                <span class="chip">{behaviorLabel(pet?.behavior ?? 'idle')} · {pet?.behavior ?? 'idle'}</span>
                <span class="screen-note">MVP preview</span>
              </div>
            </div>
          </section>
        {:else}
          <section class="set">
            <div class="set__row">
              <span>显示宠物</span>
              <div class="s-seg">
                <button class:on={pet?.visible} aria-pressed={pet?.visible} disabled={opBusy || !pet} onclick={() => pet && runOp('pet.set_visible', { visible: true })}>显示中</button>
                <button class:on={pet && !pet.visible} aria-pressed={pet && !pet.visible} disabled={opBusy || !pet} onclick={() => pet && runOp('pet.set_visible', { visible: false })}>已隐藏</button>
              </div>
            </div>
            <div class="set__row">
              <span>尺寸</span>
              <div class="s-seg">
                {#each sizeOptions as option}
                  <button class:on={pet?.size === option} aria-pressed={pet?.size === option} disabled={opBusy || !pet} onclick={() => runOp('pet.set_size', { size: option })}>{sizeLabel[option]}</button>
                {/each}
              </div>
            </div>
            <div class="set__row">
              <span>活跃度</span>
              <div class="s-seg">
                {#each activityOptions as option}
                  <button class:on={pet?.activity === option} aria-pressed={pet?.activity === option} disabled={opBusy || !pet} onclick={() => runOp('pet.set_activity', { activity: option })}>{activityLabel[option]}</button>
                {/each}
              </div>
            </div>
            <div class="set__row">
              <span>模拟事件</span>
              <div class="s-seg">
                {#each noticeOptions as [kind, label]}
                  <button disabled={opBusy || !pet} onclick={() => runOp('pet.notify', { kind })}>{label}</button>
                {/each}
              </div>
              <span class="screen-note">发送后真实宠物按优先级规则显示气泡，正文不持久化。</span>
            </div>
            <div class="set__row">
              <span>连接状态</span>
              <span class="chip">{status === 'connected' ? '正常' : '等待连接'}</span>
            </div>
            {#if adoption.status === 'reset_confirm'}
              <div class="set__danger" role="alert">
                <strong>重置待确认</strong>
                <p>宠物数据将被清除且不可恢复：个体、外观与互动统计全部清零。</p>
                <div class="footbar__actions" style="margin-left:0;">
                  <button class="s-btn s-btn--danger" disabled={opBusy} onclick={() => runOp('pet.reset.confirm')}>确认重置（不可恢复）</button>
                  <button class="s-btn" disabled={opBusy} onclick={() => runOp('pet.reset.cancel')}>取消</button>
                </div>
              </div>
            {:else if pet}
              <div class="set__danger">
                <strong>高级操作</strong>
                <p>清除当前宠物个体并回到未领养状态，需要两步确认。</p>
                <button class="s-btn s-btn--danger" disabled={opBusy} onclick={() => runOp('pet.reset.request')}>重置宠物数据…</button>
              </div>
            {/if}
          </section>
        {/if}
      </div>
    </div>

    <nav class="keys" aria-label="主导航">
      {#each nav as [id, label]}
        <button class="key" aria-current={active === id ? 'page' : undefined} onclick={() => (active = id)}>
          <span class="dot" aria-hidden="true"></span>{label}
        </button>
      {/each}
      <button class="key key--icon" onclick={refresh} aria-label="刷新状态" title="刷新状态">
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" /><path d="M13.5 1.8v2.7h-2.7" /></svg>
      </button>
      <button class="key key--icon" onclick={toggleTheme} aria-label="切换主题" title="切换主题">
        <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true"><circle cx="8" cy="8" r="5.6" /><path d="M8 2.4a5.6 5.6 0 0 1 0 11.2z" fill="currentColor" stroke="none" opacity=".85" /></svg>
      </button>
    </nav>

    <footer class="footbar">
      {#if active === 'overview' && pet}
        <div class="footbar__stats">
          <span>陪伴时间 <b>{Math.floor(pet.stats.companion_seconds / 3600)}h</b></span>
          <span>被摸 <b>{pet.stats.touch_count}</b></span>
          <span>被戳 <b>{pet.stats.poke_count}</b></span>
        </div>
        <div class="footbar__actions">
          <button class="fkey" disabled={opBusy} onclick={() => runOp('pet.set_visible', { visible: !pet.visible })}>
            {pet.visible ? '显示中 · 点击隐藏' : '已隐藏 · 点击显示'}
          </button>
          <button class="fkey fkey--amber" onclick={() => (active = 'lab')}>预览动作</button>
        </div>
      {:else if active === 'adoption' && adoption.candidates?.length}
        <div class="footbar__note">
          {adoption.status === 'persisting' ? '正在保存领养资料…' : adoption.status === 'candidate_selected' ? '已选择一位候选' : '选择你喜欢的外观'}
        </div>
        <div class="footbar__actions">
          <button class="fkey" disabled={opBusy || adoption.status === 'persisting'} onclick={() => runOp('adoption.back')}>返回角色选择</button>
          <button class="fkey" disabled={opBusy || adoption.status === 'persisting'} onclick={() => runOp('adoption.reroll')}>重新抽取</button>
          <button class="fkey fkey--amber" disabled={opBusy || adoption.selected == null || adoption.status === 'persisting'} onclick={() => runOp('adoption.confirm')}>确认领养</button>
        </div>
      {:else if active === 'settings'}
        <div class="footbar__note">档位即改即存，与桌面窗口同一真源</div>
      {/if}
    </footer>
  </div>
</div>
