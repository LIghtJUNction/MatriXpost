import { needCredential, request, readJson, shopEndpoint, htmlText, validGid, fail } from './common.js';

export const SHOPIFY_API_VERSION = '2026-10';

const QUERIES = {
  currency: 'query MatriXpostCurrency { shop { currencyCode } }',
  create: `mutation MatriXpostCreate($product: ProductCreateInput!, $media: [CreateMediaInput!]) {
    productCreate(product: $product, media: $media) {
      product { id status variants(first: 1) { nodes { id inventoryItem { id } } } }
      userErrors { field message }
    }
  }`,
  price: `mutation MatriXpostVariant($productId: ID!, $variants: [ProductVariantsBulkInput!]!) {
    productVariantsBulkUpdate(productId: $productId, variants: $variants, allowPartialUpdates: false) {
      productVariants { id price inventoryItem { id tracked sku } }
      userErrors { field message }
    }
  }`,
  inventoryRead: `query MatriXpostInventory($itemId: ID!, $locationId: ID!) {
    inventoryItem(id: $itemId) { id inventoryLevel(locationId: $locationId) { id quantities(names: ["available"]) { name quantity } } }
  }`,
  inventoryActivate: `mutation MatriXpostInventoryActivate($itemId: ID!, $locationId: ID!, $quantity: Int!, $key: String!) {
    inventoryActivate(inventoryItemId: $itemId, locationId: $locationId, available: $quantity) @idempotent(key: $key) {
      inventoryLevel { id quantities(names: ["available"]) { name quantity } }
      userErrors { field message }
    }
  }`,
  inventorySet: `mutation MatriXpostInventorySet($input: InventorySetQuantitiesInput!, $key: String!) {
    inventorySetQuantities(input: $input) @idempotent(key: $key) {
      inventoryAdjustmentGroup { changes { name quantityAfterChange } }
      userErrors { field message }
    }
  }`,
  active: `mutation MatriXpostActivate($product: ProductUpdateInput!) {
    productUpdate(product: $product) { product { id status } userErrors { field message } }
  }`,
  publish: `mutation MatriXpostPublish($id: ID!, $publicationId: ID!) {
    publishablePublish(id: $id, input: [{publicationId: $publicationId}]) {
      publishable { publishedOnPublication(publicationId: $publicationId) }
      userErrors { field message }
    }
  }`,
};

async function graphql({ endpoint, token, query, variables = {}, operation, fetchImpl, signal, mutating = true }) {
  const response = await request(`${endpoint}/admin/api/${SHOPIFY_API_VERSION}/graphql.json`, {
    method: 'POST', headers: { 'X-Shopify-Access-Token': token, 'Content-Type': 'application/json' },
    body: JSON.stringify({ query, variables }),
  }, { fetchImpl, signal, mutating });
  const data = await readJson(response, { mutating });
  if (Array.isArray(data.errors) && data.errors.length) {
    fail('GRAPHQL_ERROR', 'Shopify 拒绝了 API 请求，请核对 API 权限和店铺配置。', { uncertain: mutating });
  }
  if (!data.data || typeof data.data !== 'object') fail('INVALID_RESPONSE', 'Shopify 未返回可核实的操作结果。', { uncertain: mutating });
  if (!operation) return data.data;
  const result = data.data[operation];
  if (!result || !Array.isArray(result.userErrors)) fail('INVALID_RESPONSE', 'Shopify 未返回完整的操作结果。', { uncertain: mutating });
  if (result.userErrors.length) fail('PLATFORM_VALIDATION', 'Shopify 拒绝了部分字段，请检查商品字段、库存地点或渠道权限。', { uncertain: operation === 'productCreate' && !!result.product?.id });
  return result;
}

function outcome(endpoint, checkpoint, action) {
  const numericId = checkpoint.remoteId.split('/').at(-1);
  return {
    remoteId: checkpoint.remoteId,
    url: `${endpoint}/admin/products/${numericId}`,
    status: action === 'publish' ? 'published' : 'draft',
    platformStatus: action === 'publish' ? 'ACTIVE' : 'DRAFT',
    publicationId: action === 'publish' ? checkpoint.publicationId : undefined,
    inventoryApplied: Boolean(checkpoint.inventoryCompleted),
    mediaStatus: 'platform_managed',
  };
}

