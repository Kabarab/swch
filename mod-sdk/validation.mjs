import fs from 'node:fs';
import path from 'node:path';
import {deflateRawSync,inflateRawSync} from 'node:zlib';
import {createHash} from 'node:crypto';
export const MAX_PACKAGE=50*1024*1024,MAX_EXPANDED=200*1024*1024;
export const validId=id=>{if(typeof id!=='string'||!/^[a-z0-9][a-z0-9_.-]{0,79}$/.test(id))return false;try{safePath(id);return true;}catch{return false;}};
export function safePath(name){
  if(!name||name.includes('\\')||name.split('/').some(p=>!p||p==='.'||p==='..'||/[\x00-\x1f<>:"|?*]/.test(p)||/[. ]$/.test(p)||/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p)))throw new Error('Invalid package path: '+name);
  return name;
}
export function version(text){const m=/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/.exec(text);if(!m||[m?.[4],m?.[5]].filter(Boolean).some(ids=>ids.split('.').some(id=>!id))||m?.[4]?.split('.').some(id=>/^0\d+$/.test(id)))throw new Error('Invalid version: '+text);return m.slice(1,4).map(Number);}
export function compareVersions(a,b){const x=version(a),y=version(b);return x[0]-y[0]||x[1]-y[1]||x[2]-y[2];}
export function supportsApi(range){
  // SDK v1's documented ranges. Rust remains the authoritative compatibility check.
  return ['^1.0.0','~1.0.0','1.0.0','>=1.0.0 <2.0.0','>=1.0.0, <2.0.0'].includes(range);
}
export function validateManifest(m,files){
  const allowed=['id','name','version','api','platforms','description','backend','ui','theme','games'];
  if(Object.keys(m).some(k=>!allowed.includes(k))||!validId(m.id)||typeof m.name!=='string'||!m.name.trim()||m.name.length>120||typeof (m.description??'')!=='string'||(m.description??'').length>2000)throw new Error('Invalid manifest identity');
  version(m.version);if(!supportsApi(m.api))throw new Error('Use an SDK v1 API range (^1.0.0 recommended)');
  if(!Array.isArray(m.platforms)||!m.platforms.length||m.platforms.some(p=>!['windows-x64','macos-arm64'].includes(p)))throw new Error('Invalid platforms');
  if(m.games!==undefined){
    const seen=new Set(),length=v=>Buffer.byteLength(v,'utf8');
    if(!Array.isArray(m.games)||m.games.length>100)throw new Error('Invalid game bindings');
    for(const game of m.games){
      if(!game||typeof game!=='object'||Object.keys(game).some(k=>!['launcher','app_id','name'].includes(k))||typeof game.launcher!=='string'||!game.launcher||length(game.launcher)>256||!/^[a-z0-9_.:-]+$/.test(game.launcher)||typeof game.app_id!=='string'||!game.app_id.trim()||game.app_id!==game.app_id.trim()||length(game.app_id)>256||/[\p{Cc}]/u.test(game.app_id)||(game.name!=null&&(typeof game.name!=='string'||!game.name.trim()||length(game.name)>256)))throw new Error('Invalid game binding');
      const key=JSON.stringify([game.launcher,game.app_id]);if(seen.has(key))throw new Error('Duplicate game binding');seen.add(key);
    }
  }
  const entries=['backend','ui','theme'].filter(k=>m[k]!=null);if(!entries.length)throw new Error('Missing entry point');
  for(const key of entries){safePath(m[key]);if(!files.has(m[key]))throw new Error('Missing '+key+' entry point');}
  if(m.theme){const theme=JSON.parse(files.get(m.theme).toString('utf8'));for(const [k,v]of Object.entries(theme))if(!(k==='radius'?/^\d{1,2}px$/.test(v)&&parseInt(v)<=32:['bg','panel','panel-hover','line','text','muted','accent'].includes(k)&&/^#[\da-f]{6}$/i.test(v)))throw new Error('Invalid theme token '+k);}
  return m;
}
export function inspectDirectory(directory){
  const files=new Map(),seen=new Set();let total=0;
  function walk(dir,prefix=''){for(const entry of fs.readdirSync(dir,{withFileTypes:true})){
    if(['.git','dist','.DS_Store','.env','.swch-sdk'].includes(entry.name))continue;
    const name=safePath(prefix+entry.name),file=path.join(dir,entry.name),stat=fs.lstatSync(file);
    if(stat.isSymbolicLink()||(!stat.isFile()&&!stat.isDirectory()))throw new Error('Links and special files are unsupported');
    if(seen.has(name.toLowerCase()))throw new Error('Duplicate path');seen.add(name.toLowerCase());
    if(stat.isDirectory()){walk(file,name+'/');continue;}
    if(name.toLowerCase().endsWith('.node'))throw new Error('Native addons are unsupported');
    total+=stat.size;if(total>MAX_EXPANDED||files.size>=10000)throw new Error('Package exceeds limits');files.set(name,fs.readFileSync(file));
  }}
  walk(directory);if(!files.has('manifest.json')||files.get('manifest.json').length>16384)throw new Error('Missing or oversized manifest');
  return {manifest:validateManifest(JSON.parse(files.get('manifest.json')),files),files};
}
const crcTable=Array.from({length:256},(_,n)=>{for(let i=0;i<8;i++)n=n&1?0xedb88320^(n>>>1):n>>>1;return n>>>0;});
function crc(data){let n=0xffffffff;for(const byte of data)n=crcTable[(n^byte)&255]^(n>>>8);return (n^0xffffffff)>>>0;}
export const sha256=bytes=>createHash('sha256').update(bytes).digest('hex');
export function makeZip(files){
  const locals=[],central=[];let offset=0;
  for(const [file,bytes]of [...files].sort(([a],[b])=>a.localeCompare(b))){
    const name=Buffer.from(file),compressed=deflateRawSync(bytes),sum=crc(bytes);
    const header=Buffer.alloc(30);header.writeUInt32LE(0x04034b50);header.writeUInt16LE(20,4);header.writeUInt16LE(0x800,6);header.writeUInt16LE(8,8);header.writeUInt32LE(sum,14);header.writeUInt32LE(compressed.length,18);header.writeUInt32LE(bytes.length,22);header.writeUInt16LE(name.length,26);
    locals.push(header,name,compressed);
    const record=Buffer.alloc(46);record.writeUInt32LE(0x02014b50);record.writeUInt16LE(0x0314,4);record.writeUInt16LE(20,6);record.writeUInt16LE(0x800,8);record.writeUInt16LE(8,10);record.writeUInt32LE(sum,16);record.writeUInt32LE(compressed.length,20);record.writeUInt32LE(bytes.length,24);record.writeUInt16LE(name.length,28);record.writeUInt32LE(0x81a40000,38);record.writeUInt32LE(offset,42);central.push(record,name);offset+=header.length+name.length+compressed.length;
  }
  const end=Buffer.alloc(22),index=Buffer.concat(central);end.writeUInt32LE(0x06054b50);end.writeUInt16LE(files.size,8);end.writeUInt16LE(files.size,10);end.writeUInt32LE(index.length,12);end.writeUInt32LE(offset,16);
  const archive=Buffer.concat([...locals,index,end]);if(archive.length>MAX_PACKAGE)throw new Error('Compressed package too large');return archive;
}
export function inspectZip(bytes){
  if(bytes.length>MAX_PACKAGE||bytes.length<22)throw new Error('Invalid ZIP size');
  let end=bytes.length-22;while(end>=Math.max(0,bytes.length-65557)&&bytes.readUInt32LE(end)!==0x06054b50)end--;
  if(end<0||bytes.readUInt32LE(end)!==0x06054b50)throw new Error('Invalid ZIP directory');
  const count=bytes.readUInt16LE(end+10),size=bytes.readUInt32LE(end+12);let offset=bytes.readUInt32LE(end+16);
  if(count>10000||offset+size> end||bytes.readUInt16LE(end+4)||bytes.readUInt16LE(end+6))throw new Error('Unsupported ZIP directory');
  const files=new Map(),seen=new Set();let total=0;
  for(let i=0;i<count;i++){
    if(offset+46>bytes.length||bytes.readUInt32LE(offset)!==0x02014b50)throw new Error('Invalid central record');
    const flag=bytes.readUInt16LE(offset+8),method=bytes.readUInt16LE(offset+10),compressed=bytes.readUInt32LE(offset+20),expanded=bytes.readUInt32LE(offset+24),n=bytes.readUInt16LE(offset+28),extra=bytes.readUInt16LE(offset+30),comment=bytes.readUInt16LE(offset+32),mode=bytes.readUInt32LE(offset+38)>>>16,local=bytes.readUInt32LE(offset+42);
    const raw=bytes.subarray(offset+46,offset+46+n).toString('utf8'),name=safePath(raw.replace(/\/$/,'')),sum=bytes.readUInt32LE(offset+16);offset+=46+n+extra+comment;
    if(flag&1||![0,8].includes(method)||[0xa000,0x2000,0x6000,0x1000,0xc000].includes(mode&0xf000)||name.toLowerCase().endsWith('.node'))throw new Error('Encrypted archives, links and native addons are unsupported');
    if(seen.has(name.toLowerCase()))throw new Error('Duplicate path');seen.add(name.toLowerCase());
    total+=expanded;if(total>MAX_EXPANDED)throw new Error('Expanded package too large');
    if(local+30>bytes.length||bytes.readUInt32LE(local)!==0x04034b50)throw new Error('Invalid local record');
    const localNameLength=bytes.readUInt16LE(local+26);
    if(bytes.subarray(local+30,local+30+localNameLength).toString('utf8')!==raw)throw new Error('Local and central paths differ');
    const start=local+30+bytes.readUInt16LE(local+26)+bytes.readUInt16LE(local+28);if(start+compressed>bytes.length)throw new Error('Truncated content');
    const content=method===0?bytes.subarray(start,start+compressed):inflateRawSync(bytes.subarray(start,start+compressed),{maxOutputLength:Math.max(1,expanded)});
    if(content.length!==expanded||crc(content)!==sum)throw new Error('Corrupt ZIP entry');
    if(!raw.endsWith('/'))files.set(name,content);
  }
  if(!files.has('manifest.json')||files.get('manifest.json').length>16384)throw new Error('Missing or oversized manifest');
  return {manifest:validateManifest(JSON.parse(files.get('manifest.json')),files),sha256:sha256(bytes)};
}
