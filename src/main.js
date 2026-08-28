// ITSM 管理工具 - 前端逻辑
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let currentSeachType = 2;
let currentViewName = '我处理中的';
let currentTickets = [];    // 当前页数据（后端已分页）
let totalCount = 0;         // 当前视图全量总数（后端 count）
let selectedId = null;
let currentAction = null;
let claimingLock = false;     // 批量/自动接单进行中标志，防并发
const PENDING_CLAIM_VIEW = '待我接单';   // 目标视图 viewName，精确匹配
let autoClaimEnabled = false;             // 内存缓存，config-changed 时刷新
let autoClaimSeachType = null;            // 内存缓存，null 表示未配置
let autoClaimNotify = true;               // 内存缓存：自动接单后弹 Windows 通知
let lastClaimIds = [];                    // 上一轮自动接单的 incidentId 列表，死循环防护
let lastClaimTime = 0;                    // 上一轮自动接单时间戳（ms）
let allViews = [];
let pageSize = 50;          // 当前视图页大小：50/100/200，切视图时从 config 读
let currentPage = 1;        // 当前页码，从 1 开始
let currentSearch = null;   // 搜索态（当前视图工作变量，随 applyView 切换）
let currentFetchedAt = null;   // 当前列表数据时间戳（状态栏"X分钟前"）
let currentIsSearch = false;   // 当前列表是否搜索结果
let viewStates = new Map();    // seachType -> ViewState（per-view 内存快照）
// 自动登录模式标记：null=手动；'startup'=启动过渡；'silent-boot'=开机自启静默；'silent-runtime'=运行中静默
let autoLoginMode = null;
// 自动登录确定性失败/验证码放弃标志：带 10 分钟冷却。
// 旧实现一旦置 true 整个会话不再自动重登（重启才恢复），是"过期退出后不自动登录"的根因之一。
let autoLoginGaveUpAt = 0;   // 0=未放弃；否则为放弃时刻 ms
const GAVE_UP_COOLDOWN_MS = 10 * 60 * 1000;
const gaveUpActive = () => autoLoginGaveUpAt > 0 && (Date.now() - autoLoginGaveUpAt) < GAVE_UP_COOLDOWN_MS;

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

// 搜索状态下拉选项（STATUS_NAME 反转：value=后端 code，label=中文）
const STATUS_OPTIONS = Object.entries(STATUS_NAME).map(([k, v]) => ({ value: k, label: v }));

const $ = (id) => document.getElementById(id);
// 登录失效统一判定：token 缺失（"未登录"）或 token 失效（"登录已失效"，ITSM 返 PERMISSION_NOT_PASS）
const isAuthExpired = (e) => /未登录|登录已失效/.test(String(e));
// login-tip 提示：isError=true 时醒目（大字红）
function setTip(msg, isError = false) {
  const t = $('login-tip');
  t.textContent = msg;
  t.classList.toggle('error', isError);
}
const esc = (s) => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const fmt = (s) => s ? String(s).replace('T', ' ').slice(0, 16) : '-';
const stripHtml = (h) => String(h || '').replace(/<[^>]+>/g, '').trim();

// 富文本白名单清洗：防 <script>/on* 事件/javascript: 等执行
const ALLOWED_TAGS = new Set(['P','BR','STRONG','B','EM','I','U','S','STRIKE','UL','OL','LI','A','IMG','SPAN','DIV','H2','H3','H4','PRE','CODE','BLOCKQUOTE']);
const DROP_TAGS = new Set(['SCRIPT','STYLE','NOSCRIPT','IFRAME','OBJECT','EMBED','LINK','META','BASE','FORM','INPUT','BUTTON','SVG']);
const SAFE_ATTR = new Set(['href','src','alt','title','target','colspan','rowspan','start','type']);
function sanitizeHtml(html) {
  const doc = new DOMParser().parseFromString(String(html || ''), 'text/html');
  const walk = (node) => {
    [...node.childNodes].forEach(child => {
      if (child.nodeType === Node.ELEMENT_NODE) {
        const tag = child.tagName;
        if (DROP_TAGS.has(tag)) { node.removeChild(child); return; }
        // 先剥离危险属性
        [...child.attributes].forEach(attr => {
          const name = attr.name.toLowerCase();
          const val = String(attr.value);
          if (name.startsWith('on') || !SAFE_ATTR.has(name)) {
            child.removeAttribute(attr.name);
          } else if ((name === 'href' || name === 'src') && /^\s*javascript:/i.test(val)) {
            child.removeAttribute(attr.name);
          }
        });
        // 先递归清理后代，再决定保留或解包
        walk(child);
        if (!ALLOWED_TAGS.has(tag)) {
          while (child.firstChild) node.insertBefore(child.firstChild, child);
          node.removeChild(child);
        }
      } else if (child.nodeType === Node.COMMENT_NODE) {
        node.removeChild(child);
      }
    });
  };
  walk(doc.body);
  return doc.body.innerHTML;
}

function fileToBase64(file) {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => {
      const s = String(r.result || '');
      resolve(s.slice(s.indexOf(',') + 1));
    };
    r.onerror = () => reject('读取文件失败');
    r.readAsDataURL(file);
  });
}

// 富文本编辑器（wangEditor v5）。懒创建：首次 openDialog 时实例化并缓存。
const richEditors = {};
function createRichEditor(toolbarId, contentId) {
  // wangEditor v5：必须先建 editor，再建 toolbar 并把 editor 传入，否则 toolbar 报 editor is null
  const editor = wangEditor.createEditor({
    selector: '#' + contentId,
    mode: 'default',
    html: '<p><br></p>',
    config: {
      placeholder: '请输入内容',
      MENU_CONF: {
        uploadImage: {
          maxFileSize: 10 * 1024 * 1024,
          allowedFileTypes: ['image/*'],
          async customUpload(file, insertFn) {
            try {
              const b64 = await fileToBase64(file);
              const r = await invoke('upload_attachment', { fileName: file.name, mime: file.type || 'image/png', fileBase64: b64 });
              insertFn(r.file_path, r.file_name || file.name, r.file_path);
            } catch (e) {
              toast('图片上传失败: ' + e, 'error');
            }
          }
        }
      }
    }
  });
  const toolbar = wangEditor.createToolbar({
    selector: '#' + toolbarId,
    editor,
    mode: 'default',
    config: {
      excludeKeys: ['headerSelect','group','todo','emotion','insertVideo','insertTable','codeBlock','divide','quote','color','bgColor','justifyLeft','justifyRight','justifyCenter','justifyJustify','indent','unIndent','lineHeight'],
    }
  });
  return {
    getHtml() {
      const h = editor.getHtml();
      return h === '<p><br></p>' ? '' : h;
    },
    reset() { editor.setHtml('<p><br></p>'); },
  };
}
function ensureEditor(kind) {
  if (!window.wangEditor) { toast('富文本组件未加载', 'error'); return null; }
  if (!richEditors[kind]) richEditors[kind] = createRichEditor(kind + '-toolbar', kind + '-editor');
  return richEditors[kind];
}

// 详情面板宽度拖动（20%–70%）+ 持久化
function initResizer() {
  const resizer = $('resizer');
  const content = document.querySelector('.content');
  if (!resizer || !content) return;
  let dragging = false, startX = 0, startPct = 0, saveTimer = null;
  const setPct = (pct) => {
    document.documentElement.style.setProperty('--detail-w', pct + '%');
  };
  const curPct = () => parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--detail-w')) || 35;
  const onMove = (e) => {
    if (!dragging) return;
    // resizer 在 list 与 detail 之间：向右拖应让 detail 变窄，故用减号
    const pct = Math.max(20, Math.min(70, startPct - (e.clientX - startX) / content.clientWidth * 100));
    setPct(pct);
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => invoke('save_detail_width', { pct }).catch(() => {}), 300);
  };
  const onUp = () => {
    if (!dragging) return;
    dragging = false;
    document.removeEventListener('mousemove', onMove);
    document.removeEventListener('mouseup', onUp);
    resizer.classList.remove('active');
    clearTimeout(saveTimer);
    invoke('save_detail_width', { pct: curPct() }).catch(() => {});
  };
  resizer.addEventListener('mousedown', (e) => {
    dragging = true; startX = e.clientX; startPct = curPct();
    resizer.classList.add('active');
    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    e.preventDefault();
  });
  resizer.addEventListener('dblclick', () => {
    setPct(35);
    invoke('save_detail_width', { pct: 35 }).catch(() => {});
  });
}

// 可拖动 + 可缩放弹窗几何持久化（localStorage，纯 UI 偏好，不走后端 config）
function applyDialogGeom(dlg) {
  if (!dlg) return;
  const vw = window.innerWidth, vh = window.innerHeight;
  let g = null;
  try { g = JSON.parse(localStorage.getItem('dlg-geom-v2-' + dlg.id) || ''); } catch (e) {}
  // 尺寸：有记录用记录，否则用 CSS 默认宽度
  if (g) {
    dlg.style.width = Math.min(g.w, vw - 20) + 'px';
    dlg.style.height = Math.min(g.h, vh - 20) + 'px';
  }
  // 位置：每次弹出居中（模态弹窗不记忆位置）
  const w = dlg.offsetWidth || 720;
  const h = dlg.offsetHeight || 400;
  dlg.style.left = Math.max(0, Math.round((vw - w) / 2)) + 'px';
  dlg.style.top = Math.max(0, Math.round((vh - h) / 2)) + 'px';
}

