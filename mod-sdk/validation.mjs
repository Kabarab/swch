import fs from 'node:fs';
import path from 'node:path';
import {createDeflateRaw,createInflateRaw,deflateRawSync,inflateRawSync} from 'node:zlib';
import {createHash,randomUUID} from 'node:crypto';
import {Readable,Transform} from 'node:stream';
import {pipeline} from 'node:stream/promises';
export function packageLimits(environment=process.env){
  const read=name=>{const text=String(environment[name]??'').trim();if(!text)return 0;const value=Number(text);if(!/^\+?\d+$/.test(text)||!Number.isSafeInteger(value))throw new Error('Invalid '+name+': use a nonnegative byte/file count (0 is unlimited)');return value;};
  return {zipBytes:read('SWCH_MOD_MAX_ZIP_BYTES'),expandedBytes:read('SWCH_MOD_MAX_EXPANDED_BYTES'),files:read('SWCH_MOD_MAX_FILES')};
}
function enforceLimit(value,maximum,label){if(maximum&&value>maximum)throw new Error(label+' exceeds configured limit ('+maximum+')');}
function rejectNativeAddon(name){if(name.toLowerCase().endsWith('.node'))throw new Error('Native addons are unsupported');}
export const validId=id=>{if(typeof id!=='string'||!/^[a-z0-9][a-z0-9_.-]{0,79}$/.test(id))return false;try{safePath(id);return true;}catch{return false;}};
export function safePath(name){
  if(!name||name.includes('\\')||name.split('/').some(p=>!p||p==='.'||p==='..'||/[\x00-\x1f<>:"|?*]/.test(p)||/[. ]$/.test(p)||/^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(p)))throw new Error('Invalid package path: '+name);
  return name;
}
export function version(text){const m=/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/.exec(text);if(!m||[m?.[4],m?.[5]].filter(Boolean).some(ids=>ids.split('.').some(id=>!id))||m?.[4]?.split('.').some(id=>/^0\d+$/.test(id)))throw new Error('Invalid version: '+text);return m.slice(1,4).map(Number);}
export function compareVersions(a,b){const x=version(a),y=version(b);return x[0]-y[0]||x[1]-y[1]||x[2]-y[2];}
export function supportsApi(range){
  // SDK v1's documented ranges. Rust remains the authoritative compatibility check.
  return ['^1.0.0','^1.1.0','~1.1.0','1.1.0','>=1.0.0 <2.0.0','>=1.0.0, <2.0.0','>=1.1.0 <2.0.0','>=1.1.0, <2.0.0'].includes(range);
}
export function validateManifest(m,files){
  if(!m||typeof m!=='object'||Array.isArray(m)||!validId(m.id)||typeof m.name!=='string'||!m.name.trim()||m.name.length>120||typeof (m.description??'')!=='string'||(m.description??'').length>2000)throw new Error('Invalid manifest identity');
  version(m.version);if(!supportsApi(m.api))throw new Error('Use an SDK v1 API range (^1.0.0 recommended)');
  if(m.cover_url!=null){
    let url;try{url=new URL(m.cover_url);}catch{}
    if(typeof m.cover_url!=='string'||Buffer.byteLength(m.cover_url,'utf8')>2048||/[\s\p{Cc}]/u.test(m.cover_url)||url?.protocol!=='https:'||!url.hostname||url.username||url.password)throw new Error('Mod cover_url must be an HTTPS URL without credentials (at most 2048 bytes)');
  }
  if(!Array.isArray(m.platforms)||!m.platforms.length||m.platforms.some(p=>!['windows-x64','macos-arm64'].includes(p)))throw new Error('Invalid platforms');
  if(m.games!==undefined){
    const seen=new Set(),length=v=>Buffer.byteLength(v,'utf8');
    if(!Array.isArray(m.games))throw new Error('Invalid game bindings');
    for(const game of m.games){
      if(!game||typeof game!=='object'||Object.keys(game).some(k=>!['launcher','app_id','name'].includes(k))||typeof game.launcher!=='string'||!game.launcher||length(game.launcher)>256||!/^[a-z0-9_.:-]+$/.test(game.launcher)||typeof game.app_id!=='string'||!game.app_id.trim()||game.app_id!==game.app_id.trim()||length(game.app_id)>256||/[\p{Cc}]/u.test(game.app_id)||(game.name!=null&&(typeof game.name!=='string'||!game.name.trim()||length(game.name)>256)))throw new Error('Invalid game binding');
      const key=JSON.stringify([game.launcher,game.app_id]);if(seen.has(key))throw new Error('Duplicate game binding');seen.add(key);
    }
  }
  if(m.native_tools!==undefined){
    if(!Array.isArray(m.native_tools))throw new Error('Invalid native tools');const seen=new Set();
    if(m.native_tools.length && !['^1.1.0','~1.1.0','1.1.0','>=1.1.0 <2.0.0','>=1.1.0, <2.0.0'].includes(m.api))throw new Error('Native tools require api ^1.1.0');
    for(const tool of m.native_tools){
      if(!tool||typeof tool!=='object'||!validId(tool.id)||!['windows-x64','macos-arm64'].includes(tool.platform)||typeof tool.version!=='string'||!tool.version.trim()||typeof tool.license!=='string'||!tool.license.trim()||!Array.isArray(tool.files)||!tool.files.length)throw new Error('Invalid native tool');
      safePath(tool.version);if(tool.version.includes('/'))throw new Error('Native tool version must be one path component');
      const key=JSON.stringify([tool.id,tool.platform]);if(seen.has(key))throw new Error('Duplicate native tool');seen.add(key);const paths=new Set();
      for(const file of tool.files){
        if(!file||typeof file!=='object'||typeof file.path!=='string'||typeof file.sha256!=='string'||!/^[\da-f]{64}$/i.test(file.sha256)||(file.executable!==undefined&&typeof file.executable!=='boolean'))throw new Error('Invalid native tool file');
        safePath(file.path);rejectNativeAddon(file.path);if(paths.has(file.path.toLowerCase()))throw new Error('Duplicate native tool file');paths.add(file.path.toLowerCase());if(!files.has(file.path))throw new Error('Missing native tool file: '+file.path);
      }
    }
  }
  const entries=['backend','ui','theme'].filter(k=>m[k]!=null);if(!entries.length)throw new Error('Missing entry point');
  for(const key of entries){safePath(m[key]);if(!files.has(m[key]))throw new Error('Missing '+key+' entry point');}
  if(m.theme){const theme=JSON.parse(files.get(m.theme).toString('utf8'));for(const [k,v]of Object.entries(theme))if(!(k==='radius'?/^\d{1,2}px$/.test(v)&&parseInt(v)<=32:['bg','panel','panel-hover','line','text','muted','accent'].includes(k)&&/^#[\da-f]{6}$/i.test(v)))throw new Error('Invalid theme token '+k);}
  return m;
}
export function inspectDirectory(directory,limits=packageLimits()){
  const files=new Map(),seen=new Set(),pending=[[path.resolve(directory),'']];let total=0;
  while(pending.length){const [dir,prefix]=pending.pop();for(const entry of fs.readdirSync(dir,{withFileTypes:true})){
    if(['.git','dist','.DS_Store','.env','.swch-sdk'].includes(entry.name))continue;
    const name=safePath(prefix+entry.name),file=path.join(dir,entry.name),stat=fs.lstatSync(file);
    if(stat.isSymbolicLink()||(!stat.isFile()&&!stat.isDirectory()))throw new Error('Links and special files are unsupported');
    if(seen.has(name.toLowerCase()))throw new Error('Duplicate path');seen.add(name.toLowerCase());
    if(stat.isDirectory()){pending.push([file,name+'/']);continue;}
    rejectNativeAddon(name);total=safeSize(total+stat.size);enforceLimit(total,limits.expandedBytes,'Expanded package');enforceLimit(files.size+1,limits.files,'Package file count');
    files.set(name,{path:file,size:stat.size});
  }}
  if(!files.has('manifest.json'))throw new Error('Missing manifest');
  const manifestBytes=readSource(files.get('manifest.json')),manifest=JSON.parse(manifestBytes);
  files.set('manifest.json',manifestBytes);
  if(typeof manifest.theme==='string'&&files.has(manifest.theme))files.set(manifest.theme,readSource(files.get(manifest.theme)));
  validateManifest(manifest,files);verifyNativeTools(manifest,file=>sourceDigest(files.get(file)));return {manifest,files};
}
const crcTable=Array.from({length:256},(_,n)=>{for(let i=0;i<8;i++)n=n&1?0xedb88320^(n>>>1):n>>>1;return n>>>0;});
function updateCrc(n,data){for(const byte of data)n=crcTable[(n^byte)&255]^(n>>>8);return n;}
function crc(data){return (updateCrc(0xffffffff,data)^0xffffffff)>>>0;}
export const sha256=bytes=>createHash('sha256').update(bytes).digest('hex');
const ZIP32=0xffffffff,ZIP16=0xffff;
function safeSize(value){if(!Number.isSafeInteger(value)||value<0)throw new Error('ZIP value exceeds supported integer precision');return value;}
function uint64(bytes,offset){const value=bytes.readBigUInt64LE(offset);if(value>BigInt(Number.MAX_SAFE_INTEGER))throw new Error('ZIP value exceeds supported integer precision');return Number(value);}
function sourceSize(source){return safeSize(Buffer.isBuffer(source)?source.length:source.size);}
function readSource(source){return Buffer.isBuffer(source)?source:fs.readFileSync(source.path);}
function sourceDigest(source){
  if(Buffer.isBuffer(source))return sha256(source);const descriptor=fs.openSync(source.path,'r'),hash=createHash('sha256'),bytes=Buffer.alloc(1024*1024);
  try{let size;while((size=fs.readSync(descriptor,bytes,0,bytes.length,null)))hash.update(bytes.subarray(0,size));return hash.digest('hex');}finally{fs.closeSync(descriptor);}
}
function nativeHashes(manifest){const hashes=new Map();for(const tool of manifest.native_tools||[])for(const file of tool.files){const expected=file.sha256.toLowerCase();if(hashes.has(file.path)&&hashes.get(file.path)!==expected)throw new Error('Conflicting native tool hashes: '+file.path);hashes.set(file.path,expected);}return hashes;}
function verifyNativeTools(manifest,digest){for(const [file,expected]of nativeHashes(manifest))if(digest(file)!==expected)throw new Error('Native tool checksum mismatch: '+file);}
function zipName(file){const name=Buffer.from(safePath(file));if(name.length>ZIP16)throw new Error('ZIP path exceeds format length');return name;}
function zip64Extra(values){const extra=Buffer.alloc(4+8*values.length);extra.writeUInt16LE(1);extra.writeUInt16LE(8*values.length,2);values.forEach((v,i)=>extra.writeBigUInt64LE(BigInt(safeSize(v)),4+i*8));return extra;}
function localHeader(name,{sum=0,compressed=0,expanded=0,descriptor=false,zip64=false}){
  const extra=zip64?zip64Extra([expanded,compressed]):Buffer.alloc(0),header=Buffer.alloc(30);
  header.writeUInt32LE(0x04034b50);header.writeUInt16LE(zip64?45:20,4);header.writeUInt16LE(descriptor?0x808:0x800,6);header.writeUInt16LE(8,8);header.writeUInt32LE(sum,14);header.writeUInt32LE(zip64?ZIP32:compressed,18);header.writeUInt32LE(zip64?ZIP32:expanded,22);header.writeUInt16LE(name.length,26);header.writeUInt16LE(extra.length,28);
  return [header,name,extra];
}
function centralHeader(name,{sum,compressed,expanded,offset,descriptor=false,zip64=false}){
  zip64 ||= compressed>=ZIP32||expanded>=ZIP32||offset>=ZIP32;
  const extra=zip64?zip64Extra([expanded,compressed,offset]):Buffer.alloc(0),record=Buffer.alloc(46);
  record.writeUInt32LE(0x02014b50);record.writeUInt16LE(0x032d,4);record.writeUInt16LE(zip64?45:20,6);record.writeUInt16LE(descriptor?0x808:0x800,8);record.writeUInt16LE(8,10);record.writeUInt32LE(sum,16);record.writeUInt32LE(zip64?ZIP32:compressed,20);record.writeUInt32LE(zip64?ZIP32:expanded,24);record.writeUInt16LE(name.length,28);record.writeUInt16LE(extra.length,30);record.writeUInt32LE(0x81a40000,38);record.writeUInt32LE(zip64?ZIP32:offset,42);
  return [record,name,extra];
}
function endRecords(count,size,offset,forceZip64=false){
  const end=Buffer.alloc(22),zip64=forceZip64||count>=ZIP16||size>=ZIP32||offset>=ZIP32;
  end.writeUInt32LE(0x06054b50);end.writeUInt16LE(zip64?ZIP16:count,8);end.writeUInt16LE(zip64?ZIP16:count,10);end.writeUInt32LE(zip64?ZIP32:size,12);end.writeUInt32LE(zip64?ZIP32:offset,16);
  if(!zip64)return [end];
  const extended=Buffer.alloc(56),locator=Buffer.alloc(20);
  extended.writeUInt32LE(0x06064b50);extended.writeBigUInt64LE(44n,4);extended.writeUInt16LE(45,12);extended.writeUInt16LE(45,14);extended.writeBigUInt64LE(BigInt(count),24);extended.writeBigUInt64LE(BigInt(count),32);extended.writeBigUInt64LE(BigInt(size),40);extended.writeBigUInt64LE(BigInt(offset),48);
  locator.writeUInt32LE(0x07064b50);locator.writeBigUInt64LE(BigInt(safeSize(offset+size)),8);locator.writeUInt32LE(1,16);
  return [extended,locator,end];
}
// In-memory compatibility helper. The CLI uses writeZip so assets need not fit in RAM.
export function makeZip(files,limits=packageLimits()){
  const locals=[],central=[];let offset=0,totalExpanded=0;enforceLimit(files.size,limits.files,'Package file count');
  for(const [file,source]of [...files].sort(([a],[b])=>a.localeCompare(b))){
    const name=zipName(file),bytes=readSource(source),compressed=deflateRawSync(bytes),metadata={sum:crc(bytes),compressed:compressed.length,expanded:bytes.length,offset,zip64:compressed.length>=ZIP32||bytes.length>=ZIP32},header=localHeader(name,metadata);
    totalExpanded=safeSize(totalExpanded+bytes.length);enforceLimit(totalExpanded,limits.expandedBytes,'Expanded package');
    locals.push(...header,compressed);central.push(...centralHeader(name,metadata));offset=safeSize(offset+header.reduce((n,b)=>n+b.length,0)+compressed.length);
  }
  const index=Buffer.concat(central),end=endRecords(files.size,index.length,offset);enforceLimit(safeSize(offset+index.length+end.reduce((n,b)=>n+b.length,0)),limits.zipBytes,'ZIP package');return Buffer.concat([...locals,index,...end]);
}
export async function writeZip(files,output,limits=packageLimits()){
  output=path.resolve(output);fs.mkdirSync(path.dirname(output),{recursive:true});
  enforceLimit(files.size,limits.files,'Package file count');let totalExpanded=0;for(const [file,source]of files){rejectNativeAddon(file);totalExpanded=safeSize(totalExpanded+sourceSize(source));enforceLimit(totalExpanded,limits.expandedBytes,'Expanded package');}
  const temporary=output+'.'+randomUUID()+'.tmp',handle=await fs.promises.open(temporary,'wx'),hash=createHash('sha256'),central=[];let offset=0,actualExpanded=0;
  const write=async bytes=>{enforceLimit(safeSize(offset+bytes.length),limits.zipBytes,'ZIP package');let position=0;while(position<bytes.length){const {bytesWritten}=await handle.write(bytes,position,bytes.length-position);if(!bytesWritten)throw new Error('Unable to write ZIP');position+=bytesWritten;}hash.update(bytes);offset=safeSize(offset+bytes.length);};
  try{
    for(const [file,source]of [...files].sort(([a],[b])=>a.localeCompare(b))){
      const name=zipName(file),expanded=sourceSize(source),local=offset;
      for(const bytes of localHeader(name,{expanded,descriptor:true,zip64:true}))await write(bytes);
      let actual=0,sum=0xffffffff,compressed=0;
      const input=Buffer.isBuffer(source)?Readable.from([source]):fs.createReadStream(source.path),meter=new Transform({transform(chunk,encoding,callback){try{actual=safeSize(actual+chunk.length);actualExpanded=safeSize(actualExpanded+chunk.length);enforceLimit(actualExpanded,limits.expandedBytes,'Expanded package');sum=updateCrc(sum,chunk);callback(null,chunk);}catch(error){callback(error);}}}),deflater=createDeflateRaw();
      const completion=pipeline(input,meter,deflater);completion.catch(()=>{});
      try{for await(const chunk of deflater){compressed=safeSize(compressed+chunk.length);await write(chunk);}await completion;}catch(error){deflater.destroy(error);await completion.catch(()=>{});throw error;}
      if(actual!==expanded)throw new Error('File changed while packaging: '+file);
      sum=(sum^0xffffffff)>>>0;
      const descriptor=Buffer.alloc(24);descriptor.writeUInt32LE(0x08074b50);descriptor.writeUInt32LE(sum,4);descriptor.writeBigUInt64LE(BigInt(compressed),8);descriptor.writeBigUInt64LE(BigInt(expanded),16);await write(descriptor);
      central.push(centralHeader(name,{sum,compressed,expanded,offset:local,descriptor:true,zip64:true}));
    }
    const indexOffset=offset;for(const record of central)for(const bytes of record)await write(bytes);
    for(const bytes of endRecords(files.size,offset-indexOffset,indexOffset,true))await write(bytes);
    await handle.close();await fs.promises.rename(temporary,output);return {path:output,sha256:hash.digest('hex')};
  }catch(error){await handle.close().catch(()=>{});await fs.promises.rm(temporary,{force:true}).catch(()=>{});throw error;}
}
function boundedReader(read,size){return(offset,length)=>{safeSize(offset);safeSize(length);if(offset+length>size)throw new Error('Truncated ZIP content');const bytes=read(offset,length);if(bytes.length!==length)throw new Error('Truncated ZIP content');return bytes;};}
function resolveZip64(extra,values){
  const needed=Object.values(values).some(v=>v===ZIP32);let cursor=0,found=false;
  while(cursor+4<=extra.length){const id=extra.readUInt16LE(cursor),length=extra.readUInt16LE(cursor+2);cursor+=4;if(cursor+length>extra.length)throw new Error('Invalid ZIP extra field');
    if(id===1){if(found)throw new Error('Duplicate ZIP64 field');found=true;let at=cursor;for(const key of ['expanded','compressed','local'])if(values[key]===ZIP32){if(at+8>cursor+length)throw new Error('Incomplete ZIP64 field');values[key]=uint64(extra,at);at+=8;}}
    cursor+=length;
  }
  if(cursor!==extra.length||needed&&!found)throw new Error('Invalid ZIP64 extra field');return values;
}
// Positional reads keep large central directories and >4 GiB offsets independent of Buffer limits.
export function readZipDirectory(readAt,size,limits=packageLimits()){
  safeSize(size);enforceLimit(size,limits.zipBytes,'ZIP package');if(size<22)throw new Error('Invalid ZIP size');const read=boundedReader(readAt,size),tailOffset=Math.max(0,size-65557),tail=read(tailOffset,size-tailOffset);let end=tail.length-22;
  while(end>=0&&(tail.readUInt32LE(end)!==0x06054b50||end+22+tail.readUInt16LE(end+20)!==tail.length))end--;
  if(end<0)throw new Error('Invalid ZIP directory');
  const absoluteEnd=tailOffset+end;
  if(tail.readUInt16LE(end+4)||tail.readUInt16LE(end+6)||tail.readUInt16LE(end+8)!==tail.readUInt16LE(end+10))throw new Error('Multidisk ZIP archives are unsupported');
  let count=tail.readUInt16LE(end+10),indexSize=tail.readUInt32LE(end+12),offset=tail.readUInt32LE(end+16),directoryEnd=absoluteEnd;
  if(count===ZIP16||indexSize===ZIP32||offset===ZIP32){
    const locator=read(absoluteEnd-20,20);if(locator.readUInt32LE(0)!==0x07064b50||locator.readUInt32LE(4)||locator.readUInt32LE(16)!==1)throw new Error('Invalid ZIP64 locator');
    directoryEnd=uint64(locator,8);const record=read(directoryEnd,56);if(record.readUInt32LE(0)!==0x06064b50||uint64(record,4)<44||directoryEnd+12+uint64(record,4)>absoluteEnd-20||record.readUInt32LE(16)||record.readUInt32LE(20)||uint64(record,24)!==uint64(record,32))throw new Error('Invalid ZIP64 directory');
    count=uint64(record,32);indexSize=uint64(record,40);offset=uint64(record,48);
  }
  const indexEnd=safeSize(offset+indexSize),entries=[],seen=new Set();let totalExpanded=0;
  if(indexEnd>directoryEnd||count>Math.floor(indexSize/46))throw new Error('Invalid ZIP directory bounds');
  enforceLimit(count,limits.files,'Package file count');
  for(let i=0;i<count;i++){
    if(offset+46>indexEnd)throw new Error('Invalid central record');const record=read(offset,46);if(record.readUInt32LE(0)!==0x02014b50)throw new Error('Invalid central record');
    const flag=record.readUInt16LE(8),method=record.readUInt16LE(10),n=record.readUInt16LE(28),extra=record.readUInt16LE(30),comment=record.readUInt16LE(32),mode=record.readUInt32LE(38)>>>16;
    if(offset+46+n+extra+comment>indexEnd)throw new Error('Truncated central record');
    const nameBytes=read(offset+46,n),raw=nameBytes.toString('utf8'),name=safePath(raw.replace(/\/$/,'')),sum=record.readUInt32LE(16),values=resolveZip64(read(offset+46+n,extra),{compressed:record.readUInt32LE(20),expanded:record.readUInt32LE(24),local:record.readUInt32LE(42)});
    offset+=46+n+extra+comment;
    if(flag&1||![0,8].includes(method)||![0,0x4000,0x8000].includes(mode&0xf000)||record.readUInt16LE(34))throw new Error('Encrypted archives, links and special files are unsupported');
    rejectNativeAddon(name);totalExpanded=safeSize(totalExpanded+values.expanded);enforceLimit(totalExpanded,limits.expandedBytes,'Expanded package');
    if(seen.has(name.toLowerCase()))throw new Error('Duplicate path');seen.add(name.toLowerCase());
    entries.push({name,nameBytes,directory:raw.endsWith('/'),flag,method,sum,...values,indexOffset:indexEnd-indexSize});
  }
  if(offset!==indexEnd)throw new Error('Invalid ZIP directory size');return entries;
}
function entryStart(readAt,size,entry){
  const read=boundedReader(readAt,size),record=read(entry.local,30);if(record.readUInt32LE(0)!==0x04034b50||record.readUInt16LE(6)!==entry.flag||record.readUInt16LE(8)!==entry.method)throw new Error('Invalid local record');
  const n=record.readUInt16LE(26),extra=record.readUInt16LE(28),start=safeSize(entry.local+30+n+extra);
  if(!read(entry.local+30,n).equals(entry.nameBytes))throw new Error('Local and central paths differ');
  if(start+entry.compressed>entry.indexOffset)throw new Error('Truncated content');return start;
}
export function inspectZip(bytes,limits=packageLimits()){
  const read=(offset,length)=>bytes.subarray(offset,offset+length),entries=readZipDirectory(read,bytes.length,limits),files=new Map();
  for(const entry of entries){
    const start=entryStart(read,bytes.length,entry),compressed=bytes.subarray(start,start+entry.compressed),content=entry.method===0?compressed:inflateRawSync(compressed,{maxOutputLength:Math.max(1,entry.expanded)});
    if(content.length!==entry.expanded||crc(content)!==entry.sum)throw new Error('Corrupt ZIP entry');if(!entry.directory)files.set(entry.name,content);
  }
  if(!files.has('manifest.json'))throw new Error('Missing manifest');const manifest=validateManifest(JSON.parse(files.get('manifest.json')),files);verifyNativeTools(manifest,file=>sha256(files.get(file)));return {manifest,sha256:sha256(bytes)};
}
export async function inspectZipFile(file,limits=packageLimits()){
  const descriptor=fs.openSync(file,'r');
  try{
    const size=fs.fstatSync(descriptor).size,read=(offset,length)=>{const bytes=Buffer.alloc(length);let at=0;while(at<length){const n=fs.readSync(descriptor,bytes,at,length-at,offset+at);if(!n)throw new Error('Truncated ZIP content');at+=n;}return bytes;},entries=readZipDirectory(read,size,limits),files=new Map(entries.filter(e=>!e.directory).map(e=>[e.name,e]));
    const verify=async(entry,capture=false,expectedHash)=>{
      const start=entryStart(read,size,entry),source=entry.compressed?fs.createReadStream(file,{fd:descriptor,autoClose:false,start,end:start+entry.compressed-1}):Readable.from([]),chunks=[],hash=expectedHash?createHash('sha256'):null;let expanded=0,sum=0xffffffff;
      const inflater=entry.method===8?createInflateRaw():null,content=inflater||source,completion=inflater?pipeline(source,inflater):Promise.resolve();completion.catch(()=>{});
      try{for await(const chunk of content){expanded=safeSize(expanded+chunk.length);if(expanded>entry.expanded)throw new Error('Corrupt ZIP entry');sum=updateCrc(sum,chunk);hash?.update(chunk);if(capture)chunks.push(chunk);}await completion;}catch(error){content.destroy(error);await completion.catch(()=>{});throw error;}
      if(expanded!==entry.expanded||((sum^0xffffffff)>>>0)!==entry.sum)throw new Error('Corrupt ZIP entry');if(hash&&hash.digest('hex')!==expectedHash)throw new Error('Native tool checksum mismatch: '+entry.name);return capture?Buffer.concat(chunks):undefined;
    };
    const manifestEntry=files.get('manifest.json');if(!manifestEntry)throw new Error('Missing manifest');
    const manifestBytes=await verify(manifestEntry,true),manifest=JSON.parse(manifestBytes);files.set('manifest.json',manifestBytes);
    // Validate structure before trusting native tool metadata; verify the real theme below.
    if(typeof manifest.theme==='string'&&files.has(manifest.theme))files.set(manifest.theme,Buffer.from('{}'));validateManifest(manifest,files);
    const hashes=nativeHashes(manifest);if(hashes.has('manifest.json')&&sha256(manifestBytes)!==hashes.get('manifest.json'))throw new Error('Native tool checksum mismatch: manifest.json');
    for(const entry of entries){if(entry===manifestEntry)continue;const content=await verify(entry,entry.name===manifest.theme,hashes.get(entry.name));if(content)files.set(entry.name,content);}
    validateManifest(manifest,files);const hash=createHash('sha256');for await(const chunk of fs.createReadStream(file,{fd:descriptor,autoClose:false,start:0}))hash.update(chunk);
    return {manifest,sha256:hash.digest('hex')};
  }finally{fs.closeSync(descriptor);}
}
