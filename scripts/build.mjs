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