function persistDialogGeom(dlg) {
  if (!dlg) return;
  const r = dlg.getBoundingClientRect();
  try {
    // 只记忆尺寸，不记忆位置（每次弹出居中）
    localStorage.setItem('dlg-geom-v2-' + dlg.id, JSON.stringify({
      w: Math.round(r.width), h: Math.round(r.height),
    }));
  } catch (e) { /* localStorage 不可用时静默跳过 */ }
}

// 标题栏 h3 作拖动手柄
function makeDraggable(dlg) {
  const handle = dlg.querySelector('h3');
  if (!handle) return;
  let dragging = false, sx = 0, sy = 0, ox = 0, oy = 0;
  const onMove = (e) => {
    if (!dragging) return;
    const nx = Math.max(0, Math.min(window.innerWidth - 60, ox + e.clientX - sx));
    const ny = Math.max(0, Math.min(window.innerHeight - 40, oy + e.clientY - sy));
    dlg.style.left = nx + 'px';
    dlg.style.top = ny + 'px';
  };
  const onUp = () => {
    if (!dragging) return;
    dragging = false;
    document.body.style.userSelect = '';
    persistDialogGeom(dlg);
  };
  handle.addEventListener('mousedown', (e) => {
    dragging = true; sx = e.clientX; sy = e.clientY;
    const r = dlg.getBoundingClientRect();
    ox = r.left; oy = r.top;
    // 锁定当前位置为显式 left/top，后续 mousemove 才能基于此偏移
    dlg.style.left = ox + 'px'; dlg.style.top = oy + 'px';
    document.body.style.userSelect = 'none';
    e.preventDefault();
  });
  document.addEventListener('mousemove', onMove);
  document.addEventListener('mouseup', onUp);
}

// 打开可拖缩弹窗：showModal 后立即应用几何（不依赖 toggle 事件，WebView2 时序更可靠）
function openDlg(dlg) {
  dlg.showModal();
  applyDialogGeom(dlg);
}

function enableResizableDialogs() {
  document.querySelectorAll('dialog.dialog-resizable').forEach(dlg => {
    makeDraggable(dlg);
    // 鼠标松开时持久化尺寸（resize 手柄/标题栏拖动结束；此时 dialog 仍 visible，rect 有效）
    dlg.addEventListener('mouseup', () => persistDialogGeom(dlg));
  });
}

async function applyDetailWidth() {
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    const pct = cfg.detail_width_pct ?? 35;
    document.documentElement.style.setProperty('--detail-w', pct + '%');
  } catch (e) { /* 默认 35% */ }
}

// 启动时把顶部刷新间隔下拉同步到 config.interval_sec（HTML 默认 selected=30s，不回显会与配置脱节）
async function applyRefreshInterval() {
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    const sec = cfg.interval_sec;
    $('refresh-interval').value = [0, 30, 60, 120, 300].includes(sec) ? String(sec) : '30';
  } catch (e) { /* 默认 30 */ }
}

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
  // 用 popover（top layer）显示，避免被 dialog 遮罩层(::backdrop)盖住
  try { t.hidePopover(); } catch (e) {}   // 已开先隐藏，防重复 show 抛异常
  t.showPopover();
  clearTimeout(t._toastTimer);
  t._toastTimer = setTimeout(() => { try { t.hidePopover(); } catch (e) {} }, 2500);
}

async function init() {
  const hidden = await invoke('is_start_hidden');
  let cfg = { auto_login_enabled: false };
  try { cfg = await invoke('get_config', { seachType: currentSeachType }); } catch (e) {}
  let creds = null;
  try { creds = await invoke('get_creds'); } catch (e) {}

  if (creds && creds.token) {
    if (cfg.auto_login_enabled) {
      // 启动主动验证 token（设计第 3 节 B）；verify 期间显示登录页骨架避免白屏
      if (!hidden) {
        showLogin();
        setTip('正在自动登录...');
        $('login-btn').disabled = true;   // 覆盖 verify 等待窗口，给"自动登录中"反馈
      }
      try {
        const valid = await invoke('verify_token');
        if (valid) { showMain(creds); return; }
        // 失效 → 自动登录
        startAutoLogin(hidden ? 'silent-boot' : 'startup');
      } catch (e) {
        // 暂时性错误：过渡停登录页 tip / 静默重试
        if (hidden) {
          setTimeout(() => init_retry_verify(hidden), 5000);  // 简单退避重试
        } else {
          showLogin();
          setTip('网络异常，请稍后重试', true);
        }
      }
    } else {
      showMain(creds);   // 未勾自动登录：乐观进主界面（现状）
    }
    return;
  }
  // 无 token
  if (cfg.auto_login_enabled) {
    startAutoLogin(hidden ? 'silent-boot' : 'startup');
  } else {
    showLogin();
  }
}

// 静默启动 verify_token 网络重试（最多 3 次）
async function init_retry_verify(hidden, attempt = 1) {
  try {
    const valid = await invoke('verify_token');
    let creds = null;
    try { creds = await invoke('get_creds'); } catch (e) {}
    if (valid && creds) { showMain(creds); return; }
    startAutoLogin(hidden ? 'silent-boot' : 'startup');
  } catch (e) {
    if (attempt < 3) {
      setTimeout(() => init_retry_verify(hidden, attempt + 1), 5000 * attempt);
    } else {
      invoke('send_system_notification', {
        title: 'ITSM 管理工具', body: '网络异常，自动登录失败，请打开应用手动登录'
      }).catch(() => {});
    }
  }
}

// 切到登录屏并复位登录态；每次进入都用已存账密（"记住密码"）填充空框
async function showLogin() {
  $('login-screen').hidden = false;
  $('main-screen').hidden = true;
  // 复位登录态：首次登录成功后 disabled 残留 true，tip 残留"登录成功"
  $('login-btn').disabled = false;
  setTip('');
  // 加载已存账密：token 过期、登出、need-login 等回到登录页都会触发
  try {
    const stored = await invoke('load_stored_cred');
    if (stored && stored.account) {
      // 仅填充空框，避免覆盖用户正在输入的新账号/密码
      if (!$('login-account').value) $('login-account').value = stored.account;
      if (!$('login-password').value) $('login-password').value = stored.password;
      $('login-remember').checked = true;
    }
  } catch (e) {}
  // 读 config 同步自动登录勾选；强联动 #login-remember
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    $('login-auto').checked = !!cfg.auto_login_enabled;
    syncRememberLock();
  } catch (e) {}
}

// 强联动：勾 #login-auto 则勾并禁用 #login-remember；取消则恢复
function syncRememberLock() {
  const auto = $('login-auto').checked;
  const rem = $('login-remember');
  if (auto) { rem.checked = true; rem.disabled = true; }
  else { rem.disabled = false; }
}

let bootUpdateChecked = false;   // 启动静默检查更新去重（每次会话仅一次）
function showMain(creds) {
  $('login-screen').hidden = true;
  $('main-screen').hidden = false;
  $('user-name').textContent = creds.user_name || '已登录';
  applyRefreshInterval();
  loadViews();
  // 进主屏后静默检查更新一次（延迟避开启动高峰）；checkForUpdate 为函数声明，已提升
  if (!bootUpdateChecked) {
    bootUpdateChecked = true;
    setTimeout(() => checkForUpdate(true), 3000);
  }
}

// 登录（自建：账密 → login_auto 注入外部窗口自动填充）
async function doLogin() {
  autoLoginMode = null;   // 手动登录：清除自动登录模式标记
  autoLoginGaveUpAt = 0; // 手动登录：重置放弃标志
  const account = $('login-account').value.trim();
  const password = $('login-password').value;
  if (!account || !password) { setTip('请输入账号和密码', true); return; }
  setTip('正在自动登录...');
  $('login-btn').disabled = true;
  try {
    await invoke('login_auto', { account, password });
    // login-success listener（顶层）接管 showMain
  } catch (e) {
    setTip('登录启动失败: ' + e, true);
    $('login-btn').disabled = false;
  }
}

// 自动登录触发：mode ∈ {'startup','silent-boot','silent-runtime'}
async function startAutoLogin(mode) {
  if (autoLoginMode !== null || gaveUpActive()) return;   // 防抖 + 冷却期内的失败放弃
  autoLoginMode = mode;   // 同步前置：关闭 await 窗口，防 need-login 抢入致 login_auto 并发
  const stored = await invoke('load_stored_cred');
  if (!stored || !stored.account) {
    autoLoginMode = null;   // 无账密回退，复位标记
    fallbackNoCred(mode);
    return;
  }
  if (mode === 'startup') {
    showLogin();
    setTip('正在自动登录...');
    $('login-btn').disabled = true;   // 自动登录期间置灰，避免重复触发（失败由 login-failed 复位）
  }
  // silent-* 不切屏、不 tip；login_auto 开隐藏 webview
  invoke('login_auto', { account: stored.account, password: stored.password }).catch(e => {
    if (mode === 'startup') { setTip('登录启动失败: ' + e, true); $('login-btn').disabled = false; }
    autoLoginMode = null;
  });
}

