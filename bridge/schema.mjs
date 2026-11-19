const text = (maxLength, extra = {}) => ({ type: 'string', maxLength, ...extra });
const id = text(128, { minLength: 1, pattern: '^[^\\u0000-\\u001f\\u007f]+$' });
const object = (properties, required = []) => ({ type: 'object', properties, required, additionalProperties: false });
const empty = object({});
const action = { type: 'string', enum: ['draft', 'publish'] };
const accountIds = { type: 'array', items: id, minItems: 1, maxItems: 64, uniqueItems: true };
const target = { draftId: id, accountIds, action };
const draft = object({
  id,
  revision: { type: 'integer', minimum: 1 },
  kind: { type: 'string', enum: ['video', 'product'] },
  title: text(1000, { minLength: 1 }),
  description: text(100000),
  tags: { type: 'array', items: text(100), maxItems: 100 },
  assetIds: { type: 'array', items: id, maxItems: 32, uniqueItems: true },
  video: object({
    privacy: { type: 'string', enum: ['private', 'unlisted', 'public'] },
    categoryId: text(20),
    madeForKids: { type: 'boolean' },
  }),
  product: object({
    price: text(32), currency: text(3, { minLength: 3 }), sku: text(128),
    quantity: { type: 'integer', minimum: 0, maximum: 2147483647 },
    images: { type: 'array', items: text(4096), maxItems: 32 },
    vendor: text(255), productType: text(255), publicationId: text(255),
  }),
}, ['kind', 'title']);

const definition = (name, method, description, inputSchema, readOnly = false) => ({
  name, method, description, inputSchema,
  annotations: { readOnlyHint: readOnly, destructiveHint: !readOnly, idempotentHint: readOnly, openWorldHint: !readOnly },
});

export const TOOL_DEFINITIONS = Object.freeze([
  definition('platforms_list', 'platforms.list', 'List platform capabilities and honest integration status.', empty, true),
  definition('accounts_list', 'accounts.list', 'List account references without credentials. Browser sessions share their browser profile.', empty, true),
  definition('drafts_list', 'drafts.list', 'List local video and product drafts.', empty, true),
  definition('drafts_save', 'drafts.save', 'Create or update a local draft. Existing drafts require their current revision. Media must already be imported in the extension.', object({ draft }, ['draft'])),
  definition('publish_preview', 'publish.preview', 'Validate content and account targets without publishing.', object(target, ['draftId', 'accountIds', 'action']), true),
  definition('jobs_enqueue', 'jobs.enqueue', 'Queue a draft or publish request. Publish requests from AI require review and approval in the extension; enqueuing is not evidence of publication.', object({ ...target, scheduledAt: text(40) }, ['draftId', 'accountIds', 'action'])),
  definition('jobs_list', 'jobs.list', 'Read local job states and confirmed platform results. Inspect this after an uncertain timeout before retrying.', empty, true),
  definition('jobs_retry', 'jobs.retry', 'Request retry of an eligible failed job. The extension checks retry safety and any publication approval.', object({ id }, ['id'])),
  definition('jobs_cancel', 'jobs.cancel', 'Cancel an eligible local job. Cancellation cannot undo a platform publication.', object({ id }, ['id'])),
]);

export class BridgeError extends Error {
  constructor(code, message, status = 400, details = {}) {
    super(message);
    this.name = 'BridgeError';
    this.code = code;
    this.status = status;
    this.uncertain = Boolean(details.uncertain);
  }
}

export function isObject(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype || Object.getPrototypeOf(value) === null);
}

export function validateSchema(value, schema, path = 'arguments', depth = 0) {
  const reject = (message) => { throw new BridgeError('INVALID_ARGUMENT', `${path}: ${message}`); };
  if (depth > 12) reject('object nesting is too deep');
  if (schema.enum && !schema.enum.includes(value)) reject(`must be one of ${schema.enum.join(', ')}`);
  if (schema.type === 'object') {
    if (!isObject(value)) reject('must be an object');
    for (const key of Object.keys(value)) {
      if (!Object.hasOwn(schema.properties, key)) reject('contains an unsupported field');
      validateSchema(value[key], schema.properties[key], `${path}.${key}`, depth + 1);
    }
    for (const key of schema.required ?? []) if (!Object.hasOwn(value, key)) reject(`missing ${key}`);
  } else if (schema.type === 'array') {
    if (!Array.isArray(value)) reject('must be an array');
    if (value.length < (schema.minItems ?? 0) || value.length > (schema.maxItems ?? Infinity)) reject('invalid number of items');
    if (schema.uniqueItems && new Set(value.map((item) => JSON.stringify(item))).size !== value.length) reject('items must be unique');
    value.forEach((item, i) => validateSchema(item, schema.items, `${path}[${i}]`, depth + 1));
  } else if (schema.type === 'string') {
    if (typeof value !== 'string') reject('must be a string');
    if (value.length < (schema.minLength ?? 0) || value.length > (schema.maxLength ?? Infinity)) reject('invalid string length');
    if (schema.pattern && !new RegExp(schema.pattern, 'u').test(value)) reject('contains invalid characters');
  } else if (schema.type === 'integer') {
    if (!Number.isSafeInteger(value) || value < (schema.minimum ?? -Infinity) || value > (schema.maximum ?? Infinity)) reject('must be a valid integer');
  } else if (schema.type === 'boolean' && typeof value !== 'boolean') reject('must be a boolean');
}

export function validateCall(value) {
  if (!isObject(value) || Object.keys(value).some((key) => key !== 'method' && key !== 'params')) {
    throw new BridgeError('INVALID_ARGUMENT', 'Expected an object containing method and params.');
  }
  const tool = TOOL_DEFINITIONS.find((entry) => entry.method === value.method);
  if (!tool) throw new BridgeError('METHOD_NOT_ALLOWED', 'This method is not exposed to AI.', 403);
  const params = value.params ?? {};
  validateSchema(params, tool.inputSchema);
  if (params.scheduledAt !== undefined && !/^\d{4}-\d{2}-\d{2}T/.test(params.scheduledAt)) {
    throw new BridgeError('INVALID_ARGUMENT', 'scheduledAt must be an ISO date-time.');
  }
  if (params.scheduledAt !== undefined && !Number.isFinite(Date.parse(params.scheduledAt))) {
    throw new BridgeError('INVALID_ARGUMENT', 'scheduledAt must be a valid ISO date-time.');
  }
  return { method: tool.method, params };
}

export function publicTools() {
  return TOOL_DEFINITIONS.map(({ method: _method, ...tool }) => tool);
}
