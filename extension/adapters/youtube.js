import { needCredential, request, readJson, fail, AdapterError } from './common.js';

export const YOUTUBE_CHUNK_SIZE = 8 * 1024 * 1024;
const UPLOAD_URL = 'https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status';
const VIDEO_URL = 'https://www.googleapis.com/youtube/v3/videos';

export function youtubeSession(value) {
  let url;
  try { url = new URL(value); } catch { fail('INVALID_UPLOAD_SESSION', '上传会话地址无效，请检查任务记录。'); }
  if (url.protocol !== 'https:' || url.hostname !== 'www.googleapis.com' || url.port || url.username || url.password || url.hash || url.pathname !== '/upload/youtube/v3/videos' || !url.searchParams.has('upload_id')) {
    fail('INVALID_UPLOAD_SESSION', '上传会话不是受信任的 YouTube 地址。');
  }
  return url.href;
}

function confirmedBytes(response, total) {
  const range = response.headers.get('Range');
  if (!range) return 0;
  const match = /^bytes=0-(\d+)$/.exec(range.trim());
  const offset = match ? Number(match[1]) + 1 : NaN;
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > total) fail('INVALID_RESPONSE', '平台返回了无效的上传进度。');
  return offset;
}

async function finish(response, checkpoint, privacy) {
  const data = await readJson(response);
  if (typeof data.id !== 'string' || !/^[A-Za-z0-9_-]{6,64}$/.test(data.id)) fail('INVALID_RESPONSE', '平台未返回可核实的视频 ID，请核对 YouTube Studio。', { uncertain: true });
  if (['failed', 'rejected', 'deleted'].includes(data.status?.uploadStatus)) {
    fail('VIDEO_REJECTED', 'YouTube 已拒绝或移除该视频，请在 YouTube Studio 查看具体原因。');
  }
  const result = {
    remoteId: data.id,
    url: `https://www.youtube.com/watch?v=${encodeURIComponent(data.id)}`,
    status: 'processing',
    platformStatus: data.status?.uploadStatus || 'uploaded',
    privacy: data.status?.privacyStatus || privacy,
  };
  return { done: true, checkpoint: { ...checkpoint, offset: checkpoint.total, remoteId: data.id, result }, result };
}

