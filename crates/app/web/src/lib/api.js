/** @typedef {{state_revision:number, adoption_revision:number, adoption:{status:string, species?:string, reroll_generation?:number, selected?:number|null, candidates?:Array<{index:number,candidate_id:string,name:{zh:string,en:string},appearance:{body:number,coat_color:number,marking:number,accent:number}}>} , pet: null|{name:{zh:string,en:string}, species:string, behavior:string, visible:boolean, activity:string, size:string, stats:{companion_seconds:number,touch_count:number,poke_count:number}}}} PetState */

/** @returns {Promise<PetState>} */
export async function getState() {
  const response = await fetch('/api/state', { credentials: 'include' });
  if (!response.ok) throw new Error(`state_${response.status}`);
  return response.json();
}

/**
 * 提交领养写操作（异步）：服务器校验 adoption_revision 乐观锁并去重 request_id，
 * 返回 operation_id；实际执行在 Gesso 主循环，结果经 /api/state 轮询反映。
 * @param {number} adoptionRevision 快照里的 adoption_revision 乐观锁
 * @param {'adoption.start'|'adoption.reroll'|'adoption.select'|'adoption.confirm'|'adoption.back'|'pet.set_visible'} op
 * @param {{index?: number, visible?: boolean}} [payload]
 * @returns {Promise<{operation_id:string, duplicate?:boolean}>}
 */
export async function postOp(adoptionRevision, op, payload = {}) {
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

/**
 * @param {'idle'|'sleep'|'blink'} state
 * @param {string} [candidateId] 领养候选取 id：渲染候选外观而非正式宠物
 */
export function previewUrl(state, candidateId) {
  const suffix = candidateId ? `&candidate=${encodeURIComponent(candidateId)}` : '';
  return `/api/pet/preview?state=${encodeURIComponent(state)}${suffix}`;
}

export async function exchangeBootstrapCode() {
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
