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

  const nav = [
    ['overview', '概览'],
    ['adoption', '领养'],
    ['lab', '宠物实验室'],
    ['settings', '设置'],
  ];

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
    return () => window.clearInterval(timer);
  });

  $: pet = snapshot?.pet ?? null;
  $: adoption = snapshot?.adoption ?? { status: 'not_adopted' };

  function toggleTheme() {
    theme = theme === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = theme;
  }

  let opBusy = false;
  let opError = '';

  /** @param {'adoption.start'|'adoption.reroll'|'adoption.select'|'adoption.confirm'|'adoption.back'|'pet.set_visible'} op */
  async function runOp(op, payload = {}) {
    if (!snapshot || opBusy) return;
    opBusy = true;
    opError = '';
    try {
      await postOp(snapshot.adoption_revision ?? 0, op, payload);
      window.setTimeout(refresh, 250);
    } catch (cause) {
      opError = cause instanceof Error && cause.message === 'op_409'
        ? '领养状态已在其他窗口变化，请重新确认候选'
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

<div class="shell">
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-mark">G</div>
      <div><strong>Gesso</strong><span>Pet Studio</span></div>
    </div>

    <nav aria-label="主导航">
      {#each nav as [id, label]}
        <button class:active={active === id} class="nav-item" onclick={() => (active = id)}>
          {label}
        </button>
      {/each}
    </nav>

    <div class="sidebar-pet">
      <span class:online={status === 'connected'} class="status-dot"></span>
      <div><span>本机 Gesso</span><small>{status === 'connected' ? '已连接' : '连接中'}</small></div>
    </div>
  </aside>

  <main class="main">
    <header class="topbar">
      <div><span class="eyebrow">DESKTOP COMPANION</span><h1>{nav.find(([id]) => id === active)?.[1]}</h1></div>
      <div class="topbar-actions"><button class="quiet-button" onclick={toggleTheme} aria-label="切换主题">{theme === 'dark' ? '浅色' : '深色'}</button><button class="quiet-button" onclick={refresh}>刷新状态</button></div>
    </header>

    {#if error}
      <div class="error-banner" role="alert">无法读取 Gesso 状态：{error}<button onclick={refresh}>重试</button></div>
    {/if}

    {#if active === 'overview'}
      {#if pet}
        <section class="hero-grid">
          <div class="preview-stage">
            <div class="stage-label">LIVE SNAPSHOT</div>
            <div class="desk-line"></div>
            <img class="pet-image" src={previewUrl('idle')} alt={pet.name.zh} />
          </div>
          <div class="hero-copy">
            <span class="tag">{pet.behavior}</span>
            <h2>{pet.name.zh}</h2>
            <p>它正在桌面上安静地生活。</p>
            <div class="actions"><button class="primary" onclick={() => (active = 'lab')}>预览动作</button><button class="secondary" disabled={opBusy} onclick={() => runOp('pet.set_visible', { visible: !pet.visible })}>{pet.visible ? '显示中' : '已隐藏'}</button></div>
          </div>
        </section>
        <section class="stats-grid">
          <div><span>陪伴时间</span><strong>{Math.floor(pet.stats.companion_seconds / 3600)}h</strong></div>
          <div><span>被摸</span><strong>{pet.stats.touch_count}</strong></div>
          <div><span>被戳</span><strong>{pet.stats.poke_count}</strong></div>
        </section>
      {:else}
        <section class="empty-card"><span class="eyebrow">NOT ADOPTED</span><h2>还没有一位桌面居民</h2><p>选择一个角色，生成三位候选，确认后它才会来到你的桌面。</p><button class="primary" onclick={() => (active = 'adoption')}>开始领养</button></section>
      {/if}
    {:else if active === 'adoption'}
      <section class="adoption-page"><div class="adoption-heading"><span class="eyebrow">ADOPTION</span><h2>让一位小生命住进桌面</h2><p>候选只存在于当前领养会话中，确认前不会写入存档。</p></div>{#if adoption.candidates?.length}<div class="candidate-grid">{#each adoption.candidates as candidate}<article class:selected={adoption.selected === candidate.index} class="candidate-card"><div class="candidate-art"><img class="candidate-img" src={previewUrl('idle', candidate.candidate_id)} alt={candidate.name.zh} /></div><div class="candidate-meta"><strong>{candidate.name.zh}</strong><span>{candidate.name.en}</span><small>个体候选 {candidate.index + 1}</small></div><button class="candidate-select" class:chosen={adoption.selected === candidate.index} disabled={opBusy || adoption.status === 'persisting'} onclick={() => runOp('adoption.select', { index: candidate.index })}>{adoption.selected === candidate.index ? '已选择' : '选择'}</button></article>{/each}</div><div class="adoption-foot"><span class="muted">{adoption.status === 'persisting' ? '正在保存领养资料…' : adoption.status === 'candidate_selected' ? '已选择一位候选' : '选择你喜欢的外观'}</span>{#if opError}<span class="op-error" role="alert">{opError}</span>{/if}<div class="foot-buttons"><button class="secondary" disabled={opBusy || adoption.status === 'persisting'} onclick={() => runOp('adoption.reroll')}>重新抽取</button><button class="secondary" disabled={opBusy || adoption.status === 'persisting'} onclick={() => runOp('adoption.back')}>返回角色选择</button><button class="primary" disabled={opBusy || adoption.selected == null || adoption.status === 'persisting'} onclick={() => runOp('adoption.confirm')}>确认领养</button></div></div>{:else if adoption.status === 'adopted'}<div class="empty-card adoption-empty"><div class="placeholder-art">✦</div><h3>已经有了一位桌面居民</h3><p>{pet ? `当前居民：${pet.name.zh}。重置后才能重新领养。` : '领养已完成，请回到概览查看当前宠物。'}</p><button class="secondary" onclick={() => (active = 'overview')}>返回概览</button></div>{:else}<div class="empty-card adoption-empty"><div class="placeholder-art">✦</div><h3>准备遇见一位新朋友</h3><p>从首发角色「小蜡烛」开始，会为你生成三位候选，确认前不会写入存档。</p>{#if opError}<span class="op-error" role="alert">{opError}</span>{/if}<button class="primary" disabled={opBusy} onclick={() => runOp('adoption.start')}>开始领养小蜡烛</button></div>{/if}</section>
    {:else if active === 'lab'}
      <section class="lab-layout"><div class="lab-preview"><span class="eyebrow">PREVIEW CANVAS</span>{#if pet}<img class="lab-pet-image" src={previewUrl(previewState)} alt={`${previewState} preview`} />{:else}<span class="muted">领养宠物后可预览动作</span>{/if}</div><div class="lab-controls"><span class="eyebrow">PET LAB</span><h2>宠物实验室</h2><p>预览不会改变桌面宠物的真实状态。</p><div class="control-group"><span class="control-label">动作</span><div class="segmented">{#each previewStates as state}<button class:chosen={previewState === state} onclick={() => (previewState = state)}>{state}</button>{/each}</div></div><div class="lab-row"><span class="tag">{pet?.behavior ?? 'idle'}</span><span class="muted">MVP preview</span></div></div></section>
    {:else}
      <section class="page-card"><span class="eyebrow">SETTINGS</span><h2>宠物设置</h2><p>尺寸和活跃度档位暂在 Gesso 桌面窗口调整；显示状态可直接切换。</p><div class="setting-list"><div class="setting-row"><span>显示宠物</span><button class="secondary" disabled={opBusy || !pet} onclick={() => pet && runOp('pet.set_visible', { visible: !pet.visible })}>{pet?.visible ? '显示中' : '已隐藏'}</button></div><div class="setting-row"><span>尺寸</span><strong>{pet?.size ?? '—'}</strong></div><div class="setting-row"><span>活跃度</span><strong>{pet?.activity ?? '—'}</strong></div><div class="setting-row"><span>连接状态</span><strong>{status === 'connected' ? '正常' : '等待连接'}</strong></div></div>{#if opError}<span class="op-error" role="alert">{opError}</span>{/if}</section>
    {/if}
  </main>
</div>
