import test from 'node:test';
import assert from 'node:assert/strict';
import { executeStep, validateTarget, requiredOrigins, canResumeAfterInterruption, promoteDraftCheckpoint } from '../extension/adapters/index.js';
import { publicHttps, AdapterError } from '../extension/adapters/common.js';
import { YOUTUBE_CHUNK_SIZE } from '../extension/adapters/youtube.js';
import { PLATFORMS } from '../extension/core/catalog.js';

const product = (overrides = {}) => ({
  id: 'draft-product', revision: 1, kind: 'product', title: 'Test product', description: '<script>never HTML</script>', tags: ['tag'], assetIds: [],
  product: { price: '12.90', currency: 'USD', sku: 'SKU-1', quantity: 0, images: ['https://cdn.example.com/image.jpg'], ...overrides },
});
const video = (overrides = {}) => ({
  id: 'draft-video', revision: 1, kind: 'video', title: 'Test video', description: 'description', tags: ['tag'], assetIds: ['asset-1'],
  video: { privacy: 'private', categoryId: '22', madeForKids: false }, ...overrides,
});
const woo = { id: 'woo-account', platform: 'woocommerce', endpoint: 'https://shop.example.com/store', label: 'Shop' };
const shopify = { id: 'shopify-account', platform: 'shopify', endpoint: 'https://matrixpost-fixture.myshopify.com', label: 'Shop' };
const youtube = { id: 'youtube-account', platform: 'youtube', label: 'Channel' };
const json = (body, status = 200, headers = {}) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json', ...headers } });
function fixture(responses) {
  const calls = [];
  const fetchImpl = async (url, options) => {
    calls.push({ url, options, body: typeof options.body === 'string' ? JSON.parse(options.body) : options.body });
    if (!responses.length) throw new Error('Unexpected request');
    const next = responses.shift();
    if (next instanceof Error) throw next;
    return typeof next === 'function' ? next(calls.at(-1)) : next;
  };
  return { calls, fetchImpl };
}
async function runToDone(options, maxSteps = 15) {
  let checkpoint = options.checkpoint || {};
  const steps = [];
  for (let index = 0; index < maxSteps; index++) {
    const result = await executeStep({ ...options, checkpoint });
    steps.push(result);
    if (result.done) return { ...result, steps };
    checkpoint = result.checkpoint;
  }
  throw new Error('Too many adapter steps');
}

test('catalog exposes only implemented APIs as direct integrations', () => {
  assert.equal(new Set(PLATFORMS.map((entry) => entry.id)).size, PLATFORMS.length);
  assert.ok(PLATFORMS.length >= 24);
  assert.deepEqual(PLATFORMS.filter((entry) => entry.mode === 'api').map((entry) => entry.id).sort(), ['shopify', 'woocommerce', 'youtube']);
});

test('account permissions never expand to all sites', () => {
  assert.deepEqual(requiredOrigins(woo), ['https://shop.example.com/*']);
  assert.deepEqual(requiredOrigins(shopify), ['https://matrixpost-fixture.myshopify.com/*']);
  assert.deepEqual(requiredOrigins(youtube), ['https://www.googleapis.com/*']);
  assert.deepEqual(requiredOrigins({ platform: 'douyin' }), []);
});

test('reject credential-bearing, local, IP, non-HTTPS and deceptive Shopify destinations', () => {
  for (const endpoint of ['http://shop.example.com', 'https://user:secret@shop.example.com', 'https://127.0.0.1', 'https://0x7f000001', 'https://[::1]', 'https://localhost', 'https://localhost.', 'https://shop.local', 'https://shop.local.', 'https://shop.example.com:8443', 'https://shop.example.com/?token=secret']) {
    assert.throws(() => publicHttps(endpoint), AdapterError, endpoint);
  }
  assert.throws(() => requiredOrigins({ ...shopify, endpoint: 'https://matrix.myshopify.com.evil.example.com' }), /myshopify/);
  assert.throws(() => requiredOrigins({ ...shopify, endpoint: 'https://matrix.myshopify.com/admin' }));
});

