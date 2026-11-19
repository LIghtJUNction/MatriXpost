import { deflateSync } from 'node:zlib';
import { mkdir,writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
export function crc32(data) {
  let crc=0xffffffff;
  for(const byte of data) {crc^=byte;for(let j=0;j<8;j++) crc=(crc>>>1)^((crc&1)?0xedb88320:0);}
  return (crc^0xffffffff)>>>0;
}
function chunk(name,data) {
  const type=Buffer.from(name);const length=Buffer.alloc(4),sum=Buffer.alloc(4);
  length.writeUInt32BE(data.length);sum.writeUInt32BE(crc32(Buffer.concat([type,data])));
  return Buffer.concat([length,type,data,sum]);
}
function icon(size) {
  const raw=Buffer.alloc(size*(size*4+1));
  const lines=[[[.25,.7],[.25,.3]],[[.25,.3],[.5,.57]],[[.5,.57],[.75,.3]],[[.75,.3],[.75,.7]]];
  function distance(x,y,a,b) {const dx=b[0]-a[0],dy=b[1]-a[1],t=Math.min(1,Math.max(0,((x-a[0])*dx+(y-a[1])*dy)/(dx*dx+dy*dy)));return Math.hypot(x-(a[0]+t*dx),y-(a[1]+t*dy));}
  for(let y=0;y<size;y++) for(let x=0;x<size;x++) {
    const nx=(x+.5)/size,ny=(y+.5)/size,p=y*(size*4+1)+1+x*4;
    const white=lines.some(([a,b])=>distance(nx,ny,a,b)<.037);
    const accent=Math.hypot(nx-.78,ny-.79)<.04;
    raw.set(white?[245,244,240,255]:accent?[212,137,88,255]:[25,26,28,255],p);
  }
  const header=Buffer.alloc(13);header.writeUInt32BE(size,0);header.writeUInt32BE(size,4);header[8]=8;header[9]=6;
  return Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(raw)),chunk('IEND',Buffer.alloc(0))]);
}
export async function generateIcons() {
  const root=new URL('../extension/icons/',import.meta.url);await mkdir(root,{recursive:true});
  for(const size of [16,48,128]) await writeFile(new URL(`${size}.png`,root),icon(size));
}
if(process.argv[1]===fileURLToPath(import.meta.url)) await generateIcons();
