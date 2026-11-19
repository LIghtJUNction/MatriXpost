import { basicAuth, needCredential, request, readJson, wooEndpoint, htmlText, safeLink, fail } from './common.js';

export async function executeWooCommerce({ draft, account, credentials, action, checkpoint = {}, fetchImpl, signal }) {
  if (checkpoint.result) return { done: true, checkpoint, result: checkpoint.result };
  const endpoint = wooEndpoint(account);
  const key = needCredential(credentials, 'consumerKey');
  const secret = needCredential(credentials, 'consumerSecret');
  const product = draft.product || {};
  if (!checkpoint.currencyVerified) {
    const response = await request(`${endpoint}/wp-json/wc/v3/settings/general/woocommerce_currency`, {
      method: 'GET', headers: { Authorization: basicAuth(key, secret) },
    }, { fetchImpl, signal, mutating: false });
    const setting = await readJson(response, { mutating: false });
    if (!/^[A-Z]{3}$/.test(setting.value || '')) fail('CURRENCY_UNVERIFIED', '无法核实店铺币种，请检查 WooCommerce 读取设置权限。');
    if (setting.value !== product.currency) fail('CURRENCY_MISMATCH', `商品币种与店铺币种 ${setting.value} 不一致，请修正后重新排队。`);
    return { done: false, checkpoint: { currencyVerified: true, storeCurrency: setting.value, stage: 'create_product' } };
  }
  const payload = {
    name: draft.title,
    type: 'simple',
    status: action === 'publish' ? 'publish' : 'draft',
    description: htmlText(draft.description),
    regular_price: String(product.price),
    sku: product.sku || '',
    manage_stock: true,
    stock_quantity: Number(product.quantity || 0),
    images: (product.images || []).map((src) => ({ src })),
  };
  const response = await request(`${endpoint}/wp-json/wc/v3/products`, {
    method: 'POST', headers: { Authorization: basicAuth(key, secret), 'Content-Type': 'application/json' }, body: JSON.stringify(payload),
  }, { fetchImpl, signal });
  const data = await readJson(response);
  if (!Number.isSafeInteger(data.id) || data.id <= 0 || !['draft', 'publish', 'pending', 'private'].includes(data.status)) {
    fail('INVALID_RESPONSE', '平台没有返回可核实的商品 ID 和状态，请核对店铺后台。', { uncertain: true });
  }
  const result = {
    remoteId: String(data.id),
    url: safeLink(data.permalink, `${endpoint}/wp-admin/post.php?post=${data.id}&action=edit`),
    status: data.status === 'publish' ? 'published' : data.status === 'draft' ? 'draft' : 'accepted',
    platformStatus: data.status,
  };
  return { done: true, checkpoint: { ...checkpoint, remoteId: result.remoteId, result }, result };
}