test('YouTube validates byte limits, visibility and video asset count before dispatch', () => {
  const bad = video({ description: '汉'.repeat(1667), assetIds: [], video: { privacy: 'guess' } });
  const fields = validateTarget(bad, youtube, 'publish').map((entry) => entry.field);
  assert.ok(fields.includes('description'));
  assert.ok(fields.includes('video.privacy'));
  assert.ok(fields.includes('assetIds'));
  assert.ok(validateTarget(video({ title: '<title>' }), youtube, 'publish').some((error) => error.field === 'title'));
});

test('commerce validation rejects invalid prices, stock and unconfigured Shopify channels', () => {
  for (const price of ['-1', '1e4', 'NaN', '1.999', '001.00']) assert.ok(validateTarget(product({ price }), woo, 'draft').some((error) => error.field === 'product.price'));
  assert.ok(validateTarget(product({ quantity: 0.5 }), woo, 'draft').some((error) => error.field === 'product.quantity'));
  assert.ok(validateTarget(product(), shopify, 'publish').some((error) => error.field === 'product.publicationId'));
  assert.ok(validateTarget(product({ quantity: 4 }), shopify, 'draft').some((error) => error.field === 'product.trackInventory'));
  assert.ok(validateTarget(product({ images: ['http://cdn.example.com/image.jpg'] }), woo, 'draft').some((error) => error.field === 'product.images'));
});

test('assisted platforms never return a fabricated API success', async () => {
  await assert.rejects(() => executeStep({ draft: video(), account: { platform: 'bilibili' }, action: 'publish', fetchImpl: () => assert.fail('No network allowed') }), (error) => error.code === 'ASSISTED_ONLY');
});

test('WooCommerce verifies currency, escapes description and explicitly creates a draft', async () => {
  const net = fixture([json({ value: 'USD' }), json({ id: 123, status: 'draft', permalink: 'https://shop.example.com/product/123' }, 201)]);
  const result = await runToDone({ draft: product(), account: woo, credentials: { consumerKey: 'ck_fixture', consumerSecret: 'cs_fixture' }, action: 'draft', fetchImpl: net.fetchImpl });
  assert.equal(result.result.status, 'draft');
  assert.equal(result.result.remoteId, '123');
  assert.match(net.calls[0].url, /settings\/general\/woocommerce_currency$/);
  assert.match(net.calls[1].url, /\/store\/wp-json\/wc\/v3\/products$/);
  assert.equal(net.calls[1].body.status, 'draft');
  assert.equal(net.calls[1].body.regular_price, '12.90');
  assert.match(net.calls[1].body.description, /^&lt;script&gt;/);
  assert.equal(net.calls[1].options.redirect, 'error');
  assert.equal(net.calls[1].options.credentials, 'omit');
});

test('WooCommerce publishes only when requested and preserves reported pending status', async () => {
  const net = fixture([json({ value: 'USD' }), json({ id: 9, status: 'pending' }, 201)]);
  const result = await runToDone({ draft: product({ quantity: 4 }), account: woo, credentials: { consumerKey: 'key', consumerSecret: 'secret' }, action: 'publish', fetchImpl: net.fetchImpl });
  assert.equal(net.calls[1].body.status, 'publish');
  assert.equal(net.calls[1].body.stock_quantity, 4);
  assert.equal(result.result.status, 'accepted');
  assert.equal(result.result.platformStatus, 'pending');
});

test('currency mismatch stops before a commerce write', async () => {
  const net = fixture([json({ value: 'CNY' })]);
  await assert.rejects(() => runToDone({ draft: product(), account: woo, credentials: { consumerKey: 'key', consumerSecret: 'secret' }, action: 'publish', fetchImpl: net.fetchImpl }), (error) => error.code === 'CURRENCY_MISMATCH');
  assert.equal(net.calls.length, 1);
  assert.equal(net.calls[0].options.method, 'GET');
});

