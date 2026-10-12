import {PLATFORMS,registerModPlatforms} from './platforms.mjs?v=mods-1';
import {localizedConfirm as confirm,getLocale} from './i18n.js';
import {modGuideHTML,setupModGuide} from './mod-guide.js';
const esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let host,mods=[],theme={},catalog=[],compatibility={},catalogWarning='',target='',events,timer,frameKey='',renderGeneration=0;
const enc=encodeURIComponent;
export const isModPlatform=id=>id?.startsWith('mod:');
export const modCapability=(id,operation)=>PLATFORMS[id]?.capabilities?.includes(operation);
export async function modOperation(id,operation,input={}) {return host.api(`/api/platforms/${enc(id)}/${operation}`,input);}
export function applyModTheme() {
  for(const key of ['bg','panel','panel-hover','line','text','muted','radius']) {
    if(theme[key])document.documentElement.style.setProperty('--'+key,theme[key]);else document.documentElement.style.removeProperty('--'+key);
  }
  if(theme.accent){document.documentElement.style.setProperty('--accent',theme.accent);document.documentElement.style.setProperty('--accent-rgb',[1,3,5].map(i=>parseInt(theme.accent.slice(i,i+2),16)).join(','));}
}
function navigation(platforms) {
  const sidebar=document.querySelector('.sidebar'),anchor=sidebar.querySelector('.connect-link');
  sidebar.querySelectorAll('[data-mod-platform-nav]').forEach(e=>e.remove());
  for(const p of platforms){const button=document.createElement('button');button.className='platform-item';button.dataset.platform=p.id;button.dataset.modPlatformNav='';button.innerHTML=`<span class="platform-mark">◈</span>${esc(p.name)}<span id="${esc(p.id)}-count">0</span>`;anchor.before(button);}
  const nav=sidebar.querySelector('nav');nav.querySelectorAll('[data-mod-tab-nav]').forEach(e=>e.remove());
  for(const mod of mods.filter(m=>m.enabled&&m.status==='active'&&m.manifest.ui))for(const tab of mod.contributions?.tabs||[]){const button=document.createElement('button');button.className='nav-item';button.dataset.page=`mod-tab:${mod.manifest.id}:${tab.id}`;button.dataset.modTabNav='';button.textContent=tab.name;nav.append(button);}
}
export async function loadModLibrary() {
  const data=await host.api('/api/platforms/library');
  const state=await host.api('/api/mods');mods=state.mods||[];theme=state.theme||{};
  const platforms=data.platforms||[];registerModPlatforms(platforms);navigation(platforms);applyModTheme();
  return {games:data.games||[],accounts:data.accounts||[]};
}
export function modPageTitle(page) {if(page==='mods')return ['Твои моды','Новые возможности от сообщества.','Моды'];if(page.startsWith('mod-tab:')){const [,id,tab]=page.split(':');const name=mods.find(m=>m.manifest.id===id)?.contributions?.tabs?.find(t=>t.id===tab)?.name||'Мод';return [name,'Расширение SWCH','Мод'];}return null;}
export function renderModPage(page) {
  const view=document.getElementById('mods-view');view.hidden=page!=='mods'&&!page.startsWith('mod-tab:');
  if(view.hidden){if(frameKey){view.replaceChildren();frameKey='';}return;}
  if(page==='mods'){frameKey='';void renderCatalogPage(view);return;}
  if(frameKey===page)return;frameKey=page;const generation=++renderGeneration;
  const [,id,tab]=page.split(':');view.textContent='Загружаем мод…';
  void host.api(`/api/mods/${enc(id)}/ui`).then(({html})=>{
    if(generation!==renderGeneration||frameKey!==page)return;
    const frame=document.createElement('iframe');frame.className='mod-frame';frame.title=modPageTitle(page)[0];frame.sandbox='allow-scripts allow-forms';
    const bootstrap=`<script>const pending=new Map(),listeners=new Map();let sequence=0;window.swch={call:(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence;const timer=setTimeout(()=>{pending.delete(id);reject(new Error('SWCH request timed out'));},35000);pending.set(id,{resolve,reject,timer});parent.postMessage({swch:1,id,method,params},'*');}),on:(event,fn)=>{if(!listeners.has(event))listeners.set(event,new Set());listeners.get(event).add(fn);return ()=>listeners.get(event)?.delete(fn);},tab:${JSON.stringify(tab)}};addEventListener('message',e=>{if(e.source!==parent||e.data?.swch!==1)return;const d=e.data;if(d.event){for(const fn of listeners.get(d.event)||[])fn(d.data);return;}const p=pending.get(d.id);if(p){clearTimeout(p.timer);pending.delete(d.id);d.error?p.reject(new Error(d.error)):p.resolve(d.result);}});<\/script>`;
    frame.srcdoc=bootstrap+html;view.replaceChildren(frame);
    frame.dataset.modId=id;
  }).catch(e=>{if(generation===renderGeneration)view.textContent=e.message;});
}
function button(action,label,id,extra=''){return `<button class="secondary" data-mod-action="${action}" data-mod-id="${esc(id)}" ${extra}>${label}</button>`;}
function installedCard(mod) {
  const {manifest:m}=mod;const remote=catalog.find(c=>c.manifest.id===m.id);
  const newer=(a,b)=>{const x=a.split(/[.+-]/).slice(0,3).map(Number),y=b.split(/[.+-]/).slice(0,3).map(Number);for(let i=0;i<3;i++){if(x[i]!==y[i])return x[i]>y[i];}return false;};
  const upgrade=remote&&newer(remote.manifest.version,m.version)&&compatibility[m.id]!==false&&!mod.development;
  return `<article class="mod-card"><div class="mod-card-heading"><h3>${esc(m.name)}</h3><span>${esc(m.version)}</span></div><p translate="no">${esc(m.description)}</p><p class="muted">${mod.enabled?'Включён':'Выключен'}${mod.development?' · разработка':''}</p>${mod.error?`<p role="alert" class="error-text">${esc(mod.error)}</p>`:''}<div class="mod-actions">${button('toggle',mod.enabled?'Выключить':'Включить',m.id,`data-enabled="${!mod.enabled}"`)}${button('restart',mod.development?'Перезагрузить':'Перезапустить',m.id)}${upgrade?button('install','Обновить',m.id):''}${m.theme?button('theme','Применить тему',m.id):''}${button('settings','Настройки',m.id)}${button('logs','Журнал',m.id)}${button('remove','Удалить',m.id)}</div><div class="mod-actions">${(mod.contributions?.actions||[]).map(a=>button('run',esc(a.name),m.id,`data-contribution="${esc(a.id)}"`)).join('')}</div></article>`;
}
function catalogCard(entry) {
  const m=entry.manifest,installed=mods.some(x=>x.manifest.id===m.id),supported=compatibility[m.id]??m.platforms.includes(target);
  return `<article class="mod-card"><div class="mod-card-heading"><h3>${esc(m.name)}</h3><span>${esc(m.version)}</span></div><p translate="no">${esc(m.description)}</p><p class="muted">${esc(entry.author)} · ${esc(m.platforms.join(', '))} · API ${esc(m.api)}</p><a href="https://github.com/${esc(entry.repository)}" target="_blank" rel="noopener noreferrer">Исходники на GitHub ↗</a><div class="mod-actions">${installed?'<span>Установлен</span>':button('install',supported?'Установить':'Несовместим с этой версией SWCH или ОС',m.id,supported?'':'disabled')}</div></article>`;
}
async function renderCatalogPage(view) {
  const generation=++renderGeneration;
  if(!view.querySelector('#mod-search')){
    view.innerHTML=`<div class="mods-toolbar"><label class="search"><input id="mod-search" placeholder="Найти мод" aria-label="Найти мод"></label>${button('catalog','Обновить каталог','')}${button('reset-theme','Стандартная тема','')}</div><p class="muted">Моды имеют права пользователя на компьютере. Автоматическая проверка каталога проверяет пакет и автора, но не безопасность кода.</p><p id="mod-warning" role="status"></p><h2>Установленные</h2><div id="installed-mods" class="mods-grid"></div><div id="mod-platform-controls"></div><h2>Каталог</h2><div id="catalog-mods" class="mods-grid"></div><details class="ux-disclosure"><summary>Установка из ZIP и разработка</summary><label>ZIP мода<input id="mod-zip" type="file" accept=".zip"></label><label>Папка разработки<input id="mod-dev-path" placeholder="Полный путь к папке мода"></label>${button('development','Подключить папку','')}<p><a href="https://github.com/Kabarab/swch/blob/main/docs/mods.md" target="_blank" rel="noopener noreferrer">Документация для авторов ↗</a></p></details><h3>Очистка сохранённых данных</h3><label>ID удалённого мода<input id="mod-clear-id" placeholder="author.mod"></label>${button('clear-data','Очистить данные','')}`;
    view.querySelector('#mod-search').addEventListener('input',filterCards);
    view.insertAdjacentHTML('afterbegin',modGuideHTML());setupModGuide(view);
  }
  try{const state=await host.api('/api/mods');if(generation!==renderGeneration)return;mods=state.mods||[];theme=state.theme||{};applyModTheme();}catch(e){catalogWarning=e.message;}
  if(generation!==renderGeneration)return;
  view.querySelector('#installed-mods').innerHTML=mods.map(installedCard).join('')||'<p class="muted">Модов пока нет. Выбери мод в каталоге или установи ZIP.</p>';
  view.querySelector('#catalog-mods').innerHTML=catalog.map(catalogCard).join('')||'<p class="muted">Каталог пока пуст или недоступен.</p>';
  view.querySelector('#mod-warning').textContent=catalogWarning;
  view.querySelector('#mod-platform-controls').innerHTML=Object.entries(PLATFORMS).filter(([id])=>isModPlatform(id)).map(([id,p])=>`<section class="mod-card"><h3>${esc(p.name)}</h3><div class="mod-actions">${['connect','accounts','sync','library'].filter(op=>modCapability(id,op)).map(op=>`<button class="secondary" data-mod-platform="${esc(id)}" data-mod-operation="${op}">${{connect:'Подключить аккаунт',accounts:'Обновить аккаунты',sync:'Синхронизировать игры',library:'Загрузить библиотеку'}[op]}</button>`).join('')}</div></section>`).join('');
  filterCards();
}
function filterCards(){const query=document.getElementById('mod-search')?.value.trim().toLocaleLowerCase(getLocale())||'';document.querySelectorAll('#installed-mods .mod-card,#catalog-mods .mod-card').forEach(e=>e.hidden=!e.textContent.toLocaleLowerCase(getLocale()).includes(query));}
async function updateCatalog(force=false){try{const data=await host.api('/api/mods/catalog'+(force?'?refresh=true':''));catalog=data.catalog?.mods?.filter(m=>!data.catalog.blocked?.includes(m.manifest.id))||[];compatibility=data.compatibility||{};catalogWarning=data.warning||'';target=data.target||'';}catch(e){catalogWarning=e.message;}}
function trusted(){return confirm('Мод сможет работать с файлами, сетью и запускать программы от твоего имени. Ты доверяешь автору и хочешь установить мод?');}
async function details(id,kind){
  let dialog=document.getElementById('mod-details');if(!dialog){dialog=document.createElement('dialog');dialog.id='mod-details';dialog.className='mod-details';document.body.append(dialog);}
  const data=await host.api(`/api/mods/${enc(id)}/${kind}`);
  dialog.innerHTML=`<h2>${kind==='logs'?'Журнал':'Настройки'} · ${esc(id)}</h2>${kind==='logs'?`<pre>${esc((data.logs||[]).join('\n')||'Журнал пуст')}</pre>`:`<label>Настройки мода (JSON)<textarea id="mod-settings-json" rows="12">${esc(JSON.stringify(data,null,2))}</textarea></label>${button('save-settings','Сохранить',id)}`}<form method="dialog"><button class="secondary">Закрыть</button></form>`;dialog.showModal();
}
export function setupMods(options) {
  host=options;
  document.addEventListener('click',async e=>{
    const b=e.target.closest('[data-mod-action],[data-mod-operation]');if(!b)return;
    b.disabled=true;const id=b.dataset.modId;
    try{
      if(b.dataset.modOperation){const platform=b.dataset.modPlatform,op=b.dataset.modOperation;await modOperation(platform,op,b.dataset.account?{account_id:b.dataset.account}:{});await host.changed();return;}
      const action=b.dataset.modAction;
      if(action==='catalog'){await updateCatalog(true);await host.render();return;}
      if(action==='install'){if(!trusted())return;await host.api('/api/mods/install',{id,trusted:true});}
      if(action==='toggle')await host.api(`/api/mods/${enc(id)}/toggle`,{enabled:b.dataset.enabled==='true'});
      if(action==='restart')await host.api(`/api/mods/${enc(id)}/restart`,{});
      if(action==='remove'){if(!confirm('Удалить мод? Его настройки и данные сохранятся.'))return;await host.api(`/api/mods/${enc(id)}/remove`,{});}
      if(action==='theme')await host.api('/api/mods/theme',{id});
      if(action==='reset-theme')await host.api('/api/mods/theme',{});
      if(action==='logs'||action==='settings'){await details(id,action);return;}
      if(action==='save-settings'){await host.api(`/api/mods/${enc(id)}/settings`,JSON.parse(document.getElementById('mod-settings-json').value));document.getElementById('mod-details').close();}
      if(action==='run')await host.api(`/api/mods/${enc(id)}/sdk`,{method:'action',params:{id:b.dataset.contribution,input:{}}});
      if(action==='development'){const path=document.getElementById('mod-dev-path').value;if(!path||!trusted())return;await host.api('/api/mods/development',{path,trusted:true});}
      if(action==='clear-data'){const id=document.getElementById('mod-clear-id').value;if(!id||!confirm('Безвозвратно очистить настройки, секреты, игры и аккаунты этого мода?'))return;await host.api(`/api/mods/${enc(id)}/clear-data`,{});}
      await host.changed();host.toast('Готово');
    }catch(e){host.toast(e.message);}finally{b.disabled=false;}
  });
  document.addEventListener('change',async e=>{if(e.target.id!=='mod-zip'||!e.target.files[0])return;try{if(!trusted())return;const bytes=await e.target.files[0].arrayBuffer(),hash=[...new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))].map(n=>n.toString(16).padStart(2,'0')).join('');const res=await fetch(`/api/mods/install-file?trusted=true&sha256=${hash}`,{method:'POST',headers:{'Content-Type':'application/octet-stream'},body:bytes});const data=await res.json();if(!res.ok)throw new Error(data.error);await host.changed();host.toast('Мод установлен');}catch(err){host.toast(err.message);}finally{e.target.value='';}});
  window.addEventListener('message',async e=>{
    const frame=document.querySelector('.mod-frame');if(!frame||e.source!==frame.contentWindow||e.data?.swch!==1)return;
    const {id,method,params}=e.data;if(!Number.isSafeInteger(id)||typeof method!=='string')return;
    const source=e.source;
    try{const result=await host.api(`/api/mods/${enc(frame.dataset.modId)}/sdk`,{method,params});source.postMessage({swch:1,id,result},'*');}catch(err){source.postMessage({swch:1,id,error:err.message},'*');}
  });
  if(!options.demo){events=new EventSource('/api/mods/events');events.onmessage=e=>{try{const data=JSON.parse(e.data),frame=document.querySelector('.mod-frame'),prefix=`mod:${frame?.dataset.modId}:`;frame?.contentWindow?.postMessage({swch:1,...data,event:data.event?.startsWith(prefix)?data.event.slice(prefix.length):data.event},'*');if(['mods.changed','library.changed','accounts.changed'].includes(data.event)){if(data.event==='mods.changed')frameKey='';clearTimeout(timer);timer=setTimeout(()=>void host.changed(),200);}}catch{}};}
  // Catalog downloads only when the user opens the Mods page.
  document.addEventListener('click',e=>{if(e.target.closest('[data-page="mods"]'))void updateCatalog().then(()=>host.render());});
}
