import fs from 'node:fs';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {createHash} from 'node:crypto';
import {tmpdir} from 'node:os';
import {fileURLToPath} from 'node:url';
const lock=JSON.parse(fs.readFileSync(fileURLToPath(new URL('../mod-sdk/runtime-lock.json',import.meta.url))));
const [target,destination]=process.argv.slice(2),spec=lock[target];
if(!spec||!destination)throw new Error('Usage: bundle-mod-runtime.mjs windows-x64|macos-arm64 <runtime-directory>');
const response=await fetch(spec.archive,{signal:AbortSignal.timeout(120000)});if(!response.ok)throw new Error('Node.js download failed');
const bytes=Buffer.from(await response.arrayBuffer());if(createHash('sha256').update(bytes).digest('hex')!==spec.sha256)throw new Error('Node.js checksum mismatch');
fs.mkdirSync(destination,{recursive:true});
const temp=fs.mkdtempSync(path.join(tmpdir(),'swch-node-'));
try{
  const archive=path.join(temp,spec.archive.endsWith('.zip')?'node.zip':'node.tar.gz');fs.writeFileSync(archive,bytes);
  const result=spawnSync('tar',['-xf',archive,'-C',temp,spec.entry,spec.license],{stdio:'inherit'});if(result.status!==0)throw new Error('Node.js extraction failed');
  const executable=path.join(destination,target==='windows-x64'?'node.exe':'node');fs.copyFileSync(path.join(temp,spec.entry),executable);fs.copyFileSync(path.join(temp,spec.license),path.join(destination,'LICENSE'));
  if(target!=='windows-x64')fs.chmodSync(executable,0o755);
}finally{fs.rmSync(temp,{recursive:true,force:true});}
console.log(`Bundled verified Node.js ${lock.version} for ${target}`);