test('unknown WooCommerce creation outcomes are not retriable and do not leak tokens', async () => {
  const net = fixture([new Error('Bearer TOP_SECRET https://secret.example.com')]);
  await assert.rejects(() => executeStep({ draft: product(), account: woo, credentials: { consumerKey: 'TOP_SECRET', consumerSecret: 'secret' }, action: 'publish', checkpoint: { currencyVerified: true }, fetchImpl: net.fetchImpl }), (error) => {
    assert.equal(error.code, 'NETWORK_ERROR'); assert.equal(error.uncertain, true); assert.equal(error.retryable, false);
    assert.equal(error.message.includes('TOP_SECRET'), false); return true;
  });
});

const sessionUrl = 'https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&upload_id=fixture';
test('YouTube initializes private draft upload and checkpoints before sending media', async () => {
  const net = fixture([new Response(null, { status: 200, headers: { Location: sessionUrl } })]);
  const result = await executeStep({ draft: video({ video: { privacy: 'public' } }), account: youtube, credentials: { accessToken: 'token' }, action: 'draft', asset: new Blob(['video'], { type: 'video/mp4' }), fetchImpl: net.fetchImpl });
  assert.equal(result.done, false);
  assert.equal(net.calls.length, 1);
  assert.equal(net.calls[0].body.status.privacyStatus, 'private');
  assert.equal(result.checkpoint.sessionUrl, sessionUrl);
  assert.equal(result.checkpoint.offset, 0);
  assert.equal(canResumeAfterInterruption('youtube', result.checkpoint), true);
  assert.equal(canResumeAfterInterruption('woocommerce', { currencyVerified: true }), false);
});

test('YouTube refuses an upload Location that would steal the OAuth token', async () => {
  const net = fixture([new Response(null, { status: 200, headers: { Location: 'https://www.googleapis.com.evil.example.com/upload/youtube/v3/videos?upload_id=x' } })]);
  await assert.rejects(() => executeStep({ draft: video(), account: youtube, credentials: { accessToken: 'token' }, action: 'publish', asset: new Blob(['video'], { type: 'video/mp4' }), fetchImpl: net.fetchImpl }), (error) => error.code === 'INVALID_UPLOAD_SESSION' && error.uncertain);
  assert.equal(net.calls.length, 1);
});

test('YouTube initialization without a Location is an unknown outcome', async () => {
  const net = fixture([new Response(null, { status: 200 })]);
  await assert.rejects(() => executeStep({ draft: video(), account: youtube, credentials: { accessToken: 'token' }, action: 'publish', asset: new Blob(['video'], { type: 'video/mp4' }), fetchImpl: net.fetchImpl }), (error) => error.code === 'INVALID_UPLOAD_SESSION' && error.uncertain && !error.retryable);
});

test('YouTube queries remote progress before an 8 MiB bounded chunk', async () => {
  const asset = new Blob([new Uint8Array(YOUTUBE_CHUNK_SIZE * 2)], { type: 'video/mp4' });
  const offset = 256 * 1024;
  const net = fixture([new Response(null, { status: 308, headers: { Range: `bytes=0-${offset - 1}` } }), new Response(null, { status: 308, headers: { Range: `bytes=0-${offset + YOUTUBE_CHUNK_SIZE - 1}` } })]);
  const result = await executeStep({ draft: video(), account: youtube, credentials: { accessToken: 'token' }, action: 'publish', asset, checkpoint: { sessionUrl, offset: 0, total: asset.size, mime: asset.type }, fetchImpl: net.fetchImpl });
  assert.equal(net.calls[0].options.headers['Content-Range'], `bytes */${asset.size}`);
  assert.equal(net.calls[1].options.headers['Content-Range'], `bytes ${offset}-${offset + YOUTUBE_CHUNK_SIZE - 1}/${asset.size}`);
  assert.equal(net.calls[1].body.size, YOUTUBE_CHUNK_SIZE);
  assert.equal(result.checkpoint.offset, offset + YOUTUBE_CHUNK_SIZE);
});

