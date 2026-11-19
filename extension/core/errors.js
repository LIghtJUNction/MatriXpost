export class AppError extends Error {
  constructor(code, message, details) {
    super(message);
    this.name = 'AppError';
    this.code = code;
    if (details !== undefined) this.details = details;
  }
}

export function assert(condition, code, message, details) {
  if (!condition) throw new AppError(code, message, details);
}

export function publicError(error) {
  // Provider bodies and request headers must never be reflected into UI or MCP.
  if (error?.code && typeof error.code === 'string') {
    return { code: error.code, message: String(error.message || '操作未完成').slice(0,500),
      ...(error.details ? { details: error.details } : {}) };
  }
  return { code: 'INTERNAL_ERROR', message: '操作未完成，请重试或查看本地诊断。' };
}
