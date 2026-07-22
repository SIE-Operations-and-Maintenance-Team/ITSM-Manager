// ITSM 管理工具 - 前端逻辑
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let currentSeachType = 2;
let currentViewName = '我处理中的';
let currentTickets = [];    // 当前页数据（后端已分页）
let totalCount = 0;         // 当前视图全量总数（后端 count）
let selectedId = null;
let currentAction = null;
let allViews = [];
let pageSize = 50;          // 当前视图页大小：50/100/200，切视图时从 config 读
let currentPage = 1;        // 当前页码，从 1 开始

// 读取某视图持久化的 pageSize，未配置或非法回退 50
async function getPageSizeFor(st) {
  try {
    const cfg = await invoke('get_config', { seachType: st });
    const n = cfg.view_page_sizes?.[String(st)];
    return [50, 100, 200].includes(n) ? n : 50;
  } catch (e) { return 50; }
}

const STATUS_NAME = {
  Create: '新建', Assigning: '待受理', Processing: '处理中', Suspend: '暂挂',
  Resolved: '已解决', Closed: '已关闭', Delete: '已取消', Revoked: '已撤回', Draft: '草稿', Wait: '等待中'
};

const $ = (id) => document.getElementById(id);
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const fmt = (s) => s ? String(s).replace('T', ' ').slice(0, 16) : '-';
const stripHtml = (h) => String(h || '').replace(/<[^>]+>/g, '').trim();

function ageText(unixSec) {
  if (!unixSec) return '未知';
  const mins = Math.floor((Date.now() / 1000 - unixSec) / 60);
  if (mins < 1) return '刚刚';
  if (mins < 60) return `${mins} 分钟前`;
  return `${Math.floor(mins / 60)} 小时前`;
}

function toast(msg, type = '') {
  const t = $('toast');
  t.textContent = msg;
  t.className = 'toast' + (type ? ' ' + type : '');
  t.hidden = false;
  setTimeout(() => { t.hidden = true; }, 2500);
}

async function init() {
  try {
    const creds = await invoke('get_creds');
    if (creds && creds.token) showMain(creds);
    else showLogin();
  } catch (e) {
    showLogin();
  }
}

function showLogin() {
  $('login-screen').hidden = false;
  $('main-screen').hidden = true;
}

function showMain(creds) {
  $('login-screen').hidden = true;
  $('main-screen').hidden = false;
  $('user-name').textContent = creds.user_name || '已登录';
  loadViews();
}

// 登录
$('login-btn').addEventListener('click', async () => {
  $('login-tip').textContent = '正在打开登录窗口，请在弹出窗口中登录 ITSM...';
  try {
    await invoke('open_login');
    await listen('login-success', async (ev) => {
      const c = ev.payload;
      if (!c || !c.token) return;
      await invoke('save_creds', { creds: c });
      $('login-tip').textContent = '登录成功，正在加载...';
      showMain(c);
      toast('登录成功', 'success');
    });
    await listen('login-timeout', () => {
      $('login-tip').textContent = '登录超时，请重试';
    });
  } catch (e) {
    $('login-tip').textContent = '打开登录窗口失败: ' + e;
  }
});

$('logout-btn').addEventListener('click', async () => {
  if (!confirm('确定登出？将清除本地缓存与配置。')) return;
  await invoke('clear_creds');
  showLogin();
});

$('refresh-btn').addEventListener('click', () => invoke('trigger_refresh'));

$('refresh-interval').addEventListener('change', async (e) => {
  const sec = parseInt(e.target.value);
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    cfg.interval_sec = sec;
    await invoke('save_config', { config: cfg });
    toast(sec === 0 ? '已暂停自动刷新' : '间隔已更新', 'success');
  } catch (err) {
    toast('更新间隔失败: ' + err, 'error');
  }
});

// 视图列表
async function loadViews() {
  try {
    const res = await invoke('list_views');
    const views = res.data || [];
    allViews = views;
    const wrap = $('views');
    wrap.innerHTML = '';
    const def = views.find(v => v.viewName?.includes('处理中')) || views[0];
    if (def) { currentSeachType = def.seachType; currentViewName = def.viewName; }
    pageSize = await getPageSizeFor(currentSeachType);
    currentPage = 1;
    await invoke('set_current_page', { seachType: currentSeachType, pageIndex: 1 });
    views.forEach(v => {
      const el = document.createElement('div');
      el.className = 'view-tab' + (v.seachType === currentSeachType ? ' active' : '');
      el.dataset.seachType = v.seachType;
      el.innerHTML = `${esc(v.viewName)}<span class="count">${v.viewCount ?? ''}</span>`;
      el.addEventListener('click', () => switchView(v, el));
      wrap.appendChild(el);
    });
    loadTickets();
  } catch (e) {
    if (String(e).includes('未登录')) showLogin();
    else toast('加载视图失败: ' + e, 'error');
  }
}