// 无账密降级：过渡停登录页 tip；静默发通知
function fallbackNoCred(mode) {
  if (mode === 'startup') {
    showLogin();
    setTip('自动登录已开启但未存账密，请手动登录并勾选记住密码', true);
  } else {
    invoke('send_system_notification', {
      title: 'ITSM 管理工具',
      body: '自动登录未存账密，请打开应用登录'
    }).catch(() => {});
  }
}
$('login-btn').addEventListener('click', doLogin);
$('login-password').addEventListener('keydown', (e) => { if (e.key === 'Enter') doLogin(); });
$('login-auto').addEventListener('change', syncRememberLock);

// 外部窗口登录（降级：验证码 / SSO / 异常账号）
$('login-external-btn').addEventListener('click', async () => {
  setTip('正在打开登录窗口，请在弹出窗口中登录 ITSM...');
  try {
    await invoke('open_login');
  } catch (e) {
    setTip('打开登录窗口失败: ' + e, true);
  }
});

// open_login beacon 回传成功（顶层注册一次，避免重复 listen）
listen('login-success', (ev) => {
  const c = ev.payload;
  if (!c || !c.token) return;
  const mode = autoLoginMode;
  autoLoginMode = null;   // 复位（成功结束）
  autoLoginGaveUpAt = 0;

  if (mode === 'silent-boot') {
    // 开机自启静默成功：主窗口虽隐藏，但 DOM 仍停在初始登录屏——先切主屏，
    // 用户打开窗口时看到的才是主界面；刷新逻辑与 silent-runtime 相同
    showMain(c);
    invoke('trigger_refresh', {}).catch(() => {});  // seachType=None → restart loop
    return;
  }
  if (mode === 'silent-runtime') {
    // 静默：不切屏、不 toast；写回 token 由后端 save_creds_internal 已做；
    // restart scheduler 全白名单刷新（失效前那次拉取失败，含当前视图）
    setTip('');
    invoke('trigger_refresh', {}).catch(() => {});  // seachType=None → restart loop
    return;
  }
  // startup / null（手动）：落地勾选 + showMain + toast（Task 8 Step 5 逻辑）
  const remember = $('login-remember').checked;
  const auto = $('login-auto').checked;
  const acct = $('login-account').value.trim();
  const pwd = $('login-password').value;
  // 记住密码 / 自动登录：勾选则存账密（keychain），未勾选则清除
  if ((remember || auto) && acct && pwd) {
    invoke('save_stored_cred', { cred: { account: acct, password: pwd } }).catch(() => {});
  } else {
    invoke('clear_stored_cred').catch(() => {});
  }
  // auto_login_enabled 落地（登录页是手动登录唯一入口，此处覆盖 config）
  invoke('get_config', { seachType: currentSeachType }).then(cur => {
    cur.auto_login_enabled = auto;
    return invoke('save_config', { config: cur });
  }).catch(() => {});
  setTip('登录成功，正在加载...');
  showMain(c);
  toast('登录成功', 'success');
});
listen('login-timeout', () => {
  const mode = autoLoginMode;
  if (mode === null && $('login-screen').hidden) return;   // mode 已复位且回主屏 = login-success 刚成功：忽略迟到超时
  autoLoginMode = null;   // 超时=非确定性失败：复位放行后续 need-login，不进冷却
  if (mode === 'startup' || mode === null) {
    setTip('登录超时，请重试', true);
    $('login-btn').disabled = false;
  }
  // silent-*：不切屏，等下一轮 need-login 自动再试
});
listen('login-aborted', () => {
  const mode = autoLoginMode;
  if (mode === null && $('login-screen').hidden) return;   // 同 timeout 幂等防护：登录已成功则忽略
  autoLoginMode = null;   // 登录窗被用户关闭=非确定性放弃：复位放行后续 need-login，不进冷却
  if (mode === 'startup' || mode === null) {
    setTip('登录窗口已关闭，请重试', true);
    $('login-btn').disabled = false;
  }
  // silent-*：不切屏，等下一轮 need-login 自动再试
});
listen('login-failed', async (ev) => {
  const msg = ev.payload || '登录失败';
  const mode = autoLoginMode;
  autoLoginMode = null;
  autoLoginGaveUpAt = Date.now();   // 确定性失败：进入冷却（10 分钟后 need-login 可再试）
  if (mode === 'silent-boot') {
    // 开机自启静默：不弹窗，发通知；主窗口保持隐藏
    invoke('send_system_notification', {
      title: 'ITSM 管理工具', body: '自动登录失败：' + msg + '，请打开应用手动登录'
    }).catch(() => {});
  } else if (mode === 'silent-runtime') {
    // 运行中静默：切登录页 fallback（主窗口可见）
    await showLogin();          // await 后 setTip 才不会被 showLogin 内部 setTip('') 覆盖
    setTip(msg, true);
    $('login-btn').disabled = false;
  } else {
    // startup / null（手动）：停登录页 tip
    setTip(msg, true);
    $('login-btn').disabled = false;
  }
});
listen('login-captcha', () => {
  const mode = autoLoginMode;
  // captcha 不复位 autoLoginMode（登录未结束，等 beacon 回传 success/failed）
  if (mode === 'silent-boot') {
    // 开机自启静默：无法静默处理验证码 → 抑制 webview show + 发通知 + 放弃
    autoLoginMode = null;
    autoLoginGaveUpAt = Date.now();
    invoke('send_system_notification', {
      title: 'ITSM 管理工具', body: '自动登录需要验证码，请打开应用手动登录'
    }).catch(() => {});
  } else {
    // startup / silent-runtime / null：提示 + webview show 让用户输验证码（login_auto 后端已 show）
    setTip('需要验证码，请在弹出的 ITSM 窗口手动输入验证码后点登录', true);
  }
});

$('logout-btn').addEventListener('click', async () => {
  if (!confirm('确定登出？将清除本地缓存与登录凭证（用户设置会保留）。')) return;
  await invoke('clear_creds');
  // 清前端 per-view 快照 + 工作变量，避免下个账号看到上个账号数据
  viewStates.clear();
  currentSearch = null;
  currentTickets = [];
  totalCount = 0;
  selectedId = null;
  currentPage = 1;
  currentFetchedAt = null;
  currentIsSearch = false;
  // 用户主动登出=本会话放弃自动登录：置"永久冷却"挡住 scheduler 下轮 need-login 自动登回。
  // 手动登录成功（doLogin）会复位为 0；不改持久配置（auto_login_enabled 保持用户设置）
  autoLoginGaveUpAt = Number.MAX_SAFE_INTEGER;
  showLogin();
});

$('refresh-btn').addEventListener('click', () => {
  // 搜索态：保持搜索重新拉取；非搜索态：原 scheduler 强制刷新
  if (currentSearch) loadTickets(false);
  else invoke('trigger_refresh');
});

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

// 从后端刷新自动接单内存变量（init / config-changed 调）
async function refreshAutoClaimConfig() {
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    autoClaimEnabled = !!cfg.auto_claim_enabled;
    autoClaimSeachType = cfg.auto_claim_seach_type ?? null;
    autoClaimNotify = cfg.auto_claim_notify ?? true;
  } catch (e) { /* 静默 */ }
}

// 首次启动补默认：把"待我接单"加进白名单 + 填 auto_claim_seach_type。
// 仅在"未自定义"时改：白名单长度<=1 且不含该视图。
async function ensureFirstRunDefaults() {
  const pendingView = allViews.find(v => v.viewName === PENDING_CLAIM_VIEW);
  if (!pendingView) return;   // 后端无此视图，静默
  const pendingSt = pendingView.seachType;
  try {
    const cfg = await invoke('get_config', { seachType: currentSeachType });
    let changed = false;
    if (cfg.whitelist.length <= 1 && !cfg.whitelist.includes(pendingSt)) {
      cfg.whitelist = Array.from(new Set([...cfg.whitelist, pendingSt]));
      changed = true;
    }
    if (cfg.auto_claim_seach_type == null) {
      cfg.auto_claim_seach_type = pendingSt;
      changed = true;
    }
    if (changed) await invoke('save_config', { config: cfg });
  } catch (e) { /* 静默，不阻塞视图加载 */ }
}

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
    await ensureFirstRunDefaults();
    await refreshAutoClaimConfig();
    loadTickets();
  } catch (e) {
    if (isAuthExpired(e)) showLogin();
    else toast('加载视图失败: ' + e, 'error');
  }
}

// 工作变量 + 当前表单 DOM → 当前视图快照
function saveCurrentView() {
  viewStates.set(currentSeachType, {
    seachType: currentSeachType,
    search: currentSearch,
    form: collectForm(),
    currentPage,
    tickets: currentTickets,
    totalCount,
    selectedId,
    fetchedAt: currentFetchedAt,
    isSearch: currentIsSearch,
  });
}

