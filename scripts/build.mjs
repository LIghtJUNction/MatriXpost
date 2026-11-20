import { readdir,readFile,mkdir,writeFile,cp,rm } from 'node:fs/promises';
import { deflateRawSync } from 'node:zlib';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32,generateIcons } from './icons.mjs';
const root=fileURLToPath(new URL('..',import.meta.url));
const {version}=JSON.parse(await readFile(path.join(root,'package.json'),'utf8'));
async function entries(dir,prefix='') {
  const out=[];for(const e of (await readdir(dir,{withFileTypes:true})).sort((a,b)=>a.name.localeCompare(b.name))) {
    if(e.name.startsWith('.')) continue;
    const full=path.join(dir,e.name),name=prefix+e.name;
    if(e.isDirectory()) out.push(...await entries(full,name+'/'));else if(e.isFile()) out.push({name,data:await readFile(full)});
  }return out;
}
async function zip(destination,files) {
  const records=[],directory=[];let offset=0;
  for(const f of files) {
    const name=Buffer.from(f.name),data=deflateRawSync(f.data,{level:9}),crc=crc32(f.data);
    const local=Buffer.alloc(30);local.writeUInt32LE(0x04034b50);local.writeUInt16LE(20,4);local.writeUInt16LE(0x0800,6);local.writeUInt16LE(8,8);
    local.writeUInt16LE(0x0021,12);local.writeUInt32LE(crc,14);local.writeUInt32LE(data.length,18);local.writeUInt32LE(f.data.length,22);local.writeUInt16LE(name.length,26);
    const central=Buffer.alloc(46);central.writeUInt32LE(0x02014b50);central.writeUInt16LE(20,4);central.writeUInt16LE(20,6);central.writeUInt16LE(0x0800,8);central.writeUInt16LE(8,10);
    central.writeUInt16LE(0x0021,14);central.writeUInt32LE(crc,16);central.writeUInt32LE(data.length,20);central.writeUInt32LE(f.data.length,24);central.writeUInt16LE(name.length,28);central.writeUInt32LE(offset,42);
    records.push(local,name,data);directory.push(central,name);offset+=local.length+name.length+data.length;
  }
  const central=Buffer.concat(directory),end=Buffer.alloc(22);end.writeUInt32LE(0x06054b50);end.writeUInt16LE(files.length,8);end.writeUInt16LE(files.length,10);end.writeUInt32LE(central.length,12);end.writeUInt32LE(offset,16);
  const archive=Buffer.concat([...records,central,end]);await writeFile(destination,archive);console.log(`${path.basename(destination)}: ${archive.length} bytes, ${files.length} files`);
}
await generateIcons();
const dist=path.join(root,'dist');await mkdir(dist,{recursive:true});
// This path is a reproducible build output; source and browser data are elsewhere.
await rm(path.join(dist,'chromium'),{recursive:true,force:true});
await cp(path.join(root,'extension'),path.join(dist,'chromium'),{recursive:true});
await zip(path.join(dist,`MatriXpost-chromium-${version}.zip`),await entries(path.join(root,'extension')));
await zip(path.join(dist,`MatriXpost-ai-bridge-${version}.zip`),await entries(path.join(root,'bridge'),'bridge/'));
const install=Buffer.from(`# MatriXpost ${version}\n\n## 浏览器扩展\n\n1. 解压整个安装包。\n2. 打开 Chromium 的 chrome://extensions，启用开发者模式。\n3. 点击“加载已解压的扩展程序”，选择本包中的 chromium 文件夹。\n4. 点击 MatriXpost 图标，添加账号并创建草稿。\n\n当前适用于 Chromium 120+；真实浏览器验收使用 Linux Chromium。\n\n## AI 操作\n\n可选桥接需要 Node.js 22+。在扩展管理页复制实际扩展 ID，运行：\n\n\`\`\`sh\nnode bridge/cli.mjs serve --extension-id YOUR_CHROMIUM_EXTENSION_ID\n\`\`\`\n\n在扩展“AI 连接”输入终端显示的本机地址和配对码。完整 MCP 配置见 bridge/README.md。\n\n## 平台与数据\n\nYouTube、WooCommerce、Shopify 已实现官方 API 请求流程，需要自行配置获准授权。其他平台提供后台入口、内容准备和字段辅助；详情见 docs/PLATFORMS.md。真实平台发布验收需要对应账号授权。\n\n账号凭证加密保存在当前浏览器；没有遥测和内置模型调用。普通页面账号使用当前浏览器登录会话。\n\n## 更新扩展\n\n将新文件覆盖到原来的扩展目录，再到扩展管理页点击“重新加载”。保留同一目录与浏览器配置，不要先移除扩展，以保留草稿、素材、凭证和任务。详细步骤与常见问题见 docs/INSTALL.md。\n\n源码与更新：https://github.com/LIghtJUNction/MatriXpost\n`);
const documents=await entries(path.join(root,'docs'),'docs/');
await zip(path.join(dist,`MatriXpost-${version}.zip`),[
  ...await entries(path.join(root,'extension'),'chromium/'),...await entries(path.join(root,'bridge'),'bridge/'),
  {name:'INSTALL.md',data:install},...documents
]);
