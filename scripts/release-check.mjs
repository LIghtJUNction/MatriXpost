import assert from 'node:assert/strict';
import { appendFile, readFile, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { inflateRawSync } from 'node:zlib';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32 } from './icons.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
const pkg = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'));
const version = pkg.version;
assert.match(version, /^\d+\.\d+\.\d+$/, 'Use a Chromium-compatible release version');
const tag = process.argv[2] || `v${version}`;
assert.equal(tag, `v${version}`, 'Tag must match package.json');
const manifest = JSON.parse(await readFile(path.join(root, 'extension/manifest.json'), 'utf8'));
assert.equal(manifest.version, version, 'Source manifest version mismatch');

function readZip(buffer) {
  const end = buffer.length - 22;
  assert.ok(end >= 0, 'ZIP is truncated');
  assert.equal(buffer.readUInt32LE(end), 0x06054b50, 'ZIP directory is missing');
  assert.equal(buffer.readUInt16LE(end + 20), 0, 'Unexpected ZIP comment');
  const count = buffer.readUInt16LE(end + 10);
  const directorySize = buffer.readUInt32LE(end + 12);
  const directoryOffset = buffer.readUInt32LE(end + 16);
  assert.equal(directoryOffset + directorySize, end, 'ZIP directory bounds mismatch');
  const files = new Map();
  let cursor = directoryOffset;
  for (let index = 0; index < count; index++) {
    assert.equal(buffer.readUInt32LE(cursor), 0x02014b50, 'Invalid ZIP entry');
    const method = buffer.readUInt16LE(cursor + 10);
    const crc = buffer.readUInt32LE(cursor + 16);
    const packedSize = buffer.readUInt32LE(cursor + 20);
    const size = buffer.readUInt32LE(cursor + 24);
    const nameLength = buffer.readUInt16LE(cursor + 28);
    const extraLength = buffer.readUInt16LE(cursor + 30);
    const commentLength = buffer.readUInt16LE(cursor + 32);
    const localOffset = buffer.readUInt32LE(cursor + 42);
    const name = buffer.subarray(cursor + 46, cursor + 46 + nameLength).toString('utf8');
    assert.ok(name && !name.startsWith('/') && !name.includes('\\') && !name.split('/').includes('..'), 'Unsafe ZIP path');
    assert.ok(!files.has(name), `Duplicate ZIP entry: ${name}`);
    assert.equal(buffer.readUInt32LE(localOffset), 0x04034b50, 'Invalid local ZIP entry');
    assert.equal(method, 8, 'Expected DEFLATE ZIP entry');
    assert.equal(buffer.readUInt16LE(cursor + 8), 0x0800, 'Unexpected ZIP flags');
    assert.equal(buffer.readUInt16LE(localOffset + 6), 0x0800, 'Local ZIP flags mismatch');
    assert.equal(buffer.readUInt16LE(localOffset + 8), method, 'Local ZIP compression mismatch');
    assert.equal(buffer.readUInt32LE(localOffset + 14), crc, 'Local ZIP CRC mismatch');
    assert.equal(buffer.readUInt32LE(localOffset + 18), packedSize, 'Local ZIP compressed size mismatch');
    assert.equal(buffer.readUInt32LE(localOffset + 22), size, 'Local ZIP size mismatch');
    const localNameLength = buffer.readUInt16LE(localOffset + 26);
    assert.equal(buffer.subarray(localOffset + 30, localOffset + 30 + localNameLength).toString('utf8'), name, 'Local ZIP filename mismatch');
    const start = localOffset + 30 + localNameLength + buffer.readUInt16LE(localOffset + 28);
    assert.ok(start + packedSize <= directoryOffset, 'ZIP file overlaps directory');
    const data = inflateRawSync(buffer.subarray(start, start + packedSize), { maxOutputLength: 16 * 1024 * 1024 });
    assert.equal(data.length, size, `Size mismatch: ${name}`);
    assert.equal(crc32(data), crc, `CRC mismatch: ${name}`);
    files.set(name, data);
    cursor += 46 + nameLength + extraLength + commentLength;
  }
  assert.equal(cursor, end, 'ZIP entry count mismatch');
  return files;
}