// 快照 → 工作变量 + 回填表单 + 渲染列表/状态栏 + 详情面板
// 关键：必须同步 currentSearch 工作变量，tickets-updated 守卫(if (currentSearch) return) 依赖它
function applyView(state) {
  currentSearch = state.search;
  currentPage = state.currentPage;
  currentTickets = state.tickets;
  totalCount = state.totalCount;
  selectedId = state.selectedId;
  currentFetchedAt = state.fetchedAt;
  currentIsSearch = state.isSearch;
  applyForm(state.form);
  renderTable();
  renderListStatus();
  // 详情面板：选中行在当前列表则静默重载，否则恢复空态
  const t = selectedId ? currentTickets.find(x => x.incidentId === selectedId) : null;
  if (t) loadDetail(t);
  else $('detail-pane').innerHTML = '<div class="detail-empty">点击左侧工单查看详情</div>';
}

// 切视图：存当前视图状态 → 取目标视图快照（命中秒切，未命中首次加载）
async function switchView(v, el) {
  saveCurrentView();
  currentSeachType = v.seachType;
  currentViewName = v.viewName;
  document.querySelectorAll('.view-tab').forEach(t => t.classList.remove('active'));
  el.classList.add('active');
  pageSize = await getPageSizeFor(currentSeachType);
  const st = viewStates.get(currentSeachType);
  if (st) {
    applyView(st);   // 命中：秒切恢复（页码/搜索/列表/选中）
  } else {
    applyView(initEmptyState(currentSeachType));  // 首次：默认空态
    loadTickets();                                  // 后拉取，成功后 saveCurrentView
  }
  // 上报当前页码（scheduler 据此刷对应页）；applyView 已设 currentPage 工作变量
  await invoke('set_current_page', { seachType: currentSeachType, pageIndex: currentPage });
}

// 工单列表（真分页：按 currentPage/pageSize 向后端要对应页）
async function loadTickets(silent = false) {
  try {
    const res = await invoke('list_tickets_cached', { seachType: currentSeachType, pageIndex: currentPage, pageSize, search: currentSearch });
    currentTickets = res.data || [];
    totalCount = res.count ?? currentTickets.length;
    currentIsSearch = res.search === true;
    currentFetchedAt = res.fetched_at ?? null;
    // 越界回退：当前页空但总数>0（末尾删空），clamp 到有效末页重拉一次
    if (currentPage > 1 && currentTickets.length === 0 && totalCount > 0) {
      currentPage = Math.max(1, Math.ceil(totalCount / pageSize));
      await invoke('set_current_page', { seachType: currentSeachType, pageIndex: currentPage });
      return loadTickets(silent);
    }
    renderTable();
    renderListStatus();
    saveCurrentView();   // 成功才落快照（失败保留旧快照）
    // 搜索态不触发 scheduler 后台刷新（scheduler 只刷默认列表，与搜索解耦）
    if (!silent && !currentIsSearch) {
      invoke('trigger_refresh', { seachType: currentSeachType });
    }
  } catch (e) {
    if (isAuthExpired(e)) { showLogin(); return; }
    toast('加载失败: ' + e, 'error');
    $('list-status').textContent = '加载失败: ' + e;
  }
}

// 写操作成功后刷新列表：
//   1) invalidate_after_write：失效默认列表缓存 + 后台拉默认列表 + restart loop 刷其他视图
//   2) 搜索态下，tickets-updated 监听器会忽略后台默认列表刷新（防覆盖搜索结果），
//      故再按 currentSearch 显式重拉一次，让搜索结果同步反映写操作变化
async function refreshAfterWrite(seachType) {
  await invoke('invalidate_after_write', { seachType });
  if (currentSearch && seachType === currentSeachType) {
    await loadTickets(false);
  }
}

// ============ per-view 状态快照 ============

// 新视图默认空态（首次进入，拉取前用）
function initEmptyState(st) {
  return {
    seachType: st,
    search: null,
    form: { kw: '', status: '', dateBegin: '', dateEnd: '', cg: '' },
    currentPage: 1,
    tickets: [],
    totalCount: 0,
    selectedId: null,
    fetchedAt: null,
    isSearch: false,
  };
}

// 读搜索条 5 个控件当前值 → form 对象（saveCurrentView 落快照用）
function collectForm() {
  return {
    kw: $('s-kw').value,
    status: $('s-status').value,
    dateBegin: $('s-date-begin').value,
    dateEnd: $('s-date-end').value,
    cg: $('s-cg').value,
  };
}

// form 对象 → 回填 5 个控件（applyView 切回视图用）
function applyForm(form) {
  $('s-kw').value = form.kw || '';
  $('s-status').value = form.status || '';
  $('s-date-begin').value = form.dateBegin || '';
  $('s-date-end').value = form.dateEnd || '';
  $('s-cg').value = form.cg || '';
}

// ============ 列表搜索 ============

// 读搜索条控件 → 条件对象。返回：null=无条件；undefined=校验失败（已 toast）；object=有条件
function collectSearch() {
  const begin = $('s-date-begin').value;
  const end = $('s-date-end').value;
  if ((begin && !end) || (!begin && end)) {
    toast('请选择完整的提单日期范围', 'error');
    return undefined;
  }
  const kw = $('s-kw').value.trim();
  const status = $('s-status').value;
  const cg = $('s-cg').value.trim();
  if (!kw && !status && !begin && !cg) return null;
  return {
    codeAndSubject: kw || undefined,
    status: status || undefined,
    creationDateBegin: begin || undefined,
    creationDateEnd: end || undefined,
    contactCustomerGroupName: cg || undefined,
  };
}

function doSearch() {
  const s = collectSearch();
  if (s === undefined) return;
  currentSearch = s;
  currentPage = 1;
  invoke('set_current_page', { seachType: currentSeachType, pageIndex: 1 });
  loadTickets();
}

function clearSearch() {
  ['s-kw', 's-status', 's-date-begin', 's-date-end', 's-cg'].forEach(id => { $(id).value = ''; });
  currentSearch = null;
  currentPage = 1;
  invoke('set_current_page', { seachType: currentSeachType, pageIndex: 1 });
  loadTickets();
}

function initSearchUI() {
  const sel = $('s-status');
  STATUS_OPTIONS.forEach(o => {
    const op = document.createElement('option');
    op.value = o.value;
    op.textContent = o.label;
    sel.appendChild(op);
  });
  attachAutocomplete('s-cg', 's-cg-list',
    q => invoke('search_customer_groups', { keyword: q }),
    it => esc(it.customerGroupName),
    it => { $('s-cg').value = it.customerGroupName; });
  $('search-btn').addEventListener('click', doSearch);
  $('search-clear').addEventListener('click', clearSearch);
  $('s-kw').addEventListener('keydown', e => { if (e.key === 'Enter') doSearch(); });
  $('s-cg').addEventListener('keydown', e => { if (e.key === 'Enter') doSearch(); });
  $('s-status').addEventListener('change', doSearch);
}

// 状态栏统一渲染：搜索态显示"搜索结果"，否则显示"缓存 · X分钟前"
function renderListStatus() {
  const ageLabel = currentIsSearch ? '搜索结果'
    : (currentFetchedAt ? `缓存 · ${ageText(currentFetchedAt)}` : '-');
  $('list-status').textContent = `${currentViewName}：共 ${totalCount} 条 · 第 ${currentPage}/${totalPages()} 页 · ${ageLabel} · ${new Date().toLocaleTimeString()}`;
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
    updateClaimAllBtn();
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
  updateClaimAllBtn();
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
    // Suspend 状态显示「解挂」，其余显示「暂挂」
    if (s === 'Suspend') {
      html += `<button class="btn" data-act="unhang" data-id="${t.incidentId}">解挂</button>`;
    } else {
      html += `<button class="btn" data-act="suspend" data-id="${t.incidentId}">暂挂</button>`;
    }
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
    // ITSM 附件在回复的独立 fileList 字段（不在正文 HTML 内），需单独渲染
    const filesHtml = (r.fileList || []).map(f => `
      <a class="attach-item attach-link" href="${esc(f.filePath)}" data-url="${esc(f.filePath)}" data-name="${esc(f.sourceFileName || f.phyFileName || '附件')}" title="点击下载：${esc(f.sourceFileName)}">
        <span class="attach-name">📎 ${esc(f.sourceFileName || f.phyFileName || '附件')}</span>
        <span class="attach-size">${f.fileSize ? fmtSize(f.fileSize) : ''}</span>
      </a>`).join('');
    html += `<div class="reply-item ${r.isPrivate ? 'internal' : ''}">
      <div class="reply-meta">${esc(r.userName || r.createdByName || r.createdBy || '系统')} · ${fmt(r.replyTime || r.creationDate)}${r.isPrivate ? ' · 内部' : ''}</div>
      <div class="reply-content">${sanitizeHtml(r.detail)}</div>
      ${filesHtml ? `<div class="attach-list reply-attachments">${filesHtml}</div>` : ''}</div>`;
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
  if (act === 'reply') return openRichDialog('reply', 'reply-dialog', 'reply-code', t.incidentCode);
  if (act === 'resolve') return openRichDialog('resolve', 'resolve-dialog', 'resolve-code', t.incidentCode);
  if (act === 'suspend') return openDialog('suspend-dialog', 'suspend-code', 'suspend-text', t.incidentCode);
  if (act === 'unhang') return doUnhang(t);
  if (act === 'reassign') return openReassign(t);
  if (act === 'cancel') return openCancel(t);
  if (act === 'close') return doClose(t);
}