test('YouTube resolves a completed remote upload without retransmitting it', async () => {
  const asset = new Blob(['video'], { type: 'video/mp4' });
  const net = fixture([json({ id: 'AbCdEf12345', status: { uploadStatus: 'uploaded', privacyStatus: 'private' } }, 201)]);
  const result = await executeStep({ draft: video(), account: youtube, credentials: { accessToken: 'token' }, action: 'publish', asset, checkpoint: { sessionUrl, offset: 0, total: asset.size, mime: asset.type }, fetchImpl: net.fetchImpl });
  assert.equal(net.calls.length, 1);
  assert.equal(result.done, true);
  assert.equal(result.result.status, 'processing');
  assert.equal(result.result.remoteId, 'AbCdEf12345');
});

test('interrupted YouTube chunk can be resumed in the same session', async () => {
  const asset = new Blob(['video'], { type: 'video/mp4' });
  const net = fixture([new Response(null, { status: 308 }), new Error('connection lost')]);
  await assert.rejects(() => executeStep({ draft: video(), account: youtube, credentials: { accessToken: 'token' }, action: 'publish', asset, checkpoint: { sessionUrl, offset: 0, total: asset.size, mime: asset.type }, fetchImpl: net.fetchImpl }), (error) => error.code === 'UPLOAD_INTERRUPTED' && error.retryable && !error.uncertain);
});

test('YouTube rejected video is never reported as processing or published', async () => {
  const asset = new Blob(['video'], { type: 'video/mp4' });
  const net = fixture([json({ id: 'AbCdEf12345', status: { uploadStatus: 'rejected' } }, 201)]);
  await assert.rejects(() => executeStep({ draft: video(), account: youtube, credentials: { accessToken: 'token' }, action: 'publish', asset, checkpoint: { sessionUrl, total: asset.size, mime: asset.type }, fetchImpl: net.fetchImpl }), (error) => error.code === 'VIDEO_REJECTED');
});

test('completed adapter checkpoints cannot create duplicate posts', async () => {
  const result = { remoteId: '123', status: 'draft' };
  const done = await executeStep({ draft: product(), account: woo, credentials: {}, action: 'draft', checkpoint: { result }, fetchImpl: () => assert.fail('No network') });
  assert.deepEqual(done.result, result);
});

const productId = 'gid://shopify/Product/101';
const variantId = 'gid://shopify/ProductVariant/202';
const itemId = 'gid://shopify/InventoryItem/303';
const publicationId = 'gid://shopify/Publication/404';
const locationId = 'gid://shopify/Location/505';
const shopCurrency = () => json({ data: { shop: { currencyCode: 'USD' } } });
const shopCreated = () => json({ data: { productCreate: { product: { id: productId, status: 'DRAFT', variants: { nodes: [{ id: variantId, inventoryItem: { id: itemId } }] } }, userErrors: [] } } });
const shopPriced = () => json({ data: { productVariantsBulkUpdate: { productVariants: [{ id: variantId, price: '12.90' }], userErrors: [] } } });
const shopActive = () => json({ data: { productUpdate: { product: { id: productId, status: 'ACTIVE' }, userErrors: [] } } });
const shopPublished = (confirmed = true) => json({ data: { publishablePublish: { publishable: { publishedOnPublication: confirmed }, userErrors: [] } } });

test('Shopify draft creates a DRAFT, applies price, and never publishes', async () => {
  const net = fixture([shopCurrency(), shopCreated(), shopPriced()]);
  const result = await runToDone({ draft: product(), account: shopify, credentials: { accessToken: 'token' }, action: 'draft', fetchImpl: net.fetchImpl });
  assert.equal(result.result.status, 'draft');
  assert.equal(net.calls.length, 3);
  assert.equal(net.calls[1].body.variables.product.status, 'DRAFT');
  assert.equal(net.calls[2].body.variables.variants[0].price, '12.90');
  assert.equal(net.calls[2].body.variables.variants[0].inventoryItem.sku, 'SKU-1');
  assert.ok(net.calls.every((call) => call.url.endsWith('/admin/api/2026-10/graphql.json')));
  assert.ok(net.calls.every((call) => !call.body.query.includes('publishablePublish')));
});

