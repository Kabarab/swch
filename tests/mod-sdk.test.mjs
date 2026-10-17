import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {inspectDirectory,inspectZip,inspectZipFile,makeZip,writeZip,readZipDirectory,packageLimits,safePath,version,validId,validateManifest,sha256} from '../mod-sdk/validation.mjs';
import {main} from '../mod-sdk/cli.mjs';
import {repositoryName,verifyOwner,register,compatibleCatalog,downloadStats,downloadPackage} from '../scripts/mod-catalog.mjs';

test('catalog extensions keep the legacy manifest readable by strict clients',()=>{
  const catalog={schema:1,blocked:[],mods:[{manifest:{id:'future.mod',name:'Future',api:'^1.1.0',native_tools:[],cover_url:'https://example.com/cover.png',future_field:{enabled:true}},sha256:'abc'}]};
  const wire=compatibleCatalog(catalog);
  assert.deepEqual(Object.keys(wire.mods[0].manifest).sort(),['api','id','name']);
  assert.deepEqual(wire.mods[0].manifest_extensions.native_tools,[]);
  assert.equal(wire.mods[0].manifest_extensions.future_field.enabled,true);
  assert.deepEqual(compatibleCatalog(wire),wire);
  assert.equal(catalog.mods[0].manifest.future_field.enabled,true);
});