function openDialog(dlgId, codeSpanId, textId, code) {
  $(codeSpanId).textContent = code;
  $(textId).value = '';
  $(dlgId).showModal();
}

function openRichDialog(kind, dlgId, codeSpanId, code) {
  $(codeSpanId).textContent = code;
  const ed = ensureEditor(kind);
  if (ed) ed.reset();
  if (kind === 'reply') resetReplyAttachments();
  openDlg($(dlgId));
}

document.querySelectorAll('dialog [data-close]').forEach(b => {
  b.addEventListener('click', () => b.closest('dialog').close());
});

// ============ 图片大图预览：详情面板内 img 双击弹出，滚轮缩放 / 拖动 / 多图切换 ============
// 复用原生 <dialog> 的 modal 语义（Esc 关闭、backdrop、顶层层级），不与表单 dialog 共用内容结构
const IP_MIN_SCALE = 0.2, IP_MAX_SCALE = 5;
let ipImages = [];      // 当前详情面板内的 img 列表
let ipIndex = 0;        // 当前图片索引
let ipScale = 1, ipTx = 0, ipTy = 0;   // transform 状态（transform-origin: 0 0）
let ipDrag = null;      // 拖动状态 { sx, sy, tx0, ty0 }

function ipApply() {
  const img = $('img-preview')?.querySelector('.ip-img');
  if (img) img.style.transform = `translate(${ipTx}px, ${ipTy}px) scale(${ipScale})`;
}

// 切到第 i 张并重置变换；越界回绕
function ipShow(i) {
  const dlg = $('img-preview');
  if (!dlg || ipImages.length === 0) return;
  ipIndex = (i + ipImages.length) % ipImages.length;
  const src = ipImages[ipIndex];
  const img = dlg.querySelector('.ip-img');
  img.src = src.src;
  img.alt = src.alt || '';
  ipScale = 1; ipTx = 0; ipTy = 0;
  img.style.transform = '';
  const multi = ipImages.length > 1;
  dlg.querySelector('.ip-count').textContent = multi ? `${ipIndex + 1} / ${ipImages.length}` : '';
  dlg.querySelector('.ip-prev').hidden = !multi;
  dlg.querySelector('.ip-next').hidden = !multi;
}

// 打开预览：收集 detail-pane 内全部 img，以双击的 img 为起点
function openImagePreview(target) {
  const pane = $('detail-pane');
  const all = pane ? [...pane.querySelectorAll('img')] : [];
  if (!all.includes(target)) all.unshift(target);
  ipImages = all.length ? all : [target];
  ipShow(ipImages.indexOf(target));
  $('img-preview').showModal();
}

function initImagePreview() {
  const dlg = $('img-preview');
  if (!dlg) return;
  const img = dlg.querySelector('.ip-img');

  // 委托：详情面板内双击 img 弹大图（一次绑定，renderDetail 重渲染无需重绑）
  const pane = $('detail-pane');
  if (pane) {
    pane.addEventListener('dblclick', e => {
      if (e.target.tagName === 'IMG' && e.target.src) openImagePreview(e.target);
    });
  }

  // 滚轮缩放：以鼠标位置为锚点（origin 0 0 下 tx += kx*(1-ns/old)）
  img.addEventListener('wheel', e => {
    e.preventDefault();
    const old = ipScale;
    const ns = Math.max(IP_MIN_SCALE, Math.min(IP_MAX_SCALE, old * (e.deltaY < 0 ? 1.15 : 1 / 1.15)));
    if (ns === old) return;
    const r = img.getBoundingClientRect();
    ipTx += (e.clientX - r.left) * (1 - ns / old);
    ipTy += (e.clientY - r.top) * (1 - ns / old);
    ipScale = ns;
    ipApply();
  }, { passive: false });

  // 双击图片：适应窗口 ↔ 实际像素尺寸（1:1）；origin 0 0 下补偿 translate 使中心不动
  img.addEventListener('dblclick', e => {
    e.stopPropagation();
    if (ipScale === 1 && ipTx === 0 && ipTy === 0) {
      const cw = img.clientWidth, ch = img.clientHeight;
      if (img.naturalWidth && cw) {
        const r = Math.max(img.naturalWidth / cw, img.naturalHeight / ch);
        if (r > 1.01) {
          ipScale = r;
          ipTx = cw * (1 - r) / 2;
          ipTy = ch * (1 - r) / 2;
          ipApply();
        }
      }
    } else {
      ipScale = 1; ipTx = 0; ipTy = 0; ipApply();
    }
  });

  // 拖动平移（mousedown 在 img，move/up 走 document 以便鼠标越出 img 仍生效）
  img.addEventListener('mousedown', e => {
    if (e.button !== 0) return;
    ipDrag = { sx: e.clientX, sy: e.clientY, tx0: ipTx, ty0: ipTy };
    img.classList.add('dragging');
    e.preventDefault();
  });
  document.addEventListener('mousemove', e => {
    if (!ipDrag) return;
    ipTx = ipDrag.tx0 + (e.clientX - ipDrag.sx);
    ipTy = ipDrag.ty0 + (e.clientY - ipDrag.sy);
    ipApply();
  });
  document.addEventListener('mouseup', () => {
    if (!ipDrag) return;
    ipDrag = null;
    img.classList.remove('dragging');
  });

  // 左右切换（按钮 + 键盘 ←→）
  dlg.querySelector('.ip-prev').addEventListener('click', () => ipShow(ipIndex - 1));
  dlg.querySelector('.ip-next').addEventListener('click', () => ipShow(ipIndex + 1));
  dlg.addEventListener('keydown', e => {
    if (e.key === 'ArrowLeft') ipShow(ipIndex - 1);
    else if (e.key === 'ArrowRight') ipShow(ipIndex + 1);
  });

  // 点 backdrop（target===dialog 自身）关闭
  dlg.addEventListener('click', e => { if (e.target === dlg) dlg.close(); });

  // 关闭后清理状态与 src（释放图片资源）
  dlg.addEventListener('close', () => {
    ipImages = []; ipIndex = 0; ipScale = 1; ipTx = 0; ipTy = 0; ipDrag = null;
    img.src = ''; img.style.transform = ''; img.classList.remove('dragging');
  });
}

// 单条接单 API 调用。成功返 {ok:true}；业务/网络失败返 {ok:false,msg}；
// token 失效（命中 isAuthExpired）向上抛，供批量接单中断。
async function claimOne(id) {
  try {
    const r = await invoke('claim', { id });
    if (r.code === 800) return { ok: true };
    return { ok: false, msg: r.msg || '' };
  } catch (e) {
    if (isAuthExpired(e)) throw e;
    return { ok: false, msg: String(e) };
  }
}

// 更新一键接单按钮显隐 + 计数。
// 目标视图（按 seachType 识别，与自动接单同源）+ 未启用自动接单 + 有数据 才显示
function updateClaimAllBtn() {
  const btn = $('claim-all-btn');
  // 目标视图（按 seachType 识别，与自动接单同源）+ 未启用自动接单 即显示；无单时数字留空
  const isTargetView = autoClaimSeachType != null && currentSeachType === autoClaimSeachType;
  const show = isTargetView && !autoClaimEnabled;
  btn.hidden = !show;
  if (show) $('claim-all-count').textContent = currentTickets.length > 0 ? currentTickets.length : '';
}

// 一键接单当前页：顺序循环 claimOne，汇总成功/失败，末尾刷新
async function doClaimAll() {
  if (claimingLock) return toast('正在接单中...');
  if (currentTickets.length === 0) return toast('当前无工单可接单');
  if (!confirm(`确认接单当前页 ${currentTickets.length} 条？`)) return;
  claimingLock = true;
  let ok = 0, fail = 0;
  const failedIds = [];
  try {
    for (const t of currentTickets) {
      const res = await claimOne(t.incidentId);
      if (res.ok) ok++; else { fail++; failedIds.push(t.incidentCode || t.incidentId); }
    }
    toast(`接单完成：成功 ${ok} 条${fail ? '，失败 ' + fail + ' 条' : ''}`, fail ? 'error' : 'success');
    if (fail) console.warn('接单失败单号', failedIds);
  } catch (e) {
    if (isAuthExpired(e)) { showLogin(); toast(`接单中断（token 失效）：已接 ${ok} 条`, 'error'); }
    else toast('接单异常: ' + e, 'error');
  } finally {
    claimingLock = false;
    await refreshAfterWrite(currentSeachType);
  }
}