export async function executeYouTube({ draft, credentials, action, checkpoint = {}, asset, fetchImpl, signal }) {
  if (checkpoint.result) return { done: true, checkpoint, result: checkpoint.result };
  const accessToken = needCredential(credentials, 'accessToken');
  if (checkpoint.promote) {
    if (action !== 'publish' || !/^[A-Za-z0-9_-]{6,64}$/.test(checkpoint.remoteId || '')) fail('INVALID_CHECKPOINT', '已有视频发布任务缺少有效视频 ID。');
    const headers = { Authorization: `Bearer ${accessToken}` };
    const currentResponse = await request(`${VIDEO_URL}?part=status&id=${encodeURIComponent(checkpoint.remoteId)}`, { method: 'GET', headers }, { fetchImpl, signal, mutating: false });
    const currentData = await readJson(currentResponse, { mutating: false });
    const current = currentData.items?.find((item) => item.id === checkpoint.remoteId);
    if (!current?.status) fail('VIDEO_NOT_FOUND', '找不到该私密视频，或当前授权无法读取它。请核对账号与 youtube.force-ssl 授权。');
    if (['failed', 'rejected', 'deleted'].includes(current.status.uploadStatus)) fail('VIDEO_REJECTED', 'YouTube 已拒绝或移除该视频，请在 YouTube Studio 查看原因。');
    const privacy = draft.video?.privacy || 'private';
    // videos.update replaces mutable fields within each requested part. Preserve
    // current writable status fields instead of resetting license or embedding.
    const status = {};
    for (const key of ['embeddable', 'license', 'publicStatsViewable', 'selfDeclaredMadeForKids', 'containsSyntheticMedia']) {
      if (current.status[key] !== undefined) status[key] = current.status[key];
    }
    if (privacy === 'private' && current.status.publishAt) status.publishAt = current.status.publishAt;
    status.privacyStatus = privacy;
    const response = await request(`${VIDEO_URL}?part=status`, {
      method: 'PUT', headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify({ id: checkpoint.remoteId, status }),
    }, { fetchImpl, signal });
    const data = await readJson(response);
    if (data.id !== checkpoint.remoteId || data.status?.privacyStatus !== privacy) fail('VISIBILITY_UNCONFIRMED', 'YouTube 未确认目标视频可见性，请核对 YouTube Studio。', { uncertain: true });
    if (['failed', 'rejected', 'deleted'].includes(data.status.uploadStatus)) fail('VIDEO_REJECTED', 'YouTube 已拒绝或移除该视频，请在 YouTube Studio 查看原因。');
    const result = {
      remoteId: checkpoint.remoteId, url: `https://www.youtube.com/watch?v=${encodeURIComponent(checkpoint.remoteId)}`,
      status: data.status.uploadStatus === 'processed' ? (privacy === 'private' ? 'draft' : 'published') : 'processing',
      platformStatus: data.status.uploadStatus || 'uploaded', privacy,
    };
    return { done: true, checkpoint: { ...checkpoint, result }, result };
  }
  if (!asset || !Number.isSafeInteger(asset.size) || asset.size <= 0 || typeof asset.slice !== 'function') fail('ASSET_REQUIRED', '请选择需要上传的视频文件。');
  const mime = asset.type || 'application/octet-stream';
  if (!/^video\//.test(mime) && mime !== 'application/octet-stream') fail('INVALID_MEDIA', 'YouTube 上传需要视频文件。');
  const privacy = action === 'draft' ? 'private' : (draft.video?.privacy || 'private');
  const headers = { Authorization: `Bearer ${accessToken}` };
  if (!checkpoint.sessionUrl) {
    const response = await request(UPLOAD_URL, {
      method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/json; charset=UTF-8', 'X-Upload-Content-Length': String(asset.size), 'X-Upload-Content-Type': mime },
      body: JSON.stringify({
        snippet: { title: draft.title, description: draft.description || '', tags: draft.tags || [], categoryId: draft.video?.categoryId || '22' },
        status: { privacyStatus: privacy, selfDeclaredMadeForKids: Boolean(draft.video?.madeForKids) },
      }),
    }, { fetchImpl, signal });
    let sessionUrl;
    try { sessionUrl = youtubeSession(response.headers.get('Location')); }
    catch { fail('INVALID_UPLOAD_SESSION', '平台已响应初始化，但没有返回受信任的上传会话，请核对后台后处理。', { uncertain: true }); }
    return { done: false, checkpoint: { sessionUrl, offset: 0, total: asset.size, mime, privacy, stage: 'uploading' } };
  }
  const sessionUrl = youtubeSession(checkpoint.sessionUrl);
  if (checkpoint.total !== asset.size || checkpoint.mime !== mime) fail('ASSET_CHANGED', '视频文件与上传会话不匹配。');
  // Always reconcile with the server first. A previous worker can have ended after
  // a successful upload chunk but before persisting its local checkpoint.
  const probe = await request(sessionUrl, {
    method: 'PUT', headers: { ...headers, 'Content-Range': `bytes */${asset.size}` }, body: new Blob([]),
  }, { fetchImpl, signal, mutating: false, accepted: [308] });
  if (probe.status !== 308) return finish(probe, checkpoint, privacy);
  const offset = confirmedBytes(probe, asset.size);
  if (offset >= asset.size) fail('UPLOAD_CONFIRMATION_PENDING', '所有字节已上传，等待平台返回视频资源。', { retryable: true });
  const end = Math.min(offset + YOUTUBE_CHUNK_SIZE, asset.size);
  let response;
  try {
    response = await request(sessionUrl, {
      method: 'PUT', headers: { ...headers, 'Content-Type': mime, 'Content-Range': `bytes ${offset}-${end - 1}/${asset.size}` }, body: asset.slice(offset, end),
    }, { fetchImpl, signal, accepted: [308] });
  } catch (error) {
    // A resumable session can be queried; retrying the same job never starts a
    // new video. Never use this recovery rule for non-idempotent creation APIs.
    if (error instanceof AdapterError && error.uncertain) {
      throw new AdapterError('UPLOAD_INTERRUPTED', '上传中断；可通过同一会话核对进度并恢复。', { retryable: true });
    }
    throw error;
  }
  if (response.status === 308) {
    const nextOffset = confirmedBytes(response, asset.size);
    if (nextOffset <= offset || nextOffset > end) fail('UPLOAD_PROGRESS_INVALID', '平台未确认本次上传进度，请稍后查询同一会话。', { retryable: true });
    return { done: false, checkpoint: { ...checkpoint, offset: nextOffset, stage: 'uploading' } };
  }
  return finish(response, checkpoint, privacy);
}