test('all author templates package and validate without executing their code',()=>{
  for(const template of ['theme','tab','launcher']){const {manifest,files}=inspectDirectory(`mod-sdk/templates/${template}`);const result=inspectZip(makeZip(files));assert.deepEqual(result.manifest,manifest);assert.equal(result.sha256.length,64);}
});
test('game bindings survive packaging and reject ambiguous or malformed identities',()=>{
  const {manifest,files}=inspectDirectory('mod-sdk/templates/tab');
  const games=[{launcher:'hoyoverse',app_id:'genshin',name:'Genshin Impact'},{launcher:'steam',app_id:'2767030'},{launcher:'epic',app_id:'2767030'},{launcher:'mod:alice.launcher:demo',app_id:'game'}];
  const bound={...manifest,games},packed=new Map(files);packed.set('manifest.json',Buffer.from(JSON.stringify(bound)));
  assert.deepEqual(inspectZip(makeZip(packed)).manifest.games,games);
  for(const invalid of [null,{},[null],[{}],[{launcher:'steam',app_id:2767030}],[{launcher:'steam',app_id:' '}],[{launcher:'steam',app_id:' x'}],[{launcher:'steam',app_id:'x\n'}],[{launcher:'STEAM',app_id:'x'}],[{launcher:'steam',app_id:'x',extra:true}],[{launcher:'steam',app_id:'x',name:5}],[games[0],games[0]],Array(101).fill(games[0])])assert.throws(()=>validateManifest({...manifest,games:invalid},files),/game binding/i);
  assert.equal(validateManifest({...manifest,games:[]},files).games.length,0);
});
test('CLI creates an author project with a working release workflow',async()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'swch-sdk-'));
  try{const folder=path.join(temp,'mod');await main(['init',folder,'--template','launcher','--id','alice.launcher']);const {manifest,files}=inspectDirectory(folder);assert.equal(manifest.id,'alice.launcher');assert.ok(files.has('.github/workflows/release.yml'));await main(['pack',folder]);assert.ok(fs.existsSync(path.join(folder,'dist/alice.launcher-1.0.0.zip')));}finally{fs.rmSync(temp,{recursive:true,force:true});}
});
test('optional cover URLs survive ZIP packaging and reject unsafe sources',()=>{
  const {manifest,files}=inspectDirectory('mod-sdk/templates/tab');
  const cover_url='https://raw.githubusercontent.com/alice/mod/main/cover.png';
  const packed=new Map(files);packed.set('manifest.json',Buffer.from(JSON.stringify({...manifest,cover_url})));
  assert.equal(inspectZip(makeZip(packed)).manifest.cover_url,cover_url);
  assert.equal(validateManifest(manifest,files).cover_url,undefined);
  for(const invalid of ['',7,{},'cover.png','http://example.com/cover.png','javascript:alert(1)','data:image/png;base64,AAAA','file:///cover.png','https://user:pass@example.com/cover.png','https://example.com/a b.png','https://example.com/a\n.png','https://example.com/'+'a'.repeat(2048)])assert.throws(()=>validateManifest({...manifest,cover_url:invalid},files),/cover_url/);
});
test('validator rejects portable path hazards, corrupted content, case duplicates and links',()=>{
  for(const id of ['con','aux.mod','trailing.'])assert.equal(validId(id),false);
  for(const p of ['../evil','/absolute','C:/evil','a\\b','con.txt','a/../b','a.','a//b'])assert.throws(()=>safePath(p));
  const {files}=inspectDirectory('mod-sdk/templates/theme');const corrupt=makeZip(files);corrupt[42]^=255;assert.throws(()=>inspectZip(corrupt));
  const duplicate=new Map(files);duplicate.set('MANIFEST.JSON',Buffer.from('{}'));assert.throws(()=>inspectZip(makeZip(duplicate)),/Duplicate/);
  const addon=new Map(files);addon.set('addon.NODE',Buffer.from('native'));assert.throws(()=>inspectZip(makeZip(addon)),/native/i);
  const link=makeZip(files);let central=link.indexOf(Buffer.from([0x50,0x4b,0x01,0x02]));link.writeUInt32LE(0xa1ff0000,central+38);assert.throws(()=>inspectZip(link),/links/);
});
test('streaming ZIP64 packaging validates lazy assets and large manifests',async()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'swch-sdk-stream-'));
  try{
    const directory=path.join(temp,'mod');fs.cpSync('mod-sdk/templates/tab',directory,{recursive:true});
    const manifestFile=path.join(directory,'manifest.json'),manifest=JSON.parse(fs.readFileSync(manifestFile));
    manifest.games=Array.from({length:1200},(_,i)=>({launcher:'steam',app_id:String(i),name:'Game '+i}));fs.writeFileSync(manifestFile,JSON.stringify(manifest));
    fs.writeFileSync(path.join(directory,'helper.exe'),'native');fs.writeFileSync(path.join(directory,'asset.bin'),Buffer.alloc(1024*1024,7));
    const {files}=inspectDirectory(directory);assert.ok(Buffer.isBuffer(files.get('manifest.json')));assert.equal(files.get('asset.bin').size,1024*1024);assert.equal(typeof files.get('helper.exe').path,'string');
    const output=path.join(temp,'mod.zip'),result=await writeZip(files,output),bytes=fs.readFileSync(output);
    assert.equal(result.sha256,sha256(bytes));assert.ok(bytes.includes(Buffer.from([0x50,0x4b,0x06,0x06])));
    assert.deepEqual(inspectZip(bytes).manifest,manifest);assert.deepEqual(await inspectZipFile(output),{manifest,sha256:result.sha256});
    const corrupted=Buffer.from(bytes),entry=readZipDirectory((offset,length)=>bytes.subarray(offset,offset+length),bytes.length).find(e=>e.name==='asset.bin');corrupted[entry.local+30+entry.nameBytes.length+20]^=255;
    const corruptFile=path.join(temp,'corrupt.zip');fs.writeFileSync(corruptFile,corrupted);await assert.rejects(inspectZipFile(corruptFile));
    fs.writeFileSync(path.join(directory,'asset.bin'),'changed');await assert.rejects(writeZip(files,path.join(temp,'failed.zip')),/changed while packaging/);assert.equal(fs.existsSync(path.join(temp,'failed.zip')),false);assert.equal(fs.readdirSync(temp).some(name=>name.endsWith('.tmp')),false);
  }finally{fs.rmSync(temp,{recursive:true,force:true});}
});
test('package limits are configurable and checked against actual streaming output',async()=>{
  const unlimited=packageLimits({});assert.deepEqual(unlimited,{zipBytes:0,expandedBytes:0,files:0});
  assert.deepEqual(packageLimits({SWCH_MOD_MAX_ZIP_BYTES:'5368709120',SWCH_MOD_MAX_EXPANDED_BYTES:'0',SWCH_MOD_MAX_FILES:'70000'}),{zipBytes:5*1024**3,expandedBytes:0,files:70000});
  for(const value of ['-1','1.5','1e9','Infinity','9007199254740992'])assert.throws(()=>packageLimits({SWCH_MOD_MAX_ZIP_BYTES:value}),/SWCH_MOD_MAX_ZIP_BYTES/);
  const {files}=inspectDirectory('mod-sdk/templates/tab',unlimited),bytes=makeZip(files),temp=fs.mkdtempSync(path.join(os.tmpdir(),'swch-sdk-limits-'));
  try{
    const archive=path.join(temp,'input.zip');fs.writeFileSync(archive,bytes);
    for(const limits of [{...unlimited,zipBytes:bytes.length-1},{...unlimited,expandedBytes:1},{...unlimited,files:1}]){
      assert.throws(()=>inspectZip(bytes,limits),/configured limit/);await assert.rejects(inspectZipFile(archive,limits),/configured limit/);await assert.rejects(writeZip(files,path.join(temp,'failed.zip'),limits),/configured limit/);
    }
    for(const limits of [{...unlimited,expandedBytes:1},{...unlimited,files:1}])assert.throws(()=>inspectDirectory('mod-sdk/templates/tab',limits),/configured limit/);
    assert.equal(fs.existsSync(path.join(temp,'failed.zip')),false);assert.equal(fs.readdirSync(temp).some(name=>name.endsWith('.tmp')),false);
  }finally{fs.rmSync(temp,{recursive:true,force:true});}
});
test('catalog downloads stream to disk and enforce actual bytes without trusting Content-Length',async()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'swch-sdk-download-')),originalFetch=globalThis.fetch;
  try{
    globalThis.fetch=async()=>new Response(new ReadableStream({start(controller){controller.enqueue(new Uint8Array([1,2,3]));controller.enqueue(new Uint8Array([4,5,6]));controller.close();}}));
    const output=path.join(temp,'download.zip');await downloadPackage('https://example.test/mod.zip',output,packageLimits({}));assert.deepEqual(fs.readFileSync(output),Buffer.from([1,2,3,4,5,6]));
    await assert.rejects(downloadPackage('https://example.test/mod.zip',path.join(temp,'limited.zip'),{zipBytes:5}),/configured limit/);
  }finally{globalThis.fetch=originalFetch;fs.rmSync(temp,{recursive:true,force:true});}
});
test('declared native tools validate platforms, metadata and streamed file hashes',async()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'swch-sdk-native-'));
  try{
    const directory=path.join(temp,'mod');fs.cpSync('mod-sdk/templates/tab',directory,{recursive:true});
    const binary=Buffer.from('native helper'),tool={id:'helper',platform:'windows-x64',version:'2.0',license:'MIT',files:[{path:'native/helper.exe',sha256:sha256(binary),executable:true}]};
    fs.mkdirSync(path.join(directory,'native'));fs.writeFileSync(path.join(directory,'native/helper.exe'),binary);
    const manifest={...JSON.parse(fs.readFileSync(path.join(directory,'manifest.json'))),api:'^1.1.0',native_tools:[tool],future_metadata:{compatible:true}};fs.writeFileSync(path.join(directory,'manifest.json'),JSON.stringify(manifest));
    const {files}=inspectDirectory(directory),output=path.join(temp,'mod.zip');assert.equal(typeof files.get('native/helper.exe').path,'string');await writeZip(files,output);assert.deepEqual((await inspectZipFile(output)).manifest,manifest);assert.deepEqual(inspectZip(fs.readFileSync(output)).manifest,manifest);
    for(const native_tools of [null,[{...tool,platform:'linux'}],[{...tool,version:' '}],[{...tool,license:''}],[{...tool,files:[]}],[tool,tool],[{...tool,files:[{...tool.files[0],path:'../helper.exe'}]}],[{...tool,files:[{...tool.files[0],path:'missing.exe'}]}],[{...tool,files:[{...tool.files[0],sha256:'invalid'}]}],[{...tool,files:[{...tool.files[0],executable:'yes'}]}]])assert.throws(()=>validateManifest({...manifest,native_tools},files));
    const bad=new Map(files);bad.set('native/helper.exe',Buffer.from('changed'));const badArchive=makeZip(bad);assert.throws(()=>inspectZip(badArchive),/checksum mismatch/);fs.writeFileSync(output,badArchive);await assert.rejects(inspectZipFile(output),/checksum mismatch/);
    fs.writeFileSync(path.join(directory,'native/helper.exe'),'changed');assert.throws(()=>inspectDirectory(directory),/checksum mismatch/);
    fs.writeFileSync(path.join(directory,'addon.node'),'native addon');assert.throws(()=>inspectDirectory(directory),/Native addons/);
  }finally{fs.rmSync(temp,{recursive:true,force:true});}
});
test('ZIP64 handles more than 65535 files without imposing a package file limit',()=>{
  const {manifest,files}=inspectDirectory('mod-sdk/templates/tab'),empty=Buffer.alloc(0);
  for(let i=0;i<65536;i++)files.set('assets/'+i,empty);
  const bytes=makeZip(files),entries=readZipDirectory((offset,length)=>bytes.subarray(offset,offset+length),bytes.length);
  assert.equal(entries.length,files.size);assert.deepEqual(inspectZip(bytes).manifest,manifest);
});
test('ZIP64 metadata preserves file sizes and offsets above 4 GiB using bounded virtual reads',()=>{
  const name=Buffer.from('asset.bin'),expanded=5*1024**3,compressed=expanded+123,local=expanded+456,indexOffset=local+compressed+100;
  const record=Buffer.alloc(46);record.writeUInt32LE(0x02014b50);record.writeUInt16LE(45,6);record.writeUInt16LE(0x800,8);record.writeUInt32LE(0xffffffff,20);record.writeUInt32LE(0xffffffff,24);record.writeUInt16LE(name.length,28);record.writeUInt16LE(28,30);record.writeUInt32LE(0x81a40000,38);record.writeUInt32LE(0xffffffff,42);
  const extra=Buffer.alloc(28);extra.writeUInt16LE(1);extra.writeUInt16LE(24,2);extra.writeBigUInt64LE(BigInt(expanded),4);extra.writeBigUInt64LE(BigInt(compressed),12);extra.writeBigUInt64LE(BigInt(local),20);
  const index=Buffer.concat([record,name,extra]),endOffset=indexOffset+index.length,extended=Buffer.alloc(56),locator=Buffer.alloc(20),end=Buffer.alloc(22);
  extended.writeUInt32LE(0x06064b50);extended.writeBigUInt64LE(44n,4);extended.writeBigUInt64LE(1n,24);extended.writeBigUInt64LE(1n,32);extended.writeBigUInt64LE(BigInt(index.length),40);extended.writeBigUInt64LE(BigInt(indexOffset),48);
  locator.writeUInt32LE(0x07064b50);locator.writeBigUInt64LE(BigInt(endOffset),8);locator.writeUInt32LE(1,16);
  end.writeUInt32LE(0x06054b50);end.writeUInt16LE(0xffff,8);end.writeUInt16LE(0xffff,10);end.writeUInt32LE(0xffffffff,12);end.writeUInt32LE(0xffffffff,16);
  const segments=[[indexOffset,index],[endOffset,extended],[endOffset+56,locator],[endOffset+76,end]],size=endOffset+98,reads=[];
  const read=(offset,length)=>{reads.push(length);const bytes=Buffer.alloc(length);for(const [start,data]of segments){const a=Math.max(offset,start),b=Math.min(offset+length,start+data.length);if(a<b)data.copy(bytes,a-offset,a-start,b-start);}return bytes;};
  const [entry]=readZipDirectory(read,size);assert.equal(entry.expanded,expanded);assert.equal(entry.compressed,compressed);assert.equal(entry.local,local);assert.ok(Math.max(...reads)<=65557);assert.ok(reads.reduce((a,b)=>a+b,0)<70000);
  index.writeBigUInt64LE(BigInt(Number.MAX_SAFE_INTEGER)+1n,46+name.length+4);assert.throws(()=>readZipDirectory(read,size),/integer precision/);
});
const author={id:10,type:'User'},repository={id:100,owner:{id:10,type:'User'},private:false};
test('download statistics sum mod.zip across stable release pages and preserve real zero',async()=>{
  const pages=[],release=(count,extra={})=>({assets:[{name:'mod.zip',download_count:count},{name:'source.zip',download_count:900}],...extra});
  const stats=await downloadStats(async route=>{
    pages.push(route);
    return route.endsWith('page=1')?[release(40),release(3,{draft:true}),release(9,{prerelease:true}),...Array(97).fill({assets:[]})]:[release(2)];
  },'alice/mod');
  assert.equal(stats.downloads,42);assert.equal(pages.length,2);assert.ok(!Number.isNaN(Date.parse(stats.downloads_updated_at)));
  assert.equal((await downloadStats(async()=>[release(0)],'alice/mod')).downloads,0);
  await assert.rejects(downloadStats(async()=>{throw new Error('Rate limited');},'alice/mod'),/Rate limited/);
  for(const count of [-1,'12',null,Number.MAX_SAFE_INTEGER+1])await assert.rejects(downloadStats(async()=>[release(count)],'alice/mod'),/Invalid GitHub download count/);
});
test('GitHub ownership excludes impersonation, organizations and private repositories',()=>{
  assert.equal(repositoryName('https://github.com/alice/mod'),'alice/mod');assert.throws(()=>repositoryName('alice/mod;echo evil'));
  verifyOwner(repository,author);assert.throws(()=>verifyOwner(repository,{id:11,type:'User'}));assert.throws(()=>verifyOwner({...repository,private:true},author));assert.throws(()=>verifyOwner({...repository,owner:{id:10,type:'Organization'}},author));
});
test('catalog binds IDs to immutable repositories and detects modified releases',()=>{
  const catalog={schema:1,mods:[],blocked:[]},entry={manifest:{id:'alice.mod',version:'1.0.0'},repository_id:100,owner_id:10,sha256:'original'};
  register(catalog,entry);assert.throws(()=>register(catalog,{...entry,repository_id:101}),/another repository/);assert.throws(()=>register(catalog,{...entry,owner_id:11}),/another repository/);assert.throws(()=>register(catalog,{...entry,sha256:'replacement'}),/modified/);
  register(catalog,{...entry,manifest:{...entry.manifest,version:'2.0.0'},sha256:'new'});assert.throws(()=>register(catalog,entry),/downgrade/);assert.throws(()=>register(catalog,{...entry,manifest:{...entry.manifest,id:'other'}}),/One mod/);
  catalog.blocked.push('alice.mod');assert.throws(()=>register(catalog,entry),/blocked/);
});
test('catalog history treats IDs as data, including JavaScript property names',()=>{
  const catalog={mods:[],blocked:[],history:{}};const entry={manifest:{id:'constructor',version:'1.0.0'},repository_id:1,owner_id:1,sha256:'hash'};register(catalog,entry);assert.equal(catalog.history.constructor['1.0.0'],'hash');assert.equal(Object['1.0.0'],undefined);
  for(const v of ['01.0.0','1.0.0-alpha..beta','1.0.0-01','1.0.0+build..1'])assert.throws(()=>version(v));
});