async function doClaim(t) {
  if (!confirm(`接单 ${t.incidentCode}？`)) return;
  try {
    const res = await claimOne(t.incidentId);
    if (res.ok) {
      toast('接单成功', 'success');
      await refreshAfterWrite(currentSeachType);
      loadDetail({ ...t, status: 'Processing' });
    } else toast('接单失败: ' + res.msg, 'error');
  } catch (e) {
    if (isAuthExpired(e)) { showLogin(); return; }
    toast('接单失败: ' + e, 'error');
  }
}

$('claim-all-btn').addEventListener('click', doClaimAll);

// ============ 回复附件：选择后即传 ITSM 附件服务，提交时以 fileIds 随回复发送 ============
const ATTACH_MAX_BYTES = 50 * 1024 * 1024;
const replyAttachments = [];   // { fileId, fileName, size, status: 'uploading'|'ok'|'error', error }

function fmtSize(n) {
  return n >= 1048576 ? (n / 1048576).toFixed(1) + ' MB' : Math.max(1, Math.round(n / 1024)) + ' KB';
}

function renderReplyAttachments() {
  const list = $('reply-attach-list');
  list.hidden = replyAttachments.length === 0;
  list.innerHTML = replyAttachments.map((a, i) => `
    <div class="attach-item">
      <span class="attach-name" title="${esc(a.fileName)}">${esc(a.fileName)}</span>
      <span class="attach-size">${fmtSize(a.size)}</span>
      <span class="attach-status ${a.status}" title="${esc(a.status === 'error' ? a.error : '')}">${a.status === 'uploading' ? '上传中…' : a.status === 'ok' ? '已上传' : '失败'}</span>
      <button type="button" class="btn" data-rm="${i}">移除</button>
    </div>`).join('');
  list.querySelectorAll('button[data-rm]').forEach(btn => {
    btn.addEventListener('click', () => {
      replyAttachments.splice(Number(btn.dataset.rm), 1);
      renderReplyAttachments();
    });
  });
}

function resetReplyAttachments() {
  replyAttachments.length = 0;
  renderReplyAttachments();
}

async function addReplyAttachment(file) {
  if (file.size > ATTACH_MAX_BYTES) return toast(`「${file.name}」超过 50MB，已跳过`, 'error');
  const item = { fileId: null, fileName: file.name, size: file.size, status: 'uploading', error: '' };
  replyAttachments.push(item);
  renderReplyAttachments();
  try {
    const b64 = await fileToBase64(file);
    const r = await invoke('upload_attachment', { fileName: file.name, mime: file.type || 'application/octet-stream', fileBase64: b64 });
    item.fileId = r.file_id;
    item.status = 'ok';
  } catch (e) {
    item.status = 'error';
    item.error = String(e);
  }
  renderReplyAttachments();
}

$('reply-attach-btn').addEventListener('click', () => $('reply-attach-input').click());
$('reply-attach-input').addEventListener('change', async (e) => {
  const files = [...e.target.files];
  e.target.value = '';   // 清空以允许再次选择同名文件
  for (const f of files) await addReplyAttachment(f);   // 串行上传，避免并发挤压
});