// 切视图：读 per-view pageSize，回第 1 页，上报后端
async function switchView(v, el) {
  currentSeachType = v.seachType;
  currentViewName = v.viewName;
  document.querySelectorAll('.view-tab').forEach(t => t.classList.remove('active'));
  el.classList.add('active');
  selectedId = null;
  currentPage = 1;
  pageSize = await getPageSizeFor(currentSeachType);
  await invoke('set_current_page', { seachType: currentSeachType, pageIndex: 1 });
  loadTickets();
}

// 工单列表（真分页：按 currentPage/pageSize 向后端要对应页）
async function loadTickets(silent = false) {
  try {
    const res = await invoke('list_tickets_cached', { seachType: currentSeachType, pageIndex: currentPage, pageSize });
    currentTickets = res.data || [];
    totalCount = res.count ?? currentTickets.length;
    // 越界回退：当前页空但总数>0（末尾删空），clamp 到有效末页重拉一次
    if (currentPage > 1 && currentTickets.length === 0 && totalCount > 0) {
      currentPage = Math.max(1, Math.ceil(totalCount / pageSize));
      await invoke('set_current_page', { seachType: currentSeachType, pageIndex: currentPage });
      return loadTickets(silent);
    }
    renderTable();
    const ageLabel = res.from_cache ? `缓存 · ${ageText(res.fetched_at)}` : '实时';
    $('list-status').textContent = `${currentViewName}：共 ${totalCount} 条 · 第 ${currentPage}/${totalPages()} 页 · ${ageLabel} · ${new Date().toLocaleTimeString()}`;
    if (!silent) {
      invoke('trigger_refresh', { seachType: currentSeachType });
    }
  } catch (e) {
    if (String(e).includes('未登录')) { showLogin(); return; }
    toast('加载失败: ' + e, 'error');
    $('list-status').textContent = '加载失败: ' + e;
  }
}

function totalPages() {
  return Math.max(1, Math.ceil(totalCount / pageSize));
}

function renderTable() {
  const tb = $('ticket-body');
  tb.innerHTML = '';
  if (currentTickets.length === 0) {
    tb.innerHTML = '<tr><td colspan="8" style="text-align:center;color:#8a9099;padding:40px">暂无工单</td></tr>';
    renderPagination();
    return;
  }
  currentTickets.forEach(t => {
    const tr = document.createElement('tr');
    tr.dataset.id = t.incidentId;
    if (t.incidentId === selectedId) tr.classList.add('selected');
    tr.innerHTML = `
      <td class="col-code">${esc(t.incidentCode)}</td>
      <td class="col-subject">${esc(t.orderSubject)}</td>
      <td><span class="tag tag-${t.status}">${STATUS_NAME[t.status] || t.status}</span></td>
      <td><span class="tag tag-${t.priorityName}">${esc(t.priorityName || '')}</span></td>
      <td>${esc(t.contactCustomerGroupName || '')}</td>
      <td>${esc(t.requestorName || '')}</td>
      <td class="col-time">${fmt(t.lastUpdateDate)}</td>
      <td class="col-actions">${actionBtns(t)}</td>`;
    tr.addEventListener('click', (e) => {
      if (e.target.closest('button')) return;
      selectedId = t.incidentId;
      document.querySelectorAll('tbody tr').forEach(r => r.classList.remove('selected'));
      tr.classList.add('selected');
      loadDetail(t);
    });
    tb.appendChild(tr);
  });
  tb.querySelectorAll('button[data-act]').forEach(btn => {
    btn.addEventListener('click', (e) => {
      e.stopPropagation();
      const t = currentTickets.find(x => x.incidentId === btn.dataset.id);
      handleAction(btn.dataset.act, t);
    });
  });
  renderPagination();
}

// 分页控件：总数超过 50 才显示；上一页/下一页 + 每页条数切换（持久化 per-view）
function renderPagination() {
  const pagi = $('pagination');
  if (!pagi) return;
  if (totalCount <= 50) { pagi.hidden = true; return; }
  pagi.hidden = false;
  const tp = totalPages();
  pagi.innerHTML = `
    <button class="btn small" id="pg-prev" ${currentPage <= 1 ? 'disabled' : ''}>上一页</button>
    <span class="pg-info">第 ${currentPage} / ${tp} 页 · 共 ${totalCount} 条</span>
    <button class="btn small" id="pg-next" ${currentPage >= tp ? 'disabled' : ''}>下一页</button>
    <select id="pg-size" class="pg-size" title="每页条数">
      ${[50, 100, 200].map(n => `<option value="${n}" ${n === pageSize ? 'selected' : ''}>${n}/页</option>`).join('')}
    </select>`;
  $('pg-prev').addEventListener('click', () => { if (currentPage > 1) { currentPage--; gotoPage(); } });
  $('pg-next').addEventListener('click', () => { if (currentPage < totalPages()) { currentPage++; gotoPage(); } });
  $('pg-size').addEventListener('change', (e) => changePageSize(Number(e.target.value)));
}