test('Shopify publish executes product, price, activate and explicit channel publication in order', async () => {
  const net = fixture([shopCurrency(), shopCreated(), shopPriced(), shopActive(), shopPublished()]);
  const result = await runToDone({ draft: product({ publicationId }), account: shopify, credentials: { accessToken: 'token' }, action: 'publish', fetchImpl: net.fetchImpl });
  assert.equal(result.result.status, 'published');
  assert.equal(result.result.publicationId, publicationId);
  assert.equal(net.calls[4].body.variables.publicationId, publicationId);
  assert.equal(result.steps[1].checkpoint.remoteId, productId);
});

test('Shopify requires explicit publication evidence even after HTTP success', async () => {
  const net = fixture([shopCurrency(), shopCreated(), shopPriced(), shopActive(), shopPublished(false)]);
  await assert.rejects(() => runToDone({ draft: product({ publicationId }), account: shopify, credentials: { accessToken: 'token' }, action: 'publish', fetchImpl: net.fetchImpl }), (error) => error.code === 'PUBLICATION_UNCONFIRMED' && error.uncertain);
});

test('GraphQL user errors stop before activation and do not leak arbitrary platform messages', async () => {
  const net = fixture([shopCurrency(), shopCreated(), json({ data: { productVariantsBulkUpdate: { productVariants: [], userErrors: [{ field: ['variants'], message: 'TOP_SECRET' }] } } })]);
  await assert.rejects(() => runToDone({ draft: product({ publicationId }), account: shopify, credentials: { accessToken: 'TOP_SECRET' }, action: 'publish', fetchImpl: net.fetchImpl }), (error) => error.code === 'PLATFORM_VALIDATION' && !error.message.includes('TOP_SECRET'));
  assert.equal(net.calls.length, 3);
});

test('GraphQL partial creation evidence prevents blindly recreating the product', async () => {
  const net = fixture([shopCurrency(), json({ data: { productCreate: { product: { id: productId }, userErrors: [{ field: ['media'], message: 'media error' }] } } })]);
  await assert.rejects(() => runToDone({ draft: product(), account: shopify, credentials: { accessToken: 'token' }, action: 'draft', fetchImpl: net.fetchImpl }), (error) => error.code === 'PLATFORM_VALIDATION' && error.uncertain);
});

test('Shopify initial stock uses a persisted idempotency key and CAS at an existing location', async () => {
  const net = fixture([
    shopCurrency(), shopCreated(), shopPriced(),
    json({ data: { inventoryItem: { id: itemId, inventoryLevel: { id: 'level-1', quantities: [{ name: 'available', quantity: 0 }] } } } }),
    json({ data: { inventorySetQuantities: { inventoryAdjustmentGroup: { changes: [{ name: 'available', quantityAfterChange: 12 }] }, userErrors: [] } } }),
  ]);
  const result = await runToDone({ draft: product({ trackInventory: true, quantity: 12, locationId }), account: shopify, credentials: { accessToken: 'token' }, action: 'draft', fetchImpl: net.fetchImpl });
  assert.equal(result.result.inventoryApplied, true);
  assert.match(net.calls[4].body.query, /@idempotent\(key: \$key\)/);
  assert.equal(net.calls[4].body.variables.input.quantities[0].changeFromQuantity, 0);
  assert.equal(net.calls[4].body.variables.input.quantities[0].quantity, 12);
  assert.equal(net.calls[4].body.variables.key, result.steps[3].checkpoint.inventoryMutationKey);
});

