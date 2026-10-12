import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {inspectZip,compareVersions,MAX_PACKAGE} from '../mod-sdk/validation.mjs';

export function repositoryName(input){
  const name=String(input).trim().replace(/^https:\/\/github\.com\//,'').replace(/\/$/,'');
  if(!/^[A-Za-z0-9-]+\/[A-Za-z0-9_.-]+$/.test(name))throw new Error('Repository must be owner/name');return name;
}
export function verifyOwner(repository,author){
  if(repository.private||repository.archived||repository.disabled||repository.owner?.type!=='User'||author.type!=='User'||repository.owner.id!==author.id)throw new Error('Submit from the personal GitHub account that owns this public repository');
}
export function register(catalog,entry){
  const id=entry.manifest.id;
  if(catalog.blocked.includes(id))throw new Error('Mod is blocked');
  const previous=catalog.mods.find(m=>m.manifest.id===id);
  if(previous&&(previous.repository_id!==entry.repository_id||previous.owner_id!==entry.owner_id))throw new Error('Mod ID belongs to another repository or owner');
  if(catalog.mods.some(m=>m.repository_id===entry.repository_id&&m.manifest.id!==id))throw new Error('One mod per repository is supported');
  catalog.history=Object.assign(Object.create(null),catalog.history||{});
  catalog.history[id]=Object.assign(Object.create(null),Object.hasOwn(catalog.history,id)?catalog.history[id]:{});
  const recorded=catalog.history[id][entry.manifest.version]||(previous?.manifest.version===entry.manifest.version?previous.sha256:null);
  if(recorded&&recorded!==entry.sha256)throw new Error('Published version was modified; release a new version');
  if(previous&&compareVersions(entry.manifest.version,previous.manifest.version)<0)throw new Error('Version downgrade rejected');
  catalog.history[id][entry.manifest.version]=entry.sha256;
  catalog.mods=catalog.mods.filter(m=>m.manifest.id!==id);catalog.mods.push(entry);catalog.mods.sort((a,b)=>a.manifest.id.localeCompare(b.manifest.id));return catalog;
}
async function download(url,max,headers={}){
  const response=await fetch(url,{headers,signal:AbortSignal.timeout(120000)});if(!response.ok)throw new Error(`GitHub returned ${response.status}`);
  if(Number(response.headers.get('content-length'))>max)throw new Error('Download too large');
  const chunks=[];let count=0;for await(const chunk of response.body){count+=chunk.length;if(count>max)throw new Error('Download too large');chunks.push(chunk);}return Buffer.concat(chunks);
}
export async function run(){
  const central=repositoryName(process.env.GITHUB_REPOSITORY),token=process.env.GITHUB_TOKEN;
  if(!token)throw new Error('GITHUB_TOKEN is required in the catalog workflow');
  const headers={'Authorization':`Bearer ${token}`,'Accept':'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28'};
  const api=async(route,method='GET',body)=>{
    const response=await fetch('https://api.github.com'+route,{method,headers:{...headers,'Content-Type':'application/json'},body:body?JSON.stringify(body):undefined,signal:AbortSignal.timeout(30000)});
    if(!response.ok){const error=new Error(`GitHub API ${response.status}: ${route}`);error.status=response.status;throw error;}return response.json();
  };
  let current=null,catalog;
  try{current=await api(`/repos/${central}/contents/index.json?ref=mod-catalog`);catalog=JSON.parse(Buffer.from(current.content,'base64'));}
  catch(e){if(e.status!==404)throw e;catalog=JSON.parse(fs.readFileSync('catalog/index.json'));}
  catalog.blocked=JSON.parse(fs.readFileSync('catalog/blocked.json'));let issue;
  const event=process.env.GITHUB_EVENT_PATH?JSON.parse(fs.readFileSync(process.env.GITHUB_EVENT_PATH)):{};
  if(process.env.GITHUB_EVENT_NAME==='issues'){
    if(!event.issue?.title.startsWith('[SWCH Mod]')||event.sender?.id!==event.issue.user.id)return;
    issue=event.issue;
    const match=/### Repository\s+([^\r\n]+)/.exec(issue.body||'');if(!match)throw new Error('Missing Repository field');
    const name=repositoryName(match[1]);
    try{const repo=await api(`/repos/${name}`);verifyOwner(repo,issue.user);await ingest(repo,issue.user);}
    catch(e){console.error(e.message);await report(issue.number,'Validation failed: '+e.message);return;}
  }else{
    for(const entry of [...catalog.mods]){
      if(catalog.blocked.includes(entry.manifest.id))continue;
      try{const repo=await api(`/repositories/${entry.repository_id}`);verifyOwner(repo,{id:entry.owner_id,type:'User'});await ingest(repo,{id:entry.owner_id,type:'User'});}catch(e){console.error(`${entry.manifest.id}: ${e.message}`);}
    }
  }
  async function ingest(repo,author){
    verifyOwner(repo,author);
    const release=await api(`/repos/${repositoryName(repo.full_name)}/releases/latest`);
    if(release.draft||release.prerelease)throw new Error('A stable release is required');
    const asset=release.assets.find(a=>a.name==='mod.zip');if(!asset||asset.size>MAX_PACKAGE)throw new Error('Release must contain mod.zip (at most 50 MiB)');
    const expected=`https://github.com/${repo.full_name}/releases/download/`;
    if(!asset.browser_download_url.startsWith(expected))throw new Error('Invalid GitHub release asset URL');
    const bytes=await download(asset.browser_download_url,MAX_PACKAGE);
    const {manifest,sha256}=inspectZip(bytes);
    if(manifest.version.includes('-')||release.tag_name!==`v${manifest.version}`)throw new Error('Stable tag must match manifest version (vX.Y.Z)');
    register(catalog,{manifest,sha256,author:repo.owner.login,repository:repo.full_name,repository_id:repo.id,owner_id:repo.owner.id,release:release.html_url,download_url:asset.browser_download_url});
  }
  async function report(number,text){await api(`/repos/${central}/issues/${number}/comments`,'POST',{body:text});}
  // Create the catalog branch once; never change the default branch.
  try{await api(`/repos/${central}/git/ref/heads/mod-catalog`);}catch(e){if(e.status!==404)throw e;await api(`/repos/${central}/git/refs`,'POST',{ref:'refs/heads/mod-catalog',sha:process.env.GITHUB_SHA});}
  const content=JSON.stringify(catalog,null,2)+'\n';
  if(!current||Buffer.from(current.content,'base64').toString()!==content){await api(`/repos/${central}/contents/index.json`,'PUT',{message:'Update SWCH mod catalog',branch:'mod-catalog',content:Buffer.from(content).toString('base64'),...(current?{sha:current.sha}:{})});}
  if(issue){await report(issue.number,'Mod validated and published to the SWCH catalog. Future stable releases are checked every six hours.');await api(`/repos/${central}/issues/${issue.number}`,'PATCH',{state:'closed'});}
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)){run().catch(e=>{console.error(e.message);process.exitCode=1;});}