// 翻页：上报页码 + 重新拉该页
async function gotoPage() {
  await invoke('set_current_page', { seachType: currentSeachType, pageIndex: currentPage });
  loadTickets();
}

// 切 pageSize：持久化到 config（per-view）+ 回首页 + 清缓存（read_tickets 自动失效）
async function changePageSize(newSize) {
  pageSize = newSize;
  currentPage = 1;
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    const vps = { ...(cfg.view_page_sizes || {}) };
    vps[String(currentSeachType)] = newSize;
    await invoke('save_config', { config: { ...cfg, view_page_sizes: vps } });
  } catch (e) { toast('保存页大小失败: ' + e, 'error'); }
  await invoke('set_current_page', { seachType: currentSeachType, pageIndex: 1 });
  loadTickets();
}

function actionBtns(t) {
  const s = t.status;
  let html = '';
  if (s === 'Assigning') html += `<button class="btn" data-act="claim" data-id="${t.incidentId}">接单</button>`;
  html += `<button class="btn" data-act="reply" data-id="${t.incidentId}">回复</button>`;
  if (s === 'Processing' || s === 'Suspend' || s === 'Wait') {
    html += `<button class="btn" data-act="resolve" data-id="${t.incidentId}">解决</button>`;
    html += `<button class="btn" data-act="suspend" data-id="${t.incidentId}">暂挂</button>`;
  }
  // 转派：待受理/处理中/暂挂/等待中
  if (s === 'Assigning' || s === 'Processing' || s === 'Suspend' || s === 'Wait') {
    html += `<button class="btn" data-act="reassign" data-id="${t.incidentId}">转派</button>`;
  }
  // 取消：非终态、非已解决
  if (s !== 'Closed' && s !== 'Delete' && s !== 'Revoked' && s !== 'Resolved') {
    html += `<button class="btn" data-act="cancel" data-id="${t.incidentId}">取消</button>`;
  }
  // 关闭：非终态
  if (s !== 'Closed' && s !== 'Delete' && s !== 'Revoked') {
    html += `<button class="btn" data-act="close" data-id="${t.incidentId}">关闭</button>`;
  }
  return html;
}

// 详情（实时，不缓存）
async function loadDetail(t) {
  const pane = $('detail-pane');
  pane.innerHTML = '<div class="detail-empty">加载中...</div>';
  try {
    const [dRes, rRes] = await Promise.all([
      invoke('get_detail', { id: t.incidentId }),
      invoke('list_replies', { incidentId: t.incidentId })
    ]);
    renderDetail(dRes.data || t, rRes.data || []);
  } catch (e) {
    pane.innerHTML = '<div class="detail-empty">加载失败: ' + esc(e) + '</div>';
  }
}

function renderDetail(d, replies) {
  const pane = $('detail-pane');
  const rows = [
    ['单号', d.incidentCode], ['状态', STATUS_NAME[d.status] || d.status],
    ['优先级', d.priorityName], ['影响度', d.effectName],
    ['服务目录', d.serviceFullName || ((d.serviceTypeName || '') + '/' + (d.serviceSubTypeName || ''))],
    ['事件分类', d.incidentTypeName], ['事件来源', d.incidentSourceName],
    ['客户组', d.contactCustomerGroupName], ['提单人', d.requestorName],
    ['建单人', d.buildUserName], ['支持组', d.assignName],
    ['支持人', d.supportName], ['首次接单', d.firstSupportName],
    ['提单时间', fmt(d.creationDate)], ['首次响应', fmt(d.firstResponseTime)],
    ['预计解决', fmt(d.hopeResolvedTime)], ['更新时间', fmt(d.lastUpdateDate)],
  ];
  let html = `<h2>${esc(d.orderSubject)}</h2>`;
  html += `<div class="detail-actions">${actionBtns(d).replace(/class="btn"/g, 'class="btn small"')}</div>`;
  html += '<div class="detail-meta">';
  rows.forEach(([k, v]) => { html += `<div class="k">${esc(k)}</div><div>${esc(v || '-')}</div>`; });
  html += '</div>';
  html += `<h4>回复记录 (${replies.length})</h4>`;
  if (replies.length === 0) html += '<div class="detail-empty">暂无回复</div>';
  replies.forEach(r => {
    html += `<div class="reply-item ${r.isPrivate ? 'internal' : ''}">
      <div class="reply-meta">${esc(r.createdByName || r.createdBy || '系统')} · ${fmt(r.creationDate)}${r.isPrivate ? ' · 内部' : ''}</div>
      <div class="reply-content">${esc(stripHtml(r.detail))}</div></div>`;
  });
  pane.innerHTML = html;
  pane.querySelectorAll('button[data-act]').forEach(btn => {
    btn.addEventListener('click', () => {
      handleAction(btn.dataset.act, { ...d, incidentId: d.incidentId });
    });
  });
}