export async function executeShopify({ draft, account, credentials, action, checkpoint = {}, fetchImpl, signal }) {
  if (checkpoint.result) return { done: true, checkpoint, result: checkpoint.result };
  const endpoint = shopEndpoint(account);
  const token = needCredential(credentials, 'accessToken');
  const product = draft.product || {};
  const run = (query, variables, operation, mutating = true) => graphql({ endpoint, token, query, variables, operation, fetchImpl, signal, mutating });
  if (!checkpoint.currencyVerified) {
    const data = await run(QUERIES.currency, {}, null, false);
    const currency = data.shop?.currencyCode;
    if (!/^[A-Z]{3}$/.test(currency || '')) fail('CURRENCY_UNVERIFIED', '无法核实 Shopify 店铺币种。');
    if (currency !== product.currency) fail('CURRENCY_MISMATCH', `商品币种与店铺币种 ${currency} 不一致，请修正后重新排队。`);
    return { done: false, checkpoint: { currencyVerified: true, storeCurrency: currency, stage: 'create_product' } };
  }
  if (!checkpoint.remoteId) {
    const data = await run(QUERIES.create, {
      product: { title: draft.title, descriptionHtml: htmlText(draft.description), status: 'DRAFT', tags: draft.tags || [], vendor: product.vendor || '', productType: product.productType || '' },
      media: (product.images || []).map((originalSource) => ({ originalSource, mediaContentType: 'IMAGE', alt: draft.title })),
    }, 'productCreate');
    const variant = data.product?.variants?.nodes?.[0];
    if (!validGid(data.product?.id, 'Product') || !validGid(variant?.id, 'ProductVariant') || !validGid(variant?.inventoryItem?.id, 'InventoryItem')) {
      fail('INVALID_RESPONSE', 'Shopify 未返回完整的商品/规格 ID，请先核对后台，避免重复创建。', { uncertain: true });
    }
    return { done: false, checkpoint: { ...checkpoint, remoteId: data.product.id, variantId: variant.id, inventoryItemId: variant.inventoryItem.id, stage: 'set_price' } };
  }
  if (!validGid(checkpoint.remoteId, 'Product') || !validGid(checkpoint.variantId, 'ProductVariant') || !validGid(checkpoint.inventoryItemId, 'InventoryItem')) {
    fail('INVALID_CHECKPOINT', '商品任务中的远端标识不完整，请检查任务记录。');
  }
  if (!checkpoint.priceApplied) {
    const data = await run(QUERIES.price, {
      productId: checkpoint.remoteId,
      variants: [{ id: checkpoint.variantId, price: String(product.price), inventoryPolicy: 'DENY', inventoryItem: { sku: product.sku || '', tracked: Boolean(product.trackInventory) } }],
    }, 'productVariantsBulkUpdate');
    const variant = data.productVariants?.find((item) => item.id === checkpoint.variantId);
    if (!variant || Number(variant.price) !== Number(product.price)) fail('PRICE_UNCONFIRMED', 'Shopify 未确认目标价格，已创建的商品保留为草稿。', { uncertain: true });
    return { done: false, checkpoint: { ...checkpoint, priceApplied: true, stage: product.trackInventory ? 'read_inventory' : 'finalize' } };
  }
  if (product.trackInventory && !checkpoint.inventoryCompleted) {
    if (!checkpoint.inventoryObserved) {
      const data = await run(QUERIES.inventoryRead, { itemId: checkpoint.inventoryItemId, locationId: product.locationId }, null, false);
      if (data.inventoryItem?.id !== checkpoint.inventoryItemId) fail('INVENTORY_NOT_FOUND', 'Shopify 库存项不存在或不可读取。');
      const level = data.inventoryItem.inventoryLevel;
      const currentQuantity = level?.quantities?.find((entry) => entry.name === 'available')?.quantity;
      if (level && !Number.isInteger(currentQuantity)) fail('INVALID_RESPONSE', '无法核实库存地点的当前可用数量。');
      return { done: false, checkpoint: { ...checkpoint, inventoryObserved: true, inventoryPresent: Boolean(level), currentQuantity: level ? currentQuantity : 0, inventoryMutationKey: crypto.randomUUID(), stage: 'set_inventory' } };
    }
    if (!/^[0-9a-f-]{36}$/i.test(checkpoint.inventoryMutationKey || '')) fail('INVALID_CHECKPOINT', '库存任务缺少持久化幂等标识。');
    if (!checkpoint.inventoryPresent) {
      const data = await run(QUERIES.inventoryActivate, { itemId: checkpoint.inventoryItemId, locationId: product.locationId, quantity: Number(product.quantity), key: checkpoint.inventoryMutationKey }, 'inventoryActivate');
      const quantity = data.inventoryLevel?.quantities?.find((entry) => entry.name === 'available')?.quantity;
      if (quantity !== Number(product.quantity)) fail('INVENTORY_UNCONFIRMED', 'Shopify 未确认目标库存数量，请检查商品库存。', { uncertain: true });
    } else if (checkpoint.currentQuantity !== Number(product.quantity)) {
      const data = await run(QUERIES.inventorySet, {
        input: { name: 'available', reason: 'correction', quantities: [{ inventoryItemId: checkpoint.inventoryItemId, locationId: product.locationId, quantity: Number(product.quantity), changeFromQuantity: checkpoint.currentQuantity }] },
        key: checkpoint.inventoryMutationKey,
      }, 'inventorySetQuantities');
      const change = data.inventoryAdjustmentGroup?.changes?.find((entry) => entry.name === 'available');
      if (change?.quantityAfterChange !== Number(product.quantity)) fail('INVENTORY_UNCONFIRMED', 'Shopify 未确认目标库存数量，请检查商品库存。', { uncertain: true });
    }
    return { done: false, checkpoint: { ...checkpoint, inventoryCompleted: true, stage: 'finalize' } };
  }
  if (action === 'draft') {
    const result = outcome(endpoint, checkpoint, action);
    return { done: true, checkpoint: { ...checkpoint, result }, result };
  }
  if (!checkpoint.active) {
    const data = await run(QUERIES.active, { product: { id: checkpoint.remoteId, status: 'ACTIVE' } }, 'productUpdate');
    if (data.product?.id !== checkpoint.remoteId || data.product?.status !== 'ACTIVE') fail('STATUS_UNCONFIRMED', 'Shopify 未确认商品启用状态。', { uncertain: true });
    return { done: false, checkpoint: { ...checkpoint, active: true, stage: 'publish_channel' } };
  }
  const data = await run(QUERIES.publish, { id: checkpoint.remoteId, publicationId: product.publicationId }, 'publishablePublish');
  if (data.publishable?.publishedOnPublication !== true) fail('PUBLICATION_UNCONFIRMED', 'Shopify 未确认渠道上架，请检查销售渠道。', { uncertain: true });
  const result = outcome(endpoint, { ...checkpoint, publicationId: product.publicationId }, action);
  return { done: true, checkpoint: { ...checkpoint, publicationId: product.publicationId, result }, result };
}