const artifacts = [
  { name: `MatriXpost-chromium-${version}.zip`, required: ['manifest.json', 'app.html', 'app.js', 'app.css', 'background.js'], manifest: 'manifest.json' },
  { name: `MatriXpost-ai-bridge-${version}.zip`, required: ['bridge/cli.mjs', 'bridge/server.mjs', 'bridge/mcp.mjs', 'bridge/README.md'] },
  { name: `MatriXpost-${version}.zip`, required: ['INSTALL.md', 'docs/INSTALL.md', 'docs/PLATFORMS.md', 'docs/VALIDATION.md', 'chromium/manifest.json', 'bridge/cli.mjs'], manifest: 'chromium/manifest.json' },
];
const checksums = [];
for (const artifact of artifacts) {
  const buffer = await readFile(path.join(root, 'dist', artifact.name));
  const files = readZip(buffer);
  for (const name of artifact.required) assert.ok(files.has(name), `${artifact.name} is missing ${name}`);
  if (artifact.manifest) {
    const packedManifest = JSON.parse(files.get(artifact.manifest).toString('utf8'));
    assert.equal(packedManifest.version, version, 'Packaged manifest version mismatch');
    assert.equal(packedManifest.manifest_version, 3, 'Expected Manifest V3');
    const prefix = artifact.manifest.slice(0, -'manifest.json'.length);
    for (const name of [packedManifest.background.service_worker, packedManifest.options_ui.page, ...Object.values(packedManifest.icons)]) {
      assert.ok(files.has(prefix + name), `Packaged manifest references a missing file: ${name}`);
    }
  }
  for (const name of files.keys()) assert.ok(!/(^|\/)(\.env[^/]*|node_modules|test-results)(\/|$)|\.(token|log)$/i.test(name), `Private or development file in release: ${name}`);
  checksums.push(`${createHash('sha256').update(buffer).digest('hex')}  ${artifact.name}`);
  console.log(`Verified ${artifact.name}: ${files.size} files, ${buffer.length} bytes`);
}
await writeFile(path.join(root, 'dist/SHA256SUMS'), checksums.join('\n') + '\n');
const repo = 'https://github.com/LIghtJUNction/MatriXpost';
const notes = `# MatriXpost ${version}

这是 Chromium MV3 开发预览版，尚未上架浏览器商店。

## 下载与安装

- 只使用扩展：下载 **MatriXpost-chromium-${version}.zip**，解压后在 \`chrome://extensions\` 启用开发者模式，加载包含 \`manifest.json\` 的文件夹。
- 扩展和可选 AI 桥接一起下载：选择 **MatriXpost-${version}.zip**，扩展目录为 \`chromium/\`。
- 已安装扩展，只需 AI 桥接：选择 **MatriXpost-ai-bridge-${version}.zip**。桥接需要 Node.js 22+。
- **SHA256SUMS** 用于核对三个安装包的下载完整性。

更新时使用原来的扩展目录和浏览器配置，点击扩展管理页的“重新加载”。不要先移除扩展，避免清除本地草稿、凭证、素材和任务。

[完整安装与更新步骤](${repo}/blob/${tag}/docs/INSTALL.md) · [实际验收记录](${repo}/blob/${tag}/docs/VALIDATION.md) · [逐平台能力](${repo}/blob/${tag}/docs/PLATFORMS.md)

## 当前能力

YouTube、WooCommerce、Shopify 已实现官方 API 请求流程；其余平台提供后台入口和辅助操作。真实发布能力以逐平台验收记录为准。定时任务需要浏览器运行，浏览器重启后需要重新解锁保险箱。

工作流检查源码、核心/协议测试、安装包结构、版本和校验和。真实 Chromium 权限窗口、界面与平台账号验收单独记录；各平台的授权和发布状态见实际验收记录。
`;
await writeFile(path.join(root, 'dist/RELEASE_NOTES.md'), notes);
if (process.env.GITHUB_OUTPUT) await appendFile(process.env.GITHUB_OUTPUT, `version=${version}\ntag=${tag}\n`);
console.log('Wrote dist/SHA256SUMS and dist/RELEASE_NOTES.md');

