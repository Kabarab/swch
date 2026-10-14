import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {makeZip, inspectDirectory, sha256} from '../mod-sdk/validation.mjs';

const root=fileURLToPath(new URL('../',import.meta.url));
const source=path.join(root,'mods/autoclaimer');
export function buildUI() {
  const state=fs.readFileSync(path.join(source,'claim-state.mjs'),'utf8').replace(/\bexport /g,'');
  const ui=fs.readFileSync(path.join(source,'ui.mjs'),'utf8').replace(/^import .*\n/,'').replace(/\bexport /g,'');
  const css=fs.readFileSync(path.join(root,'ui/launcher.css'),'utf8');
  const view=fs.readFileSync(path.join(source,'view.html'),'utf8');
  const bootstrap=`
const t=value=>value;
const control=input=>swch.call('action',{id:'control',input});
async function api(path,body) {
  const {job}=await control({operation:'request',path,...(body!==undefined?{body}:{})});
  const deadline=Date.now()+670000;
  while(Date.now()<deadline){const response=await control({operation:'poll',job});
    if(response.done){if(response.error)throw new Error(response.error);return response.result;}
    await new Promise(resolve=>setTimeout(resolve,300));}
  throw new Error('Проверка не завершилась вовремя. Повтори операцию.');
}
const copyText=text=>control({operation:'clipboard',text});
const escape=value=>String(value??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
function showToast(message){const e=document.getElementById('toast');e.textContent=message;e.classList.add('show');clearTimeout(showToast.timer);showToast.timer=setTimeout(()=>e.classList.remove('show'),5000);}
const {accountState,indexedResults,runSummary,isBrowserChallenge}=(()=>{${state};return {accountState,indexedResults,runSummary,isBrowserChallenge};})();
${ui}
initAutoClaim({api,esc:escape,dateLabel:value=>new Date(value).toLocaleString('ru-RU'),toast:showToast,demo:false});
void loadAutoClaim();
`;
  return `<!doctype html><html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Автоклеймер SWCH</title><style>${css}\nbody{display:block;min-width:0;padding:24px;margin:0}.claim-workspace{max-width:1100px;margin:auto}.claim-workspace h2{margin:0}.settings-panel{display:block}.claim-workspace .claim-view[hidden]{display:none}#toast{position:fixed;bottom:24px;left:24px;right:24px;pointer-events:none;z-index:10}@media(max-width:600px){body{padding:12px}.claim-run-box{flex-wrap:wrap}.claim-tabs{overflow-x:auto}}</style></head><body>${view}<div id="toast" class="toast" role="status"></div><script type="module">${bootstrap.replace(/<\/script/gi,'<\\/script')}</script></body></html>`;
}

export function buildPackage(target,destination,binary) {
  if(!['windows-x64','macos-arm64'].includes(target))throw new Error('Target must be windows-x64 or macos-arm64');
  const name='swch-autoclaimer'+(target==='windows-x64'?'.exe':'');
  binary ||= path.join(root,'target_build/release',name);
  if(!fs.existsSync(binary))throw new Error('Build the native service first: cargo build --release --bin swch-autoclaimer');
  const files=new Map([
    ['manifest.json',Buffer.from(JSON.stringify({...JSON.parse(fs.readFileSync(path.join(source,'manifest.json'),'utf8')),platforms:[target]},null,2)+'\n')],
    ['backend.mjs',fs.readFileSync(path.join(source,'backend.mjs'))],
    ['index.html',Buffer.from(buildUI())],
    [`native/${target}/${name}`,fs.readFileSync(binary)]
  ]);
  const bytes=makeZip(files);
  fs.mkdirSync(destination,{recursive:true});
  const output=path.join(destination,'swch.autoclaimer.zip');fs.writeFileSync(output,bytes);
  console.log(JSON.stringify({path:output,sha256:sha256(bytes),files:files.size}));
  return output;
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  try{buildPackage(...process.argv.slice(2));}catch(error){console.error(error.message);process.exitCode=1;}
}
