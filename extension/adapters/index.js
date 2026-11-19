import { getPlatform } from '../core/catalog.js';
import { publicHttps, shopEndpoint, wooEndpoint, validGid, fail } from './common.js';
import { executeYouTube, youtubeSession } from './youtube.js';
import { executeWooCommerce } from './woocommerce.js';
import { executeShopify } from './shopify.js';

export { AdapterError } from './common.js';

export function requiredOrigins(account) {
  if (account?.platform === 'youtube') return ['https://www.googleapis.com/*'];
  if (account?.platform === 'woocommerce') return [`${new URL(wooEndpoint(account)).origin}/*`];
  if (account?.platform === 'shopify') return [`${shopEndpoint(account)}/*`];
  return [];
}

export function validateTarget(draft, account, action) {
  draft = draft || {};
  const errors = [];
  const add = (field, message) => errors.push({ field, message });
  const platform = getPlatform(account?.platform);
  if (!platform) return [{ field: 'platform', message: '未知平台。' }];
  if (!['draft', 'publish'].includes(action)) add('action', '操作必须是 draft 或 publish。');
  if (draft?.kind !== platform.kind) add('kind', '内容类型与目标平台不匹配。');
  if (typeof draft?.title !== 'string' || !draft.title.trim()) add('title', '请填写标题。');
  if (platform.mode !== 'api') return errors;
  try { requiredOrigins(account); } catch (error) { add('endpoint', error.message); }
  if (platform.id === 'youtube') {
    if (draft.title?.length > 100) add('title', 'YouTube 标题最多 100 个字符。');
    if (/[<>]/.test(draft.title || '')) add('title', 'YouTube 标题不能包含尖括号。');
    if (new TextEncoder().encode(draft.description || '').length > 5000) add('description', 'YouTube 描述最多 5000 字节。');
    if (!Array.isArray(draft.assetIds) || draft.assetIds.length !== 1) add('assetIds', 'YouTube 任务需要且仅需要一个视频文件。');
    if (!['private', 'unlisted', 'public'].includes(draft.video?.privacy)) add('video.privacy', '请明确选择视频可见性。');
    if (draft.video?.categoryId && !/^\d{1,4}$/.test(draft.video.categoryId)) add('video.categoryId', '视频分类必须是数字 ID。');
    const tags = draft.tags || [];
    if (!Array.isArray(tags) || tags.some((tag) => typeof tag !== 'string')) add('tags', '标签必须是字符串数组。');
    else if (tags.map((tag) => tag.includes(' ') ? `"${tag}"` : tag).join(',').length > 500) add('tags', 'YouTube 标签合计最多 500 个字符。');
  } else {
    const product = draft.product || {};
    if (!/^(?:0|[1-9]\d{0,9})(?:\.\d{1,2})?$/.test(String(product.price ?? ''))) add('product.price', '价格必须是非负十进制金额，最多两位小数。');
    if (!/^[A-Z]{3}$/.test(product.currency || '')) add('product.currency', '币种应为三个大写字母，并与店铺币种一致。');
    if (!Number.isInteger(product.quantity) || product.quantity < 0 || product.quantity > 2147483647) add('product.quantity', '库存数量必须是非负整数。');
    if (!Array.isArray(product.images) || product.images.length > 100) add('product.images', '图片应为最多 100 个 HTTPS 地址。');
    else for (const image of product.images) {
      try {
        // Media URLs may contain signed query strings; endpoints may not.
        const url = new URL(image);
        const base = new URL(url); base.search = ''; base.hash = '';
        publicHttps(base.href);
        if (url.hash) throw new Error();
      } catch { add('product.images', '商品图片必须是可公开访问的 HTTPS 地址。'); break; }
    }
    if (platform.id === 'shopify') {
      if (action === 'publish' && !validGid(product.publicationId, 'Publication')) add('product.publicationId', 'Shopify 上架需要目标销售渠道 Publication ID。');
      if (product.trackInventory && !validGid(product.locationId, 'Location')) add('product.locationId', '库存同步需要 Shopify Location ID。');
      if (!product.trackInventory && product.quantity > 0) add('product.trackInventory', '要设置 Shopify 库存，请启用库存同步并填写地点 ID。');
    }
  }
  return errors;
}

export function canResumeAfterInterruption(platform, checkpoint = {}) {
  if (checkpoint.result) return true;
  if (platform !== 'youtube' || !checkpoint.sessionUrl) return false;
  try { youtubeSession(checkpoint.sessionUrl); return true; } catch { return false; }
}

export async function executeStep(options) {
  const errors = validateTarget(options.draft, options.account, options.action);
  if (errors.length) fail('VALIDATION_FAILED', errors.map((error) => error.message).join(' '));
  switch (options.account.platform) {
    case 'youtube': return executeYouTube(options);
    case 'woocommerce': return executeWooCommerce(options);
    case 'shopify': return executeShopify(options);
    default: fail('ASSISTED_ONLY', '该平台目前提供后台入口与内容准备，请在官方平台完成发布。');
  }
}
