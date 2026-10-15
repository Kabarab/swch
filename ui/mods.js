import {PLATFORMS,registerModPlatforms} from './platforms.mjs?v=mods-1';
import {localizedConfirm as confirm,getLocale} from './i18n.js';
import {modGuideHTML,setupModGuide} from './mod-guide.js?v=copy-prompt-2';
import {gameIdentity,modGames,modMatchesGame,bridgeJSON} from './mod-games.mjs';
import {defaultModFilters,modGameChoices,modDownloadStats,filterModEntries} from './mod-filters.mjs';
const esc=v=>String(v??'').replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let host,mods=[],theme={},catalog=[],compatibility={},catalogWarning='',target='',events,timer,frameKey='',renderGeneration=0;
let gameScope=null,frameContext=null,filters=defaultModFilters();
const enc=encodeURIComponent;
export const isModPlatform=id=>id?.startsWith('mod:');
export const modCapability=(id,operation)=>PLATFORMS[id]?.capabilities?.includes(operation);
export function renderGameModsButton(game){
  const count=mods.filter(mod=>modMatchesGame(mod,game)).length;
  return `<div class="game-mods-entry"><button class="secondary" data-mod-action="game-mods" data-game="${esc(host.gameKey(game))}">Моды${count?` · ${count}`:''}</button></div>`;
}
function gameContext(){
  const game=gameScope&&host.games().find(g=>gameIdentity(g)===gameIdentity(gameScope))||null;
  const account=game&&host.accountForGame(game);
  return {game,account:account?{id:account.id,launcher:account.launcher,username:account.username,display_name:account.display_name,is_active:account.is_active}:null};
}
function gameLabels(games){return games.length?`<p class="muted mod-game-labels">Игры и приложения: ${games.map(g=>esc(g.name||`${g.launcher}: ${g.app_id}`)).join(' · ')}</p>`:'';}
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
export function modPageTitle(page) {if(page==='mods')return ['Твои моды','Новые возможности от сообщества.','Моды'];if(page.startsWith('mod-tab:')){const [,id,tab]=page.split(':');const name=mods.find(m=>m.manifest.id===id)?.contributions?.tabs?.find(t=>t.id===tab)?.name||'Мод';return [name,frameContext?.game?.name||'Расширение SWCH','Мод'];}return null;}
export function renderModPage(page) {
  const view=document.getElementById('mods-view');view.hidden=page!=='mods'&&!page.startsWith('mod-tab:');
  if(view.hidden){++renderGeneration;if(frameKey||view.querySelector('.mod-frame')){view.replaceChildren();frameKey='';}return;}
  if(page==='mods'){frameKey='';void renderCatalogPage(view);return;}
  if(frameKey===page)return;frameKey=page;const generation=++renderGeneration;
  const [,id,tab]=page.split(':');view.textContent='Загружаем мод…';
  void host.api(`/api/mods/${enc(id)}/ui`).then(({html})=>{
    if(generation!==renderGeneration||frameKey!==page)return;
    const frame=document.createElement('iframe');frame.className='mod-frame';frame.title=modPageTitle(page)[0];frame.sandbox='allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox allow-downloads';
    const bootstrap=`<script>const pending=new Map(),listeners=new Map();let sequence=0;window.swch={call:(method,params={})=>new Promise((resolve,reject)=>{const id=++sequence;const timer=setTimeout(()=>{pending.delete(id);reject(new Error('SWCH request timed out'));},35000);pending.set(id,{resolve,reject,timer});parent.postMessage({swch:1,id,method,params},'*');}),on:(event,fn)=>{if(!listeners.has(event))listeners.set(event,new Set());listeners.get(event).add(fn);return ()=>listeners.get(event)?.delete(fn);},tab:${bridgeJSON(tab)},game:${bridgeJSON(frameContext?.game||null)},account:${bridgeJSON(frameContext?.account||null)}};addEventListener('message',e=>{if(e.source!==parent||e.data?.swch!==1)return;const d=e.data;if(d.event){for(const fn of listeners.get(d.event)||[])fn(d.data);return;}const p=pending.get(d.id);if(p){clearTimeout(p.timer);pending.delete(d.id);d.error?p.reject(new Error(d.error)):p.resolve(d.result);}});<\/script>`;
    frame.srcdoc=bootstrap+html;view.replaceChildren(frame);
    if(frameContext?.game){const controls=document.createElement('div');controls.className='mod-actions';controls.innerHTML=button('back-mods','Моды этой игры','')+button('back-game','Вернуться к игре','');view.prepend(controls);}
    frame.dataset.modId=id;
  }).catch(e=>{if(generation===renderGeneration)view.textContent=e.message;});
}
function button(action,label,id,extra=''){return `<button class="secondary" data-mod-action="${action}" data-mod-id="${esc(id)}" ${extra}>${label}</button>`;}
function openButtons(mod){return mod.enabled&&mod.status==='active'&&mod.manifest.ui?(mod.contributions?.tabs||[{id:'main',name:mod.manifest.name}]).map(tab=>button('open',`Открыть · ${esc(tab.name)}`,mod.manifest.id,`data-mod-tab="${esc(tab.id)}"`)).join(''):'';}
function installedCard(mod) {
  const {manifest:m}=mod;const remote=catalog.find(c=>c.manifest.id===m.id);
  const newer=(a,b)=>{const x=a.split(/[.+-]/).slice(0,3).map(Number),y=b.split(/[.+-]/).slice(0,3).map(Number);for(let i=0;i<3;i++){if(x[i]!==y[i])return x[i]>y[i];}return false;};
  const upgrade=remote&&newer(remote.manifest.version,m.version)&&compatibility[m.id]!==false&&!mod.development;
  return `<article class="mod-card"><div class="mod-card-heading"><h3>${esc(m.name)}</h3><span>${esc(m.version)}</span></div><p translate="no">${esc(m.description)}</p><p class="muted">${mod.enabled?'Включён':'Выключен'}${mod.development?' · разработка':''}</p>${mod.error?`<p role="alert" class="error-text">${esc(mod.error)}</p>`:''}${gameLabels(modGames(mod))}${downloadLabel(mod)}<div class="mod-actions">${openButtons(mod)}${button('bindings','Игры и приложения',m.id)}${button('toggle',mod.enabled?'Выключить':'Включить',m.id,`data-enabled="${!mod.enabled}"`)}${button('restart',mod.development?'Перезагрузить':'Перезапустить',m.id)}${upgrade?button('install','Обновить',m.id):''}${m.theme?button('theme','Применить тему',m.id):''}${button('settings','Настройки',m.id)}${button('logs','Журнал',m.id)}${button('remove','Удалить',m.id)}</div><div class="mod-actions">${(mod.contributions?.actions||[]).map(a=>button('run',esc(a.name),m.id,`data-contribution="${esc(a.id)}"`)).join('')}</div></article>`;
}
function catalogCard(entry) {
  const m=entry.manifest,installed=mods.some(x=>x.manifest.id===m.id),supported=compatibility[m.id]??m.platforms.includes(target);
  return `<article class="mod-card"><div class="mod-card-heading"><h3>${esc(m.name)}</h3><span>${esc(m.version)}</span></div><p translate="no">${esc(m.description)}</p><p class="muted">${esc(entry.author)} · ${esc(m.platforms.join(', '))} · API ${esc(m.api)}</p>${gameLabels(m.games||[])}${downloadLabel(entry)}<a href="https://github.com/${esc(entry.repository)}" target="_blank" rel="noopener noreferrer">Исходники на GitHub ↗</a><div class="mod-actions">${installed?'<span>Установлен</span>':button('install',supported?'Установить':'Несовместим с этой версией SWCH или ОС',m.id,supported?'':'disabled')}</div></article>`;
}
function downloadLabel(entry){
  const stats=modDownloadStats(entry,catalog);
  const updated=stats?.downloads_updated_at?` · Данные: ${stats.downloads_updated_at}`:'';
  return `<p class="muted mod-downloads" title="${esc('Скачивания mod.zip всех стабильных версий на GitHub'+updated)}">Скачивания: ${stats?new Intl.NumberFormat(getLocale()).format(stats.downloads):'нет данных'}</p>`;
}
function filterSelect(id,label,options){return `<label>${label}<select id="${id}">${options.map(([value,name])=>`<option value="${value}">${name}</option>`).join('')}</select></label>`;}
async function renderCatalogPage(view) {
  const generation=++renderGeneration;
  if(!view.querySelector('#mod-search')){
    view.innerHTML=`<div class="mods-toolbar"><label class="search"><input id="mod-search" placeholder="Найти мод" aria-label="Найти мод"></label>${button('catalog','Обновить каталог','')}${button('reset-theme','Стандартная тема','')}</div><div class="mod-filters" aria-label="Фильтры модов"><label>Игра или приложение<select id="mod-game-filter"></select></label>${filterSelect('mod-kind-filter','Тип мода',[['all','Все типы'],['ui','С интерфейсом'],['theme','Темы'],['backend','С серверной частью']])}${filterSelect('mod-status-filter','Установка',[['all','Все моды'],['installed','Установленные'],['available','Не установленные'],['enabled','Включённые'],['disabled','Выключенные']])}${filterSelect('mod-compatibility-filter','Совместимость',[['all','Любая совместимость'],['compatible','Совместимые'],['incompatible','Несовместимые']])}${filterSelect('mod-sort','Сортировка модов',[['name','По названию'],['downloads','По скачиваниям']])}${button('reset-filters','Сбросить фильтры','')}</div><div id="mod-game-scope" class="mod-game-scope"></div><p id="mod-results" class="muted" role="status"></p><p class="muted">Моды имеют права пользователя на компьютере. Автоматическая проверка каталога проверяет пакет и автора, но не безопасность кода.</p><p id="mod-warning" role="status"></p><section id="installed-mods-section"><h2>Установленные</h2><div id="installed-mods" class="mods-grid"></div><div id="mod-platform-controls"></div></section><section id="catalog-mods-section"><h2>Каталог</h2><div id="catalog-mods" class="mods-grid"></div></section><details class="ux-disclosure"><summary>Установка из ZIP и разработка</summary><label>ZIP мода<input id="mod-zip" type="file" accept=".zip"></label><label>Папка разработки<input id="mod-dev-path" placeholder="Полный путь к папке мода"></label>${button('development','Подключить папку','')}<p><a href="https://github.com/Kabarab/swch/blob/main/docs/mods.md" target="_blank" rel="noopener noreferrer">Документация для авторов ↗</a></p></details><h3>Очистка сохранённых данных</h3><label>ID удалённого мода<input id="mod-clear-id" placeholder="author.mod"></label>${button('clear-data','Очистить данные','')}`;
    view.querySelector('#mod-search').addEventListener('input',e=>{filters.search=e.target.value;applyFilters(view);});
    for(const [id,key] of [['mod-game-filter','game'],['mod-kind-filter','kind'],['mod-status-filter','status'],['mod-compatibility-filter','compatibility'],['mod-sort','sort']])view.querySelector('#'+id).addEventListener('change',e=>{filters[key]=e.target.value;applyFilters(view);});
    view.insertAdjacentHTML('beforeend',modGuideHTML());setupModGuide(view);
  }
  try{const state=await host.api('/api/mods');if(generation!==renderGeneration)return;mods=state.mods||[];theme=state.theme||{};applyModTheme();}catch(e){catalogWarning=e.message;}
  if(generation!==renderGeneration)return;
  const choices=modGameChoices(host.games(),mods,catalog).sort((a,b)=>(a.name||a.app_id).localeCompare(b.name||b.app_id,getLocale()));
  if(gameScope&&!choices.some(game=>gameIdentity(game)===gameIdentity(gameScope)))choices.push(gameScope);
  view.querySelector('#mod-game-filter').innerHTML='<option value="all">Все игры и приложения</option><option value="general">Общие моды без привязки</option>'+choices.map(game=>`<option value="${esc(gameIdentity(game))}">${esc(game.name||game.app_id)} · ${esc(PLATFORMS[game.launcher]?.name||game.launcher)}</option>`).join('');
  applyFilters(view);
}
function applyFilters(view){
  gameScope=modGameChoices(host.games(),mods,catalog).find(game=>gameIdentity(game)===filters.game)||gameScope&&gameIdentity(gameScope)===filters.game&&gameScope||null;
  for(const [id,key] of [['mod-search','search'],['mod-game-filter','game'],['mod-kind-filter','kind'],['mod-status-filter','status'],['mod-compatibility-filter','compatibility'],['mod-sort','sort']]){
    const control=view.querySelector('#'+id);if(control.value!==filters[key])control.value=filters[key];
  }
  const environment={mods,catalog,compatibility,target,locale:getLocale()},installed=filterModEntries(mods,filters,environment),available=filterModEntries(catalog,filters,environment);
  const canReturn=gameScope&&host.games().some(game=>gameIdentity(game)===filters.game);
  view.querySelector('#mod-game-scope').innerHTML=gameScope?`<strong>${esc(gameScope.name||gameScope.app_id)}</strong><div class="mod-actions">${canReturn?button('back-game','Вернуться к игре',''):''}${button('all-mods','Все моды','')}</div>`:'';
  view.querySelector('#installed-mods').innerHTML=installed.map(installedCard).join('')||'<p class="muted">По выбранным фильтрам моды не найдены.</p>';
  view.querySelector('#catalog-mods').innerHTML=available.map(catalogCard).join('')||'<p class="muted">По выбранным фильтрам моды не найдены.</p>';
  const installedOnly=['installed','enabled','disabled'].includes(filters.status);
  view.querySelector('#installed-mods-section').hidden=filters.status==='available';
  view.querySelector('#catalog-mods-section').hidden=installedOnly;
  view.querySelector('#mod-results').textContent=`Установленные: ${installed.length} · Каталог: ${available.length}`;
  view.querySelector('#mod-warning').textContent=catalogWarning;
  view.querySelector('#mod-platform-controls').innerHTML=Object.entries(PLATFORMS).filter(([id,p])=>isModPlatform(id)&&installed.some(m=>m.manifest.id===p.mod_id)).map(([id,p])=>`<section class="mod-card"><h3>${esc(p.name)}</h3><div class="mod-actions">${['connect','accounts','sync','library'].filter(op=>modCapability(id,op)).map(op=>`<button class="secondary" data-mod-platform="${esc(id)}" data-mod-operation="${op}">${{connect:'Подключить аккаунт',accounts:'Обновить аккаунты',sync:'Синхронизировать игры',library:'Загрузить библиотеку'}[op]}</button>`).join('')}</div></section>`).join('');
}
async function updateCatalog(force=false){try{const data=await host.api('/api/mods/catalog'+(force?'?refresh=true':''));catalog=data.catalog?.mods?.filter(m=>!data.catalog.blocked?.includes(m.manifest.id))||[];compatibility=data.compatibility||{};catalogWarning=data.warning||'';target=data.target||'';}catch(e){catalogWarning=e.message;}}
function trusted(){return confirm('Мод сможет работать с файлами, сетью и запускать программы от твоего имени. Ты доверяешь автору и хочешь установить мод?');}
async function details(id,kind){
  let dialog=document.getElementById('mod-details');if(!dialog){dialog=document.createElement('dialog');dialog.id='mod-details';dialog.className='mod-details';document.body.append(dialog);}
  const data=await host.api(`/api/mods/${enc(id)}/${kind}`);
  dialog.innerHTML=`<h2>${kind==='logs'?'Журнал':'Настройки'} · ${esc(id)}</h2>${kind==='logs'?`<pre>${esc((data.logs||[]).join('\n')||'Журнал пуст')}</pre>`:`<label>Настройки мода (JSON)<textarea id="mod-settings-json" rows="12">${esc(JSON.stringify(data,null,2))}</textarea></label>${button('save-settings','Сохранить',id)}`}<form method="dialog"><button class="secondary">Закрыть</button></form>`;dialog.showModal();
}
function bindingDetails(id){
  const mod=mods.find(m=>m.manifest.id===id);if(!mod)return;
  let dialog=document.getElementById('mod-bindings');
  if(!dialog){dialog=document.createElement('dialog');dialog.id='mod-bindings';dialog.className='mod-details';document.body.append(dialog);}
  const selected=new Set(modGames(mod).map(gameIdentity)),choices=new Map();
  for(const game of [...modGames(mod),...(mod.manifest.games||[]),...host.games()]){
    if(!game.launcher||!game.app_id)continue;
    choices.set(gameIdentity(game),{launcher:game.launcher,app_id:String(game.app_id),...(game.name?{name:game.name}:{})});
  }
  dialog.innerHTML=`<h2>Игры и приложения · ${esc(mod.manifest.name)}</h2><p class="muted">Выбери, в каких играх будет доступен этот мод. Привязка добавляет переход к моду; возможности определяет сам мод.</p><label>Найти игру или приложение<input id="mod-binding-search" type="search"></label><div class="mod-binding-list">${[...choices.values()].sort((a,b)=>(a.name||a.app_id).localeCompare(b.name||b.app_id,getLocale())).map(game=>`<label><input type="checkbox" value="${esc(JSON.stringify(game))}" ${selected.has(gameIdentity(game))?'checked':''}><span>${esc(game.name||game.app_id)}<small class="muted">${esc(PLATFORMS[game.launcher]?.name||game.launcher)} · ${esc(game.app_id)}</small></span></label>`).join('')||'<p class="muted">Игры пока не найдены. Сначала добавь их в библиотеку.</p>'}</div><div class="mod-actions">${button('save-bindings','Сохранить привязку',id)}${button('reset-bindings','Как указал автор',id)}</div><form method="dialog"><button class="secondary">Закрыть</button></form>`;
  dialog.querySelector('#mod-binding-search').addEventListener('input',e=>{
    const query=e.target.value.trim().toLocaleLowerCase(getLocale());
    dialog.querySelectorAll('.mod-binding-list label').forEach(label=>label.hidden=!label.textContent.toLocaleLowerCase(getLocale()).includes(query));
  });
  dialog.showModal();
}
export function setupMods(options) {
  host=options;
  document.addEventListener('click',e=>{if(e.target.closest('[data-page]')){gameScope=null;frameContext=null;frameKey='';if(e.target.closest('[data-page="mods"]'))filters=defaultModFilters();}},true);
  document.addEventListener('click',async e=>{
    const b=e.target.closest('[data-mod-action],[data-mod-operation]');if(!b)return;
    b.disabled=true;const id=b.dataset.modId;
    try{
      if(b.dataset.modOperation){const platform=b.dataset.modPlatform,op=b.dataset.modOperation;await modOperation(platform,op,b.dataset.account?{account_id:b.dataset.account}:{});await host.changed();return;}
      const action=b.dataset.modAction;
      if(action==='game-mods'){
        gameScope=host.games().find(g=>host.gameKey(g)===b.dataset.game)||null;if(!gameScope)return;
        filters={...defaultModFilters(),game:gameIdentity(gameScope)};frameContext=null;host.navigate('mods');await updateCatalog();host.render();return;
      }
      if(action==='all-mods'||action==='reset-filters'){filters=defaultModFilters();gameScope=null;frameContext=null;applyFilters(document.getElementById('mods-view'));return;}
      if(action==='back-mods'){host.navigate('mods');return;}
      if(action==='back-game'){const game=host.games().find(g=>gameIdentity(g)===filters.game);if(game)host.openGame(host.gameKey(game));return;}
      if(action==='open'){frameContext=gameContext();frameKey='';host.navigate(`mod-tab:${id}:${b.dataset.modTab}`);return;}
      if(action==='bindings'){bindingDetails(id);return;}
      if(action==='save-bindings'||action==='reset-bindings'){
        const dialog=document.getElementById('mod-bindings');
        const games=action==='reset-bindings'?null:[...dialog.querySelectorAll('input:checked')].map(input=>JSON.parse(input.value));
        await host.api(`/api/mods/${enc(id)}/games`,{games});dialog.close();
      }
      if(action==='catalog'){await updateCatalog(true);await host.render();return;}
      if(action==='install'){if(!trusted())return;await host.api('/api/mods/install',{id,trusted:true});}
      if(action==='toggle')await host.api(`/api/mods/${enc(id)}/toggle`,{enabled:b.dataset.enabled==='true'});
      if(action==='restart')await host.api(`/api/mods/${enc(id)}/restart`,{});
      if(action==='remove'){if(!confirm('Удалить мод? Его настройки и данные сохранятся.'))return;await host.api(`/api/mods/${enc(id)}/remove`,{});}
      if(action==='theme')await host.api('/api/mods/theme',{id});
      if(action==='reset-theme')await host.api('/api/mods/theme',{});
      if(action==='logs'||action==='settings'){await details(id,action);return;}
      if(action==='save-settings'){await host.api(`/api/mods/${enc(id)}/settings`,JSON.parse(document.getElementById('mod-settings-json').value));document.getElementById('mod-details').close();}
      if(action==='run')await host.api(`/api/mods/${enc(id)}/sdk`,{method:'action',params:{id:b.dataset.contribution,input:gameScope?gameContext():{}}});
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
