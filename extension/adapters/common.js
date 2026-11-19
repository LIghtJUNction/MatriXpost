export class AdapterError extends Error {
  constructor(code, message, { retryable = false, uncertain = false, httpStatus } = {}) {
    super(message);
    this.name = 'AdapterError';
    this.code = code;
    this.retryable = retryable;
    this.uncertain = uncertain;
    if (httpStatus) this.httpStatus = httpStatus;
  }
}

export function fail(code, message, options) {
  throw new AdapterError(code, message, options);
}

/** Public DNS names only. A browser cannot prove DNS answers are public. */
export function publicHttps(value, { exactHosts, rootOnly = false } = {}) {
  let url;
  try { url = new URL(value); } catch { fail('INVALID_ENDPOINT', '请输入完整的 HTTPS 店铺地址。'); }
  const host = url.hostname.toLowerCase();
  if (url.protocol !== 'https:' || url.username || url.password || url.port || url.hash || url.search ||
      !host.includes('.') || host.endsWith('.') || host.startsWith('[') || /^\d+(?:\.\d+)*$/.test(host) ||
      /(?:^|\.)(localhost|local|internal|test|invalid|example|onion|lan|intranet|home|home\.arpa)$/.test(host) ||
      (exactHosts && !exactHosts.includes(host)) ||
      (rootOnly && !['', '/'].includes(url.pathname))) {
    fail('INVALID_ENDPOINT', '地址必须是公开 HTTPS 域名，不含账号、端口、查询参数或片段。');
  }
  return url;
}

export function shopEndpoint(account) {
  const url = publicHttps(account.endpoint, { rootOnly: true });
  if (!/^[a-z0-9][a-z0-9-]*\.myshopify\.com$/.test(url.hostname)) {
    fail('INVALID_ENDPOINT', 'Shopify 地址必须是你的 https://店铺名.myshopify.com。');
  }
  return url.origin;
}

export function wooEndpoint(account) {
  const url = publicHttps(account.endpoint);
  return url.href.replace(/\/$/, '');
}

export function cleanText(value) { return String(value ?? ''); }
export function htmlText(value) {
  return cleanText(value).replace(/[&<>"']/g, (ch) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch])).replace(/\r?\n/g, '<br>');
}
export function needCredential(credentials, key) {
  const value = credentials?.[key];
  if (typeof value !== 'string' || !value.trim() || /[\r\n]/.test(value)) {
    fail('CREDENTIAL_REQUIRED', '平台凭据缺失或无效，请重新配置该账号。');
  }
  return value.trim();
}

export function basicAuth(username, password) {
  const bytes = new TextEncoder().encode(`${username}:${password}`);
  let text = '';
  for (const byte of bytes) text += String.fromCharCode(byte);
  return `Basic ${btoa(text)}`;
}

export async function request(url, options = {}, { fetchImpl = fetch, signal, mutating = true, accepted = [] } = {}) {
  const timeout = AbortSignal.timeout(25000);
  const combined = signal ? AbortSignal.any([signal, timeout]) : timeout;
  let response;
  try {
    response = await fetchImpl(url, { ...options, signal: combined, redirect: 'error', credentials: 'omit', cache: 'no-store' });
  } catch {
    fail('NETWORK_ERROR', mutating ? '请求中断，平台结果尚不确定。请先核对平台后台，再决定是否重试。' : '无法连接平台，请检查网络后重试。', { retryable: !mutating, uncertain: mutating });
  }
  if (!response.ok && !accepted.includes(response.status)) {
    const code = response.status === 401 ? 'AUTH_EXPIRED' : response.status === 403 ? 'PERMISSION_DENIED' : response.status === 429 ? 'RATE_LIMITED' : 'PLATFORM_ERROR';
    const message = response.status === 401 ? '平台授权已失效，请更新账号凭据。' : response.status === 403 ? '平台拒绝了该操作，请检查应用和账号权限。' : response.status === 429 ? '平台限流，请稍后重试。' : `平台返回 HTTP ${response.status}，请在平台后台检查任务状态。`;
    fail(code, message, { httpStatus: response.status, retryable: !mutating && (response.status === 429 || response.status >= 500), uncertain: mutating && response.status >= 500 });
  }
  return response;
}

export async function readJson(response, { mutating = true } = {}) {
  try {
    const value = await response.json();
    if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error();
    return value;
  } catch {
    fail('INVALID_RESPONSE', '平台响应无法确认，请检查平台后台。', { uncertain: mutating });
  }
}

export function safeLink(value, fallback) {
  try {
    const url = new URL(value);
    if (url.protocol === 'https:' && !url.username && !url.password) return url.href;
  } catch { /* Missing public link is normal for private content. */ }
  return fallback;
}

export function validGid(value, type) {
  return new RegExp(`^gid://shopify/${type}/[0-9]+$`).test(value || '');
}
