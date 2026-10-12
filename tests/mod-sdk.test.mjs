import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {inspectDirectory,inspectZip,makeZip,safePath,version,validId} from '../mod-sdk/validation.mjs';
import {main} from '../mod-sdk/cli.mjs';
import {repositoryName,verifyOwner,register} from '../scripts/mod-catalog.mjs';

test('all author templates package and validate without executing their code',()=>{
  for(const template of ['theme','tab','launcher']){const {manifest,files}=inspectDirectory(`mod-sdk/templates/${template}`);const result=inspectZip(makeZip(files));assert.deepEqual(result.manifest,manifest);assert.equal(result.sha256.length,64);}
});
test('CLI creates an author project with a working release workflow',()=>{
  const temp=fs.mkdtempSync(path.join(os.tmpdir(),'swch-sdk-'));
  try{const folder=path.join(temp,'mod');main(['init',folder,'--template','launcher','--id','alice.launcher']);const {manifest,files}=inspectDirectory(folder);assert.equal(manifest.id,'alice.launcher');assert.ok(files.has('.github/workflows/release.yml'));main(['pack',folder]);assert.ok(fs.existsSync(path.join(folder,'dist/alice.launcher-1.0.0.zip')));}finally{fs.rmSync(temp,{recursive:true,force:true});}
});
test('validator rejects portable path hazards, corrupted content, case duplicates and links',()=>{
  for(const id of ['con','aux.mod','trailing.'])assert.equal(validId(id),false);
  for(const p of ['../evil','/absolute','C:/evil','a\\b','con.txt','a/../b','a.','a//b'])assert.throws(()=>safePath(p));
  const {files}=inspectDirectory('mod-sdk/templates/theme');const corrupt=makeZip(files);corrupt[42]^=255;assert.throws(()=>inspectZip(corrupt));
  const duplicate=new Map(files);duplicate.set('MANIFEST.JSON',Buffer.from('{}'));assert.throws(()=>inspectZip(makeZip(duplicate)),/Duplicate/);
  const addon=new Map(files);addon.set('addon.NODE',Buffer.from('native'));assert.throws(()=>inspectZip(makeZip(addon)),/native/);
  const link=makeZip(files);let central=link.indexOf(Buffer.from([0x50,0x4b,0x01,0x02]));link.writeUInt32LE(0xa1ff0000,central+38);assert.throws(()=>inspectZip(link),/links/);
});
const author={id:10,type:'User'},repository={id:100,owner:{id:10,type:'User'},private:false};
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
