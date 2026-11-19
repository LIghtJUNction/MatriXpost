import { readFile,readdir,access } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root=fileURLToPath(new URL('..',import.meta.url));
async function files(dir) {
  const all=[];for(const entry of await readdir(dir,{withFileTypes:true})) {
    if(entry.name==='node_modules'||entry.name==='test-results'||entry.name==='dist') continue;
    const f=path.join(dir,entry.name);if(entry.isDirectory()) all.push(...await files(f));else all.push(f);
  }return all;
}
let count=0;
for(const file of await files(root)) if(/\.(mjs|js)$/.test(file)) {
  const result=spawnSync(process.execPath,['--check',file],{encoding:'utf8'});
  if(result.status) {process.stderr.write(result.stderr);process.exit(1);}count++;
}
const manifest=JSON.parse(await readFile(path.join(root,'extension/manifest.json'),'utf8'));
const pkg=JSON.parse(await readFile(path.join(root,'package.json'),'utf8'));
if(manifest.version!==pkg.version||manifest.manifest_version!==3) throw new Error('Manifest version mismatch');
if(manifest.host_permissions?.length||manifest.externally_connectable) throw new Error('Unreviewed extension access');
for(const file of [manifest.background.service_worker,manifest.options_ui.page,...Object.values(manifest.icons),'app.js','app.css']) {
  await access(path.join(root,'extension',file));
}
const html=await readFile(path.join(root,'extension/app.html'),'utf8');
if(/<script(?![^>]*\bsrc=)[^>]*>/i.test(html)||/\son\w+=/i.test(html)) throw new Error('Inline script violates extension CSP');
console.log(`Checked ${count} JavaScript modules, Manifest V3, versions, resources, and CSP.`);
