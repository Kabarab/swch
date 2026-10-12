// Run only in the GitHub publish workflow after validating packs as data.
import fs from 'node:fs/promises';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
const root=process.cwd(),destination=path.resolve(root,'output/publish-languages');
if(!destination.startsWith(root+path.sep))throw new Error('Invalid destination');
const git=(...args)=>execFileSync('git',args,{cwd:root,stdio:'inherit'});
let branchExists=true;
try{git('fetch','origin','language-packs');}catch{branchExists=false;}
await fs.mkdir(path.dirname(destination),{recursive:true});
if(branchExists)git('worktree','add','--detach',destination,'FETCH_HEAD');
else {git('worktree','add','--detach',destination,'HEAD');execFileSync('git',['checkout','--orphan','language-packs'],{cwd:destination,stdio:'inherit'});execFileSync('git',['rm','-rf','.'],{cwd:destination,stdio:'inherit'});}
const index=JSON.parse(await fs.readFile('ui/lang/index.json','utf8'));
for(const entry of index.languages){
  if(!entry.sha256)continue;
  const folder=path.join(destination,index.revision);await fs.mkdir(folder,{recursive:true});
  const file=path.join(folder,`${entry.locale}-${entry.sha256}.json`);
  await fs.copyFile(path.join('ui/lang/packs',entry.locale+'.json'),file);
}
await fs.copyFile('ui/lang/index.json',path.join(destination,'index.json'));
execFileSync('git',['add','.'],{cwd:destination,stdio:'inherit'});
const dirty=execFileSync('git',['status','--porcelain'],{cwd:destination,encoding:'utf8'}).trim();
if(dirty){
  execFileSync('git',['-c','user.name=github-actions[bot]','-c','user.email=41898282+github-actions[bot]@users.noreply.github.com','commit','-m','Publish validated SWCH language packs'],{cwd:destination,stdio:'inherit'});
  execFileSync('git',['push','origin','HEAD:refs/heads/language-packs'],{cwd:destination,stdio:'inherit'});
}