let replySubmitting = false;
$('reply-submit').addEventListener('click', async () => {
  const { t } = currentAction;
  const ed = ensureEditor('reply');
  if (!ed) return;
  const html = ed.getHtml();
  if (!html) return toast('请输入回复内容');
  if (replyAttachments.some(a => a.status === 'uploading')) return toast('附件还在上传中，请稍候', 'error');
  const failed = replyAttachments.filter(a => a.status === 'error');
  if (failed.length > 0 && !confirm(`${failed.length} 个附件上传失败，将不随本回复发送。仍要提交吗？`)) return;
  if (replySubmitting) return;
  replySubmitting = true;
  try {
    const fileIds = replyAttachments.filter(a => a.status === 'ok').map(a => a.fileId);
    const r = await invoke('reply', { orderId: t.incidentId, detail: html, fileIds, isPrivate: $('reply-private').checked, orderType: t.orderType || '1' });
    if (r.code === 800) {
      toast('回复成功', 'success');
      $('reply-dialog').close();
      await refreshAfterWrite(currentSeachType);
      loadDetail(t);
    } else toast('回复失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('回复失败: ' + e, 'error'); }
  finally { replySubmitting = false; }
});

$('resolve-submit').addEventListener('click', async () => {
  const { t } = currentAction;
  const ed = ensureEditor('resolve');
  if (!ed) return;
  const html = ed.getHtml();
  if (!html) return toast('请输入解决方案');
  if (!confirm(`确认解决 ${t.incidentCode}？`)) return;
  try {
    const r = await invoke('resolve', { id: t.incidentId, solution: html });
    if (r.code === 800) {
      toast('解决成功', 'success');
      $('resolve-dialog').close();
      await refreshAfterWrite(currentSeachType);
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
      await refreshAfterWrite(currentSeachType);
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
  const budanEd = ensureEditor('budan'); if (budanEd) budanEd.reset();
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

  // 服务目录默认值：优先用配置的默认服务目录三级，否则按 name 默认选「软件服务」
  let cfg = null;
  try { cfg = await invoke('get_config', { seachType: currentSeachType }); } catch (e) { /* 忽略 */ }
  const defaultL1Code = cfg?.default_service_l1 || (serviceTree.find(n => n.name === '软件服务')?.code || '');
  if (defaultL1Code) {
    $('budan-l1').value = defaultL1Code;
    $('budan-l1').dispatchEvent(new Event('change'));
    // 配置了默认服务目录时联动选 L2/L3（L3 change 触发取 template）
    if (cfg?.default_service_l1 && cfg.default_service_l2) {
      $('budan-l2').value = cfg.default_service_l2;
      $('budan-l2').dispatchEvent(new Event('change'));
      if (cfg.default_service_l3) {
        $('budan-l3').value = cfg.default_service_l3;
        $('budan-l3').dispatchEvent(new Event('change'));
      }
    }
  }

  openDlg($('budan-dialog'));
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
  const budanEd = ensureEditor('budan');
  if (!budanEd) return;
  const detailHtml = budanEd.getHtml();
  if (!budanSel.serviceType || !budanSel.serviceSubType) return toast('请选完三级服务目录');
  if (!subject) return toast('请填工单主题');
  if (!detailHtml) return toast('请填详细描述');
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
    detail: detailHtml,
    fileIds: [],
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
  };
  try {
    const r = await invoke('save_replenish', { params });
    if (r.code === 800) {
      // r.data 是新单 incidentId；二次取 incidentCode 显示单号，失败回退 id
      let label = r.data || '';
      if (label) {
        try {
          const d = await invoke('get_detail', { id: label });
          if (d?.data?.incidentCode) label = d.data.incidentCode;
        } catch (e) { /* 取单号失败，回退显示 id */ }
      }
      toast('补单成功：' + label, 'success');
      $('budan-dialog').close();
      await refreshAfterWrite(currentSeachType);
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
  $('reassign-allot').value = '';

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

// 选分派类型时自动带出 dicName 到「分派原因」（对齐原系统：原因内容赋值到回复内容）
$('reassign-allot').addEventListener('change', () => {
  const code = $('reassign-allot').value;
  const item = allotTypes.find(a => a.dicCode === code);
  if (item) $('reassign-reason').value = item.dicName || '';
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
      await refreshAfterWrite(currentSeachType);
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
      await refreshAfterWrite(currentSeachType);
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
      await refreshAfterWrite(currentSeachType);
    } else toast('关闭失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('关闭失败: ' + e, 'error'); }
}

async function doUnhang(t) {
  if (!confirm(`确认解挂 ${t.incidentCode}？`)) return;
  try {
    const r = await invoke('unhang', { id: t.incidentId });
    if (r.code === 800) {
      toast('已解挂', 'success');
      await refreshAfterWrite(currentSeachType);
      loadDetail(t);
    } else toast('解挂失败: ' + (r.msg || ''), 'error');
  } catch (e) { toast('解挂失败: ' + e, 'error'); }
}

// ============ 全局事件 listener（仅注册一次） ============

// 自动接单触发判定：开关开 + 视图匹配 + 未在接单 + data 非空
async function maybeRunAutoClaim(p) {
  if (!autoClaimEnabled) return;
  if (p.seachType !== autoClaimSeachType) return;
  if (claimingLock) return;
  const data = Array.isArray(p.data) ? p.data : [];
  if (data.length === 0) return;
  await runAutoClaim(data);
}

// 自动批量接单。顺序循环 claimOne + 60s 死循环防护（同批 id 短时间内不重接）
async function runAutoClaim(data) {
  const ids = data.map(t => t.incidentId);
  const now = Date.now();
  const sameAsLast = ids.length === lastClaimIds.length && ids.every(id => lastClaimIds.includes(id));
  if (sameAsLast && now - lastClaimTime < 60_000) {
    toast('自动接单连续无变化，暂停一轮', 'error');
    return;
  }
  claimingLock = true;
  lastClaimIds = ids;
  lastClaimTime = now;
  let ok = 0, fail = 0;
  let lastOk = null;                       // 最后一条成功工单（ok===1 时取详情用）
  const failedIds = [];
  try {
    for (const t of data) {
      const res = await claimOne(t.incidentId);
      if (res.ok) { ok++; lastOk = t; } else { fail++; failedIds.push(t.incidentCode || t.incidentId); }
    }
    if (ok > 0) toast(`自动接单成功 ${ok} 条`, 'success');
    if (fail > 0) { toast(`自动接单失败 ${fail} 条`, 'error'); console.warn('自动接单失败单号', failedIds); }
    // Windows 系统通知：单条带单号+标题（截断），多条给数量
    if (autoClaimNotify) {
      let title = '', body = '';
      if (ok === 1 && lastOk) {
        title = '自动接单成功';
        const full = String(lastOk.orderSubject || '');
        body = `${lastOk.incidentCode || lastOk.incidentId || ''} ${full.slice(0, 30)}${full.length > 30 ? '…' : ''}${fail > 0 ? `（另失败 ${fail} 条）` : ''}`;
      } else if (ok >= 2) {
        title = fail > 0 ? '自动接单完成' : '自动接单成功';
        body = fail > 0 ? `成功 ${ok} 条，失败 ${fail} 条` : `已接 ${ok} 条`;
      } else if (fail > 0) {
        title = '自动接单失败';
        body = `失败 ${fail} 条`;
      }
      if (title) invoke('send_system_notification', { title, body }).catch(() => {});
    }
  } catch (e) {
    if (isAuthExpired(e)) {
      showLogin(); toast(`自动接单中断（token 失效）：已接 ${ok} 条`, 'error');
      if (autoClaimNotify) invoke('send_system_notification', { title: '自动接单中断', body: `登录已失效，已接 ${ok} 条` }).catch(() => {});
    }
    else toast('自动接单异常: ' + e, 'error');
  } finally {
    claimingLock = false;
    await refreshAfterWrite(autoClaimSeachType);
  }
}

listen('tickets-updated', (ev) => {
  const p = ev.payload || {};
  // 视图 tab 角标 count：独立于搜索态，始终更新
  document.querySelectorAll('.view-tab').forEach(tab => {
    if (Number(tab.dataset.seachType) === p.seachType) {
      const span = tab.querySelector('.count');
      if (span) span.textContent = p.count ?? '';
    }
  });
  // 自动接单：后台行为，独立于用户是否在搜索
  // （claimingLock + 60s 同批 id + 接单后视图状态过滤致 data 空，三重防循环）
  maybeRunAutoClaim(p);
  // 搜索态：不替换列表（scheduler 不带 search 条件，避免覆盖搜索结果）
  if (currentSearch) return;
  // 非搜索态：刷新的是当前视图 + 当前页 + 当前 pageSize 才替换列表
  if (p.seachType === currentSeachType && p.page_index === currentPage && p.page_size === pageSize) {
    currentTickets = p.data || [];
    totalCount = p.count ?? currentTickets.length;
    currentFetchedAt = p.fetched_at ?? null;
    currentIsSearch = false;
    renderTable();
    renderListStatus();
    saveCurrentView();
  } else if (p.seachType === currentSeachType) {
    // 同视图非当前页：仅更新 count + 状态栏 + 落快照
    totalCount = p.count ?? totalCount;
    renderListStatus();
    saveCurrentView();
  }
});

listen('need-login', async () => {
  if (autoLoginMode !== null) return;   // 静默登录进行中，防抖忽略
  let cfg = { auto_login_enabled: false };
  try { cfg = await invoke('get_config', { seachType: currentSeachType }); } catch (e) {}
  if (cfg.auto_login_enabled && !gaveUpActive()) {
    startAutoLogin('silent-runtime');
  } else if ($('login-screen').hidden) {
    showLogin();   // 放弃过/未开启 → 手动登录页（已可见则跳过：scheduler 每轮都会 emit，避免清 tip/重置勾选）
  }
});

listen('refresh-failed', () => toast('刷新连续失败，可能服务异常', 'error'));

listen('config-changed', async () => { await refreshAutoClaimConfig(); updateClaimAllBtn(); });

// ============ 设置 UI ============

// 设置对话框里临时存的默认值（点确定才落盘）
let settingsDefaults = { svcL1: '', svcL2: '', svcL3: '', sgId: '', sgName: '' };

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
  const intVal = cfg.interval_sec;
  $('settings-interval').value = [30, 60, 120, 300].includes(intVal) ? intVal : 300;
  $('settings-autostart').checked = !!cfg.autostart_enabled;
  $('settings-auto-login').checked = !!cfg.auto_login_enabled;
  $('settings-min-tray').checked = !!cfg.minimize_to_tray;
  $('settings-auto-claim').checked = !!cfg.auto_claim_enabled;
  $('settings-auto-claim-notify').checked = cfg.auto_claim_notify ?? true;
  const acViewSel = $('settings-auto-claim-view');
  acViewSel.innerHTML = '<option value="">请选择</option>';
  allViews.forEach(v => {
    const o = document.createElement('option');
    o.value = v.seachType;
    o.textContent = v.viewName;
    acViewSel.appendChild(o);
  });
  acViewSel.value = cfg.auto_claim_seach_type ?? '';

  // 默认值回填
  settingsDefaults = {
    svcL1: cfg.default_service_l1 || '', svcL2: cfg.default_service_l2 || '', svcL3: cfg.default_service_l3 || '',
    sgId: cfg.default_support_group_id || '', sgName: cfg.default_support_group_name || '',
  };
  // 服务目录 cascader 初始化（复用全局 serviceTree，懒加载）+ 按默认值预选三级
  if (!serviceTree) {
    try { const res = await invoke('list_service_tree'); serviceTree = res.data || []; } catch (e) {}
  }
  fillSelect($('settings-svc-l1'), serviceTree, 'code', 'name', '一级');
  // 预选用 cfg 原值判断/赋值：cascader 的 change handler 会重置 settingsDefaults 下级
  if (cfg.default_service_l1) {
    $('settings-svc-l1').value = cfg.default_service_l1;
    $('settings-svc-l1').dispatchEvent(new Event('change'));
    if (cfg.default_service_l2) {
      $('settings-svc-l2').value = cfg.default_service_l2;
      $('settings-svc-l2').dispatchEvent(new Event('change'));
      if (cfg.default_service_l3) {
        $('settings-svc-l3').value = cfg.default_service_l3;
        $('settings-svc-l3').dispatchEvent(new Event('change'));
      }
    }
  }
  // 支持组下拉（懒加载）
  if (allSupportGroups.length === 0) {
    try { const res = await invoke('list_support_groups'); allSupportGroups = res.data || []; } catch (e) {}
  }
  fillSelect($('settings-sg-select'), allSupportGroups, 'sgId', 'supportGroupName', '请选择');
  $('settings-sg-select').value = settingsDefaults.sgId;
  // MCP 配置回填
  $('settings-mcp-enabled').checked = !!cfg.mcp_enabled;
  $('settings-mcp-port').value = cfg.mcp_port || 17540;
  const mdSel = $('settings-mcp-default-view');
  mdSel.innerHTML = '<option value="">请选择</option>';
  allViews.forEach(v => {
    const o = document.createElement('option');
    o.value = v.seachType;
    o.textContent = v.viewName;
    mdSel.appendChild(o);
  });
  mdSel.value = cfg.mcp_default_seach_type ?? '';
  // 附件下载配置回填
  $('settings-attach-mode').value = cfg.attachment_download_mode === 'ask' ? 'ask' : 'auto';
  $('settings-attach-dir').value = cfg.attachment_download_dir || '';
  updateAttachDirRow();
  // 关于页：回填当前版本号
  $('about-version').textContent = 'v' + await invoke('get_app_version');
  // 复位到常规 tab（HTML 虽带默认 active，但上次切到的 tab 会保留，再开需复位）
  switchSettingsTab('general');
  openDlg(dlg);
}

// 侧栏分组切换：同步 nav button 与 section 的 .active
function switchSettingsTab(tab) {
  document.querySelectorAll('#settings-dialog .settings-nav button').forEach(b => {
    b.classList.toggle('active', b.dataset.tab === tab);
  });
  document.querySelectorAll('#settings-dialog .settings-body > section').forEach(s => {
    s.classList.toggle('active', s.dataset.pane === tab);
  });
}
document.querySelectorAll('#settings-dialog .settings-nav button').forEach(btn => {
  btn.addEventListener('click', () => switchSettingsTab(btn.dataset.tab));
});

// ============ 关于 / 版本更新 ============
const ABOUT_REPO_URL = 'https://github.com/SIE-Operations-and-Maintenance-Team/ITSM-Manager';

// 前往仓库（系统默认浏览器打开）
$('about-repo-link').addEventListener('click', (e) => {
  e.preventDefault();
  invoke('open_external_url', { url: ABOUT_REPO_URL })
    .catch(e => toast('打开失败: ' + e, 'error'));
});

// 检查更新：silent=true 启动静默检查（无更新/失败不提示），false 关于页手动按钮
async function checkForUpdate(silent) {
  try {
    const info = await invoke('check_update');
    if (!info.available) {
      if (!silent) toast('已是最新版本 (v' + info.current_version + ')', 'success');
      return;
    }
    // 有更新：填充 update-dialog 并弹出
    $('update-title').textContent = '发现新版本';
    $('update-new-version').textContent = 'v' + info.version;
    $('update-cur-version').textContent = '（当前 v' + info.current_version + '）';
    $('update-notes').textContent = info.notes || '(无更新说明)';
    $('update-progress-wrap').hidden = true;
    $('update-progress-fill').style.width = '0%';
    $('update-progress-text').textContent = '';
    $('update-now-btn').disabled = false;
    $('update-now-btn').textContent = '立即更新';
    $('update-later-btn').hidden = false;
    // update-dialog 固定右下角（CSS 定位），不走 applyDialogGeom 的居中计算
    $('update-dialog').showModal();
  } catch (e) {
    if (!silent) toast(String(e), 'error');
  }
}

// 关于页"检查更新"按钮
$('check-update-btn').addEventListener('click', () => {
  $('update-status').textContent = '检查中...';
  checkForUpdate(false).finally(() => { $('update-status').textContent = ''; });
});

// update-dialog"立即更新"：下载（监听进度）→ NSIS 安装 → 重启
$('update-now-btn').addEventListener('click', async () => {
  $('update-now-btn').disabled = true;
  $('update-now-btn').textContent = '下载中...';
  $('update-later-btn').hidden = true;
  $('update-progress-wrap').hidden = false;
  try {
    await invoke('download_and_install_update');
    // Windows：download_and_install 内部已退出进程执行 NSIS 安装，此行一般不可达
    $('update-progress-text').textContent = '安装完成，请重启应用';
  } catch (e) {
    $('update-now-btn').disabled = false;
    $('update-now-btn').textContent = '立即更新';
    $('update-later-btn').hidden = false;
    $('update-progress-wrap').hidden = true;
    toast('更新失败: ' + e, 'error');
  }
});

// 下载进度上报（Rust emit "update-progress"）
listen('update-progress', (ev) => {
  const { downloaded, total } = ev.payload;
  if (total > 0) {
    const pct = Math.min(100, Math.round(downloaded * 100 / total));
    $('update-progress-fill').style.width = pct + '%';
    $('update-progress-text').textContent =
      pct + '%（' + (downloaded / 1048576).toFixed(1) + '/' + (total / 1048576).toFixed(1) + ' MB）';
  } else {
    $('update-progress-text').textContent = '下载中... ' + (downloaded / 1048576).toFixed(1) + ' MB';
  }
});

// 设置里的服务目录 cascader（参照补单 budan-l1/l2 逻辑，仅记选中值，保存时落盘）
$('settings-svc-l1').addEventListener('change', () => {
  const l1 = serviceTree?.find(n => String(n.code) === $('settings-svc-l1').value);
  const l2s = l1?.serviceType || [];
  fillSelect($('settings-svc-l2'), l2s, 'stId', 'typeName', '二级');
  $('settings-svc-l2').disabled = l2s.length === 0;
  $('settings-svc-l3').innerHTML = '<option value="">三级</option>';
  $('settings-svc-l3').disabled = true;
  settingsDefaults.svcL1 = $('settings-svc-l1').value || '';
  settingsDefaults.svcL2 = ''; settingsDefaults.svcL3 = '';
});
$('settings-svc-l2').addEventListener('change', () => {
  const l1 = serviceTree?.find(n => String(n.code) === $('settings-svc-l1').value);
  const l2 = l1?.serviceType?.find(s => s.stId === $('settings-svc-l2').value);
  const l3s = l2?.children || [];
  fillSelect($('settings-svc-l3'), l3s, 'stId', 'typeName', '三级');
  $('settings-svc-l3').disabled = l3s.length === 0;
  settingsDefaults.svcL2 = $('settings-svc-l2').value || '';
  settingsDefaults.svcL3 = '';
});
$('settings-svc-l3').addEventListener('change', () => {
  settingsDefaults.svcL3 = $('settings-svc-l3').value || '';
});
$('settings-sg-select').addEventListener('change', () => {
  const g = allSupportGroups.find(x => x.sgId === $('settings-sg-select').value);
  settingsDefaults.sgId = g?.sgId || ''; settingsDefaults.sgName = g?.supportGroupName || '';
});

// 自动登录勾选即时落盘 config（已登录态 restart 无害；失败回退勾选并 toast）
$('settings-auto-login').addEventListener('change', async (e) => {
  try {
    const cur = await invoke('get_config', { seachType: currentSeachType });
    cur.auto_login_enabled = e.target.checked;
    await invoke('save_config', { config: cur });
  } catch (err) {
    e.target.checked = !e.target.checked;  // 回退
    toast('保存失败: ' + err, 'error');
  }
});

// 附件下载：mode=ask 隐藏保存目录行；「浏览」原生目录选择框
function updateAttachDirRow() {
  $('settings-attach-dir-row').hidden = $('settings-attach-mode').value !== 'auto';
}
$('settings-attach-mode').addEventListener('change', updateAttachDirRow);
$('settings-attach-dir-btn').addEventListener('click', async () => {
  try {
    const dir = await invoke('pick_directory');
    if (dir) $('settings-attach-dir').value = dir;
  } catch (e) {
    toast('选择目录失败: ' + e, 'error');
  }
});

$('settings-btn').addEventListener('click', openSettings);

$('settings-submit').addEventListener('click', async () => {
  const whitelist = Array.from(document.querySelectorAll('#settings-whitelist input:checked'))
    .map(cb => parseInt(cb.dataset.st));
  const interval_sec = parseInt($('settings-interval').value) || 300;
  if (whitelist.length === 0) return toast('至少选一个视图');
  try {
    const cur = await invoke('get_config', { seachType: currentSeachType });
    const min_tray_new = $('settings-min-tray').checked;
    const autostart_new = $('settings-autostart').checked;
    const mcp_enabled_new = $('settings-mcp-enabled').checked;
    const mcp_port_new = Number($('settings-mcp-port').value);
    if (!Number.isInteger(mcp_port_new) || mcp_port_new < 1024 || mcp_port_new > 65535) {
      switchSettingsTab('mcp');
      return toast('MCP 端口必须是 1024–65535 的整数', 'error');
    }
    const mcp_default_view_new = parseInt($('settings-mcp-default-view').value) || 7;
    const mcp_changed = mcp_enabled_new !== cur.mcp_enabled
      || mcp_port_new !== cur.mcp_port
      || mcp_default_view_new !== cur.mcp_default_seach_type;
    await invoke('save_config', {
      config: {
        ...cur,
        whitelist, interval_sec,
        default_service_l1: settingsDefaults.svcL1 || null,
        default_service_l2: settingsDefaults.svcL2 || null,
        default_service_l3: settingsDefaults.svcL3 || null,
        default_support_group_id: settingsDefaults.sgId || null,
        default_support_group_name: settingsDefaults.sgName || null,
        minimize_to_tray: min_tray_new,
        auto_claim_enabled: $('settings-auto-claim').checked,
        auto_claim_seach_type: parseInt($('settings-auto-claim-view').value) || null,
        auto_claim_notify: $('settings-auto-claim-notify').checked,
        mcp_enabled: mcp_enabled_new,
        mcp_port: mcp_port_new,
        mcp_default_seach_type: mcp_default_view_new,
        attachment_download_mode: $('settings-attach-mode').value === 'ask' ? 'ask' : 'auto',
        attachment_download_dir: $('settings-attach-dir').value.trim() || null,
      }
    });
    if (autostart_new !== cur.autostart_enabled) {
      await invoke('set_autostart', { enabled: autostart_new });
    }
    $('settings-dialog').close();
    toast(mcp_changed ? '已保存；MCP 配置重启应用后生效' : '已保存', 'success');
  } catch (e) {
    toast('保存失败: ' + e, 'error');
  }
});

initResizer();
enableResizableDialogs();
initImagePreview();
// 历史回复附件点击 → 应用内下载并用系统默认程序打开（位置按设置：固定目录 / 每次询问）。
// renderDetail 每次 innerHTML 重建，与图片预览同用 detail-pane 事件委托
$('detail-pane').addEventListener('click', async e => {
  const link = e.target.closest('a.attach-link');
  if (!link || link.classList.contains('busy')) return;
  e.preventDefault();
  const name = link.dataset.name || '附件';
  const sizeEl = link.querySelector('.attach-size');
  const originText = sizeEl ? sizeEl.textContent : '';
  link.classList.add('busy');
  if (sizeEl) sizeEl.textContent = '下载中…';
  try {
    const r = await invoke('download_attachment', { url: link.dataset.url, fileName: name });
    if (!r.canceled) {
      toast(r.opened ? `已下载并打开：${name}` : `已下载：${name}（${r.path}）`, 'success');
    }
  } catch (err) {
    toast('下载附件失败: ' + err, 'error');
  } finally {
    link.classList.remove('busy');
    if (sizeEl) sizeEl.textContent = originText;
  }
});
applyDetailWidth();
initSearchUI();
init();