test('Shopify activates a previously inactive stock location with the intended quantity', async () => {
  const net = fixture([
    shopCurrency(), shopCreated(), shopPriced(),
    json({ data: { inventoryItem: { id: itemId, inventoryLevel: null } } }),
    json({ data: { inventoryActivate: { inventoryLevel: { id: 'level-2', quantities: [{ name: 'available', quantity: 3 }] }, userErrors: [] } } }),
  ]);
  const result = await runToDone({ draft: product({ trackInventory: true, quantity: 3, locationId }), account: shopify, credentials: { accessToken: 'token' }, action: 'draft', fetchImpl: net.fetchImpl });
  assert.equal(result.result.inventoryApplied, true);
  assert.match(net.calls[4].body.query, /inventoryActivate/);
  assert.equal(net.calls[4].body.variables.quantity, 3);
});

test('WooCommerce draft promotion updates the original product without creating another', async () => {
  const credentials = { consumerKey: 'key', consumerSecret: 'secret' };
  const create = fixture([json({ value: 'USD' }), json({ id: 123, status: 'draft' }, 201)]);
  const draftResult = await runToDone({ draft: product(), account: woo, credentials, action: 'draft', fetchImpl: create.fetchImpl });
  const checkpoint = promoteDraftCheckpoint('woocommerce', draftResult.checkpoint);
  assert.equal(draftResult.checkpoint.result.status, 'draft', 'The original history is immutable');
  assert.equal(checkpoint.result, undefined);
  const publish = fixture([json({ id: 123, status: 'publish' })]);
  const result = await runToDone({ draft: product(), account: woo, credentials, action: 'publish', checkpoint, fetchImpl: publish.fetchImpl });
  assert.equal(result.result.remoteId, '123');
  assert.equal(result.result.status, 'published');
  assert.equal(publish.calls.length, 1);
  assert.equal(publish.calls[0].options.method, 'PUT');
  assert.ok(publish.calls[0].url.endsWith('/products/123'));
  assert.deepEqual(publish.calls[0].body, { status: 'publish' });
});

test('Shopify draft promotion skips creation and price writes and publishes the original product', async () => {
  const draft = product({ publicationId });
  const credentials = { accessToken: 'token' };
  const create = fixture([shopCurrency(), shopCreated(), shopPriced()]);
  const draftResult = await runToDone({ draft, account: shopify, credentials, action: 'draft', fetchImpl: create.fetchImpl });
  const checkpoint = promoteDraftCheckpoint('shopify', draftResult.checkpoint);
  const publish = fixture([shopActive(), shopPublished()]);
  const result = await runToDone({ draft, account: shopify, credentials, action: 'publish', checkpoint, fetchImpl: publish.fetchImpl });
  assert.equal(result.result.remoteId, productId);
  assert.equal(result.result.status, 'published');
  assert.equal(publish.calls.length, 2);
  assert.ok(publish.calls.every((call) => !call.body.query.includes('productCreate') && !call.body.query.includes('productVariantsBulkUpdate')));
  assert.equal(publish.calls[0].body.variables.product.id, productId);
  assert.equal(publish.calls[1].body.variables.id, productId);
});

test('YouTube draft promotion changes the same video visibility without uploading and preserves writable status', async () => {
  const draft = video({ video: { privacy: 'public', madeForKids: false } });
  const checkpoint = promoteDraftCheckpoint('youtube', {
    sessionUrl, remoteId: 'AbCdEf12345', total: 5, offset: 5,
    result: { remoteId: 'AbCdEf12345', status: 'processing', privacy: 'private' },
  });
  assert.equal(checkpoint.sessionUrl, undefined, 'Promotion no longer needs the upload capability');
  const net = fixture([
    json({ items: [{ id: 'AbCdEf12345', status: { uploadStatus: 'processed', privacyStatus: 'private', license: 'creativeCommon', embeddable: false, publicStatsViewable: false, containsSyntheticMedia: true } }] }),
    json({ id: 'AbCdEf12345', status: { uploadStatus: 'processed', privacyStatus: 'public' } }),
  ]);
  const result = await runToDone({ draft, account: youtube, credentials: { accessToken: 'token-with-force-ssl' }, action: 'publish', checkpoint, asset: null, fetchImpl: net.fetchImpl });
  assert.equal(result.result.remoteId, 'AbCdEf12345');
  assert.equal(result.result.status, 'published');
  assert.equal(net.calls.length, 2);
  assert.ok(net.calls.every((call) => !call.url.includes('/upload/')));
  assert.equal(net.calls[1].options.method, 'PUT');
  assert.deepEqual(net.calls[1].body, { id: 'AbCdEf12345', status: { license: 'creativeCommon', embeddable: false, publicStatsViewable: false, containsSyntheticMedia: true, privacyStatus: 'public' } });
});