function handleAction(act, t) {
  currentAction = { act, t };
  if (act === 'claim') return doClaim(t);
  if (act === 'reply') return openDialog('reply-dialog', 'reply-code', 'reply-text', t.incidentCode);
  if (act === 'resolve') return openDialog('resolve-dialog', 'resolve-code', 'resolve-text', t.incidentCode);
  if (act === 'suspend') return openDialog('suspend-dialog', 'suspend-code', 'suspend-text', t.incidentCode);
  if (act === 'reassign') return openReassign(t);
  if (act === 'cancel') return openCancel(t);
  if (act === 'close') return doClose(t);
}

function openDialog(dlgId, codeSpanId, textId, code) {
  $(codeSpanId).textContent = code;
  $(textId).value = '';
  $(dlgId).showModal();
}

document.querySelectorAll('dialog [data-close]').forEach(b => {
  b.addEventListener('click', () => b.closest('dialog').close());
});

async function doClaim(t) {
  if (!confirm(`接单 ${t.incidentCode}？`)) return;
  try {
    const r = await invoke('claim', { id: t.incidentId });
    if (r.code === 800) {
      toast('接单成功', 'success');
      await invoke('invalidate_after_write', { seachType: currentSeachType });
      loadDetail({ ...t, status: 'Processing' });
    } else toast('接单失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('接单失败: ' + e, 'error'); }
}

$('reply-submit').addEventListener('click', async () => {
  const { t } = currentAction;
  const text = $('reply-text').value.trim();
  if (!text) return toast('请输入回复内容');
  try {
    const r = await invoke('reply', { orderId: t.incidentId, detail: text, isPrivate: $('reply-private').checked, orderType: t.orderType || '1' });
    if (r.code === 800) {
      toast('回复成功', 'success');
      $('reply-dialog').close();
      await invoke('invalidate_after_write', { seachType: currentSeachType });
      loadDetail(t);
    } else toast('回复失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('回复失败: ' + e, 'error'); }
});

$('resolve-submit').addEventListener('click', async () => {
  const { t } = currentAction;
  const text = $('resolve-text').value.trim();
  if (!text) return toast('请输入解决方案');
  if (!confirm(`确认解决 ${t.incidentCode}？`)) return;
  try {
    const r = await invoke('resolve', { id: t.incidentId, solution: text });
    if (r.code === 800) {
      toast('解决成功', 'success');
      $('resolve-dialog').close();
      await invoke('invalidate_after_write', { seachType: currentSeachType });
    } else toast('解决失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('解决失败: ' + e, 'error'); }
});

$('suspend-submit').addEventListener('click', async () => {
  const { t } = currentAction;
  const text = $('suspend-text').value.trim();
  if (!text) return toast('请输入暂挂原因');
  if (!confirm(`确认暂挂 ${t.incidentCode}？`)) return;
  try {
    const r = await invoke('suspend', { id: t.incidentId, reason: text });
    if (r.code === 800) {
      toast('暂挂成功', 'success');
      $('suspend-dialog').close();
      await invoke('invalidate_after_write', { seachType: currentSeachType });
    } else toast('暂挂失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('暂挂失败: ' + e, 'error'); }
});

// ============ 补单 / 转派 / 取消 / 关闭 ============

let serviceTree = null;        // 服务目录树（L1 → L2 → L3）
let budanTemplate = null;      // 当前服务目录对应的补单模板
let allSupportGroups = [];     // 全支持组
let allSupportMembers = [];    // 全支持组成员（按 sgId 过滤）
let allotTypes = [];           // 分派类型字典
let budanSel = {               // 补单当前选择
  serviceType: '',             // L2 stId
  serviceSubType: '',          // L3 stId
  customerGroupId: '', customerGroupName: '',
  requestorId: '', requestorName: '',
  supportById: '', supportByName: '',
};

// 通用：填充 <select>
function fillSelect(sel, items, valKey, labelKey, placeholder) {
  sel.innerHTML = `<option value="">${placeholder || '请选择'}</option>` +
    items.map(it => `<option value="${esc(it[valKey])}">${esc(it[labelKey])}</option>`).join('');
}

// 通用 autocomplete：inputId/listId + fetch(keyword)->{data:{data:[...]}} + render(item)->html + onPick(item)
function attachAutocomplete(inputId, listId, fetchFn, renderFn, onPick) {
  const input = $(inputId), list = $(listId);
  let timer = null, lastItems = [];
  const hide = () => { list.hidden = true; };
  const run = () => {
    clearTimeout(timer);
    const q = input.value.trim();
    timer = setTimeout(async () => {
      try {
        const res = await fetchFn(q);
        lastItems = res?.data?.data || [];
      } catch (e) { lastItems = []; }
      if (!lastItems.length) {
        list.innerHTML = `<li class="ac-empty">${q ? '无匹配' : '输入关键字搜索'}</li>`;
      } else {
        list.innerHTML = lastItems.slice(0, 30).map((it, i) => `<li data-i="${i}">${renderFn(it)}</li>`).join('');
      }
      list.hidden = false;
      list.querySelectorAll('li[data-i]').forEach(li => {
        li.addEventListener('click', () => {
          const item = lastItems[Number(li.dataset.i)];
          onPick(item);
          hide();
        });
      });
    }, 300);
  };
  input.addEventListener('input', run);
  input.addEventListener('focus', () => { if (input.value.trim()) run(); });
  input.addEventListener('blur', () => setTimeout(hide, 200));
  document.addEventListener('click', (e) => { if (!input.contains(e.target) && !list.contains(e.target)) hide(); });
}

// ---- 补单 ----

$('budan-btn').addEventListener('click', openBudan);

async function openBudan() {
  // 重置
  budanTemplate = null;
  budanSel = { serviceType: '', serviceSubType: '', customerGroupId: '', customerGroupName: '', requestorId: '', requestorName: '', supportById: '', supportByName: '' };
  $('budan-subject').value = '';
  $('budan-detail').value = '';
  $('budan-cg-input').value = '';
  $('budan-rq-input').value = '';
  $('budan-sp-input').value = '';
  $('budan-l2').innerHTML = '<option value="">二级</option>';
  $('budan-l2').disabled = true;
  $('budan-l3').innerHTML = '<option value="">三级</option>';
  $('budan-l3').disabled = true;
  $('budan-l1').value = '';

  // 加载服务目录树（仅一次）
  if (!serviceTree) {
    try {
      const res = await invoke('list_service_tree');
      serviceTree = res.data || [];
    } catch (e) { return toast('加载服务目录失败: ' + e, 'error'); }
  }
  fillSelect($('budan-l1'), serviceTree, 'code', 'name', '一级（服务大类）');

  // 填默认客户组/提单人（来自配置）
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    if (cfg.default_customer_group_id) {
      budanSel.customerGroupId = cfg.default_customer_group_id;
      budanSel.customerGroupName = cfg.default_customer_group_name || '';
      $('budan-cg-input').value = budanSel.customerGroupName;
    }
    if (cfg.default_requestor_id) {
      budanSel.requestorId = cfg.default_requestor_id;
      budanSel.requestorName = cfg.default_requestor_name || '';
      $('budan-rq-input').value = budanSel.requestorName;
    }
  } catch (e) { /* 忽略，用户可手输 */ }

  $('budan-dialog').showModal();
}

// 服务目录 cascader：L1 → L2（serviceType）→ L3（children）
$('budan-l1').addEventListener('change', () => {
  const l1 = serviceTree.find(n => String(n.code) === $('budan-l1').value);
  const l2s = l1?.serviceType || [];
  fillSelect($('budan-l2'), l2s, 'stId', 'typeName', '二级（服务类型）');
  $('budan-l2').disabled = l2s.length === 0;
  $('budan-l3').innerHTML = '<option value="">三级</option>';
  $('budan-l3').disabled = true;
  budanSel.serviceType = ''; budanSel.serviceSubType = ''; budanTemplate = null;
});

$('budan-l2').addEventListener('change', async () => {
  const l1 = serviceTree.find(n => String(n.code) === $('budan-l1').value);
  const l2 = l1?.serviceType?.find(s => s.stId === $('budan-l2').value);
  const l3s = l2?.children || [];
  fillSelect($('budan-l3'), l3s, 'stId', 'typeName', '三级（子类型）');
  $('budan-l3').disabled = l3s.length === 0;
  budanSel.serviceType = $('budan-l2').value || '';
  budanSel.serviceSubType = ''; budanTemplate = null;
});

$('budan-l3').addEventListener('change', async () => {
  budanSel.serviceSubType = $('budan-l3').value || '';
  if (!budanSel.serviceSubType) { budanTemplate = null; return; }
  try {
    const res = await invoke('get_replenish_template', { leafId: budanSel.serviceSubType });
    budanTemplate = res.data || null;
  } catch (e) { toast('加载补单模板失败: ' + e, 'error'); }
});

// 客户组 autocomplete
attachAutocomplete('budan-cg-input', 'budan-cg-list',
  q => invoke('search_customer_groups', { keyword: q }),
  it => `${esc(it.customerGroupName)}<span class="ac-sub">${esc(it.companyName || '')} ${esc(it.defaultSupportGroupName ? '· 默认组: ' + it.defaultSupportGroupName : '')}</span>`,
  it => {
    budanSel.customerGroupId = it.cgId;
    budanSel.customerGroupName = it.customerGroupName;
    $('budan-cg-input').value = it.customerGroupName;
  });

// 提单人 autocomplete
attachAutocomplete('budan-rq-input', 'budan-rq-list',
  q => invoke('search_base_persons', { keyword: q }),
  it => `${esc(it.psnName)}<span class="ac-sub">${esc(it.depName || '')} ${esc(it.mobile || '')}</span>`,
  it => {
    budanSel.requestorId = it.userId;
    budanSel.requestorName = it.psnName;
    $('budan-rq-input').value = it.psnName;
  });

// 支持人 autocomplete（可选，留空走默认）
attachAutocomplete('budan-sp-input', 'budan-sp-list',
  q => invoke('search_base_persons', { keyword: q }),
  it => `${esc(it.psnName)}<span class="ac-sub">${esc(it.depName || '')} ${esc(it.mobile || '')}</span>`,
  it => {
    budanSel.supportById = it.userId;
    budanSel.supportByName = it.psnName;
    $('budan-sp-input').value = it.psnName;
  });

$('budan-submit').addEventListener('click', async () => {
  const subject = $('budan-subject').value.trim();
  const detail = $('budan-detail').value.trim();
  if (!budanSel.serviceType || !budanSel.serviceSubType) return toast('请选完三级服务目录');
  if (!subject) return toast('请填工单主题');
  if (!detail) return toast('请填详细描述');
  if (!budanSel.customerGroupId) return toast('请选择客户组');
  if (!budanSel.requestorId) return toast('请选择提单人');
  // 支持组（save body 必填）— 优先配置默认，其次提单人选中的客户组默认支持组
  let assignId = '', assignName = '';
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    assignId = cfg.default_support_group_id || '';
    assignName = cfg.default_support_group_name || '';
  } catch (e) {}
  if (!assignId) return toast('未设置默认支持组，请先在"设置"里配置', 'error');
  if (!confirm(`确认补单？\n主题：${subject}\n客户组：${budanSel.customerGroupName}\n支持组：${assignName}`)) return;

  const params = {
    serviceType: budanSel.serviceType,
    serviceSubType: budanSel.serviceSubType,
    orderSubject: subject,
    detail: `<p>${esc(detail)}</p>`,
    priority: '3',
    contactCustomerGroup: budanSel.customerGroupId,
    requestor: budanSel.requestorId,
    assign: assignId,
    supportBy: budanSel.supportById || '',
    effect: '4', urgency: '1', cc: [],
    orderSign: 1,
    contactCustomerGroupName: budanSel.customerGroupName,
    requestorName: budanSel.requestorName,
    assignName, assignLevel: 1,
    supportName: budanSel.supportByName || '',
    relatedorderList: [],
    createTemplateId: budanTemplate?.id || '',
    fileIds: [],
  };
  try {
    const r = await invoke('save_replenish', { params });
    if (r.code === 800) {
      toast('补单成功：' + (r.data || ''), 'success');
      $('budan-dialog').close();
      await invoke('invalidate_after_write', { seachType: currentSeachType });
    } else toast('补单失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('补单失败: ' + e, 'error'); }
});

// ---- 转派 ----

async function openReassign(t) {
  $('reassign-code').textContent = t.incidentCode;
  $('reassign-minutes').value = 0;
  $('reassign-reason').value = '';
  $('reassign-person').innerHTML = '<option value="">请先选支持组</option>';
  $('reassign-person').disabled = true;
  $('reassign-group').value = '';

  // 懒加载下拉数据
  if (allSupportGroups.length === 0) {
    try {
      const res = await invoke('list_support_groups');
      allSupportGroups = res.data || [];
    } catch (e) { return toast('加载支持组失败: ' + e, 'error'); }
  }
  if (allSupportMembers.length === 0) {
    try {
      const res = await invoke('list_support_members');
      allSupportMembers = res.data || [];
    } catch (e) { return toast('加载支持人失败: ' + e, 'error'); }
  }
  if (allotTypes.length === 0) {
    try {
      const res = await invoke('get_dict', { dicType: 'itsm_incident_allot_type' });
      allotTypes = res.data || [];
    } catch (e) { return toast('加载分派类型失败: ' + e, 'error'); }
  }
  fillSelect($('reassign-group'), allSupportGroups, 'sgId', 'supportGroupName', '请选择支持组');
  fillSelect($('reassign-allot'), allotTypes, 'dicCode', 'dicName', '请选择分派类型');
  // 默认选当前支持组
  if (t.assign) { $('reassign-group').value = t.assign; $('reassign-group').dispatchEvent(new Event('change')); }
  $('reassign-dialog').showModal();
}

$('reassign-group').addEventListener('change', () => {
  const sgId = $('reassign-group').value;
  const members = sgId ? allSupportMembers.filter(m => m.sgId === sgId) : [];
  fillSelect($('reassign-person'), members, 'userId', 'userName', '请选择支持人');
  $('reassign-person').disabled = members.length === 0;
});

$('reassign-submit').addEventListener('click', async () => {
  const { t } = currentAction;
  const assign = $('reassign-group').value;
  const supportBy = $('reassign-person').value;
  const allotType = $('reassign-allot').value;
  if (!assign) return toast('请选择支持组');
  if (!allotType) return toast('请选择分派类型');
  if (!confirm(`确认转派 ${t.incidentCode}？`)) return;
  // 找支持组名/支持人名
  const sg = allSupportGroups.find(g => g.sgId === assign);
  const person = allSupportMembers.find(m => m.userId === supportBy);
  const params = {
    incidentId: t.incidentId,
    assigLevel: '',
    assignType: supportBy ? 'user' : 'group',
    status: t.status,
    processTimeMinute: Number($('reassign-minutes').value) || 0,
    assign,
    supportBy,
    allotType,
    operationReason: $('reassign-reason').value.trim(),
    supportName: person?.userName || '',
    assignName: sg?.supportGroupName || '',
  };
  try {
    const r = await invoke('reassign', { params });
    if (r.code === 800) {
      toast('转派成功', 'success');
      $('reassign-dialog').close();
      await invoke('invalidate_after_write', { seachType: currentSeachType });
      loadDetail(t);
    } else toast('转派失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('转派失败: ' + e, 'error'); }
});

// ---- 取消 ----

function openCancel(t) {
  $('cancel-code').textContent = t.incidentCode;
  $('cancel-reason').value = '';
  $('cancel-dialog').showModal();
}

$('cancel-submit').addEventListener('click', async () => {
  const { t } = currentAction;
  if (!confirm(`确认取消工单 ${t.incidentCode}？此操作不可撤销。`)) return;
  try {
    const r = await invoke('cancel_incident', { id: t.incidentId, reason: $('cancel-reason').value.trim() });
    if (r.code === 800) {
      toast('已取消', 'success');
      $('cancel-dialog').close();
      await invoke('invalidate_after_write', { seachType: currentSeachType });
    } else toast('取消失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('取消失败: ' + e, 'error'); }
});

// ---- 关闭 ----

async function doClose(t) {
  if (!confirm(`确认关闭工单 ${t.incidentCode}？`)) return;
  try {
    const r = await invoke('close_incident', { id: t.incidentId });
    if (r.code === 800) {
      toast('已关闭', 'success');
      await invoke('invalidate_after_write', { seachType: currentSeachType });
    } else toast('关闭失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('关闭失败: ' + e, 'error'); }
}

// ============ 全局事件 listener（仅注册一次） ============

listen('tickets-updated', (ev) => {
  const p = ev.payload || {};
  // 刷新的是当前视图 + 当前页 + 当前 pageSize 才替换列表
  if (p.seachType === currentSeachType && p.page_index === currentPage && p.page_size === pageSize) {
    currentTickets = p.data || [];
    totalCount = p.count ?? currentTickets.length;
    renderTable();
    $('list-status').textContent = `${currentViewName}：共 ${totalCount} 条 · 第 ${currentPage}/${totalPages()} 页 · ${ageText(p.fetched_at)} · ${new Date().toLocaleTimeString()}`;
  } else if (p.seachType === currentSeachType) {
    // 同视图非当前页：仅更新 count + 状态栏
    totalCount = p.count ?? totalCount;
    $('list-status').textContent = `${currentViewName}：共 ${totalCount} 条 · 第 ${currentPage}/${totalPages()} 页 · ${ageText(p.fetched_at)} · ${new Date().toLocaleTimeString()}`;
  }
  // 更新对应视图 tab 的 count
  document.querySelectorAll('.view-tab').forEach(tab => {
    if (Number(tab.dataset.seachType) === p.seachType) {
      const span = tab.querySelector('.count');
      if (span) span.textContent = p.count ?? '';
    }
  });
});

listen('need-login', () => showLogin());

listen('refresh-failed', () => toast('刷新连续失败，可能服务异常', 'error'));

// ============ 设置 UI ============

// 设置对话框里临时存的默认值（点确定才落盘）
let settingsDefaults = { cgId: '', cgName: '', rqId: '', rqName: '', sgId: '', sgName: '' };

async function openSettings() {
  const dlg = $('settings-dialog');
  const cfg = await invoke('get_config', { seachType: currentSeachType });
  const wrap = $('settings-whitelist');
  wrap.innerHTML = '';
  allViews.forEach(v => {
    const label = document.createElement('label');
    label.className = 'row';
    label.innerHTML = `<input type="checkbox" data-st="${v.seachType}" ${cfg.whitelist.includes(v.seachType) ? 'checked' : ''}> ${esc(v.viewName)}`;
    wrap.appendChild(label);
  });
  $('settings-interval').value = cfg.interval_sec;

  // 默认值回填
  settingsDefaults = {
    cgId: cfg.default_customer_group_id || '', cgName: cfg.default_customer_group_name || '',
    rqId: cfg.default_requestor_id || '', rqName: cfg.default_requestor_name || '',
    sgId: cfg.default_support_group_id || '', sgName: cfg.default_support_group_name || '',
  };
  $('settings-cg-input').value = settingsDefaults.cgName;
  $('settings-rq-input').value = settingsDefaults.rqName;
  // 支持组下拉（懒加载）
  if (allSupportGroups.length === 0) {
    try { const res = await invoke('list_support_groups'); allSupportGroups = res.data || []; } catch (e) {}
  }
  fillSelect($('settings-sg-select'), allSupportGroups, 'sgId', 'supportGroupName', '请选择');
  $('settings-sg-select').value = settingsDefaults.sgId;
  dlg.showModal();
}

// 设置里的 autocomplete（点选只更新 settingsDefaults，确定时才落盘）
attachAutocomplete('settings-cg-input', 'settings-cg-list',
  q => invoke('search_customer_groups', { keyword: q }),
  it => `${esc(it.customerGroupName)}<span class="ac-sub">${esc(it.companyName || '')}</span>`,
  it => {
    settingsDefaults.cgId = it.cgId; settingsDefaults.cgName = it.customerGroupName;
    $('settings-cg-input').value = it.customerGroupName;
  });
attachAutocomplete('settings-rq-input', 'settings-rq-list',
  q => invoke('search_base_persons', { keyword: q }),
  it => `${esc(it.psnName)}<span class="ac-sub">${esc(it.depName || '')} ${esc(it.mobile || '')}</span>`,
  it => {
    settingsDefaults.rqId = it.userId; settingsDefaults.rqName = it.psnName;
    $('settings-rq-input').value = it.psnName;
  });
$('settings-sg-select').addEventListener('change', () => {
  const g = allSupportGroups.find(x => x.sgId === $('settings-sg-select').value);
  settingsDefaults.sgId = g?.sgId || ''; settingsDefaults.sgName = g?.supportGroupName || '';
});

$('settings-btn').addEventListener('click', openSettings);

$('settings-submit').addEventListener('click', async () => {
  const whitelist = Array.from(document.querySelectorAll('#settings-whitelist input:checked'))
    .map(cb => parseInt(cb.dataset.st));
  const interval_sec = Math.max(30, Math.min(1800, parseInt($('settings-interval').value) || 300));
  if (whitelist.length === 0) return toast('至少选一个视图');
  try {
    await invoke('save_config', {
      config: {
        whitelist, interval_sec,
        default_customer_group_id: settingsDefaults.cgId || null,
        default_customer_group_name: settingsDefaults.cgName || null,
        default_requestor_id: settingsDefaults.rqId || null,
        default_requestor_name: settingsDefaults.rqName || null,
        default_support_group_id: settingsDefaults.sgId || null,
        default_support_group_name: settingsDefaults.sgName || null,
      }
    });
    $('settings-dialog').close();
    toast('已保存', 'success');
  } catch (e) {
    toast('保存失败: ' + e, 'error');
  }
});

init();