test('promotion refuses unverified or contradictory remote IDs instead of recreating resources', async () => {
  for (const platform of ['youtube', 'woocommerce', 'shopify']) {
    assert.throws(() => promoteDraftCheckpoint(platform, {}), (error) => error.code === 'PROMOTION_UNAVAILABLE');
  }
  assert.throws(() => promoteDraftCheckpoint('woocommerce', { remoteId: '1', result: { remoteId: '2', status: 'draft' } }), (error) => error.code === 'PROMOTION_UNAVAILABLE');
  await assert.rejects(() => executeStep({ draft: product({ publicationId }), account: shopify, credentials: { accessToken: 'token' }, action: 'publish', checkpoint: { promote: true }, fetchImpl: () => assert.fail('Never create') }), (error) => error.code === 'INVALID_CHECKPOINT');
});

test('recovery distinguishes currency reads, unknown creates and fixed Shopify mutations',()=>{
  const ids={currencyVerified:true,remoteId:'gid://shopify/Product/12',
    variantId:'gid://shopify/ProductVariant/34',inventoryItemId:'gid://shopify/InventoryItem/56'};
  assert.equal(canResumeAfterInterruption('woocommerce',{}),true);
  assert.equal(canResumeAfterInterruption('shopify',{}),true);
  assert.equal(canResumeAfterInterruption('woocommerce',{currencyVerified:true,stage:'create_product'}),false);
  assert.equal(canResumeAfterInterruption('shopify',{currencyVerified:true,stage:'create_product'}),false);
  assert.equal(canResumeAfterInterruption('youtube',{}),false);
  assert.equal(canResumeAfterInterruption('shopify',ids),true);
  assert.equal(canResumeAfterInterruption('shopify',{...ids,variantId:undefined}),false);
  assert.equal(canResumeAfterInterruption('shopify',{...ids,currencyVerified:false}),false);
  assert.equal(canResumeAfterInterruption('shopify',{...ids,priceApplied:true,inventoryObserved:true}),false);
  assert.equal(canResumeAfterInterruption('shopify',{...ids,priceApplied:true,inventoryObserved:true,
    inventoryMutationKey:'12345678-1234-1234-1234-123456789abc'}),true);
});

test('resuming a Shopify price step updates the saved variant without another productCreate',async()=>{
  const checkpoint={currencyVerified:true,remoteId:'gid://shopify/Product/12',
    variantId:'gid://shopify/ProductVariant/34',inventoryItemId:'gid://shopify/InventoryItem/56',stage:'set_price'};
  const net=fixture([json({data:{productVariantsBulkUpdate:{
    productVariants:[{id:checkpoint.variantId,price:'12.90'}],userErrors:[]
  }}})]);
  assert.equal(canResumeAfterInterruption('shopify',checkpoint),true);
  const step=await executeStep({draft:product(),account:shopify,credentials:{accessToken:'fixture'},
    action:'draft',checkpoint,fetchImpl:net.fetchImpl});
  assert.equal(step.checkpoint.remoteId,checkpoint.remoteId);
  assert.equal(step.checkpoint.priceApplied,true);
  assert.equal(net.calls.length,1);
  assert.equal(net.calls[0].body.variables.productId,checkpoint.remoteId);
  assert.equal(net.calls[0].body.variables.variants[0].id,checkpoint.variantId);
  assert(!net.calls[0].body.query.includes('productCreate'));
});
