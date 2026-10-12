import {createTranslator, validatePack, validateIndex, direction, intlLocale, sha256, canonicalMessages} from './i18n-core.mjs';

const BASE = '/ui/lang/';
const REMOTE = 'https://raw.githubusercontent.com/Kabarab/swch/language-packs/';
let source, index, active = 'ru', machine = false, translate = value => String(value ?? ''), changing = false;
const originals = new WeakMap();
const ATTRIBUTES = ['title','aria-label','placeholder','alt'];
const SKIP = 'script,style,textarea,code,pre,[translate="no"],[data-i18n-skip],[data-mod-tab-nav],[data-mod-platform-nav],[data-contribution],.game-name,.poster-name,.poster-platform,.poster-art,.game-modal-poster-img,.roblox-item-name,.account-identity,.account-name,.hero-chip-name,.fortnite-card-title,.card-caption h3,.hero-copy h2,.account-row-identity h3,.account-info strong,.account-selector strong,.launch-account-options strong,.mod-card-heading h3,#game-modal-title,.game-list strong,.account-valuation h3,#steam-account-filter option:not([value="all"]),#gog-account-filter option:not([value="all"])';
export const t = value => translate(value);
export const getLocale = () => intlLocale(active);
export const numberFormat = options => new Intl.NumberFormat(getLocale(),options);
export const localizedConfirm = message => window.confirm(t(message));
function savedLocale() { try { return localStorage.getItem('swch-language') || 'ru'; } catch { return 'ru'; } }
function db() {
  return new Promise((resolve,reject)=>{
    const request = indexedDB.open('swch-languages',1);
    request.onupgradeneeded = ()=>request.result.createObjectStore('packs');
    request.onsuccess = ()=>resolve(request.result); request.onerror = ()=>reject(request.error);
  });
}
async function cached(locale, pack) {
  const database = await db();
  try {
    return await new Promise((resolve,reject)=>{
      const transaction = database.transaction('packs',pack?'readwrite':'readonly');
      const store = transaction.objectStore('packs');
      const request = pack ? store.put(pack,locale) : store.get(locale);
      let result;
      request.onsuccess = ()=>{result=request.result;};
      transaction.oncomplete = ()=>resolve(result);
      transaction.onerror = transaction.onabort = ()=>reject(transaction.error || new Error('cache'));
    });
  } finally { database.close(); }
}
async function boundedJson(url, max = 4000000) {
  const response = await fetch(url,{signal:AbortSignal.timeout(45000),cache:'no-cache'});
  if (!response.ok) throw new Error('download');
  if (Number(response.headers.get('content-length')||0)>max) throw new Error('size');
  const reader = response.body.getReader(); let size = 0; const chunks = [];
  try {
    for (;;) { const {done,value}=await reader.read();if(done)break;size+=value.byteLength;if(size>max)throw new Error('size');chunks.push(value); }
  } finally { await reader.cancel().catch(()=>{}); }
  const bytes = new Uint8Array(size); let offset=0;
  for (const chunk of chunks) {bytes.set(chunk,offset);offset+=chunk.length;}
  return {value:JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(bytes)),bytes};
}
async function loadPack(locale,allowInstalled=false) {
  if(locale==='ru')return {schema:1,locale:'ru',revision:source.revision,machine:false,messages:source.messages};
  const entry=index.languages.find(item=>item.locale===locale);
  if(!entry?.sha256 || entry.revision!==source.revision)throw new Error('unavailable');
  const verified = async value => {
    const pack=validatePack(value,source,locale);
    if(pack.machine!==entry.machine||await sha256(new TextEncoder().encode(canonicalMessages(pack.messages)))!==entry.messagesSha256)throw new Error('checksum');
    return pack;
  };
  try {
    const record=await cached(locale);
    if(record?.sha256===entry.sha256)return await verified(record.pack);
    if(allowInstalled&&record?.messagesSha256){
      const pack=validatePack(record.pack,source,locale);
      if(await sha256(new TextEncoder().encode(canonicalMessages(pack.messages)))===record.messagesSha256)return pack;
    }
  }catch{}
  try {
    const local=await verified((await boundedJson(`/api/i18n/packs/${encodeURIComponent(locale)}`)).value);
    await cached(locale,{pack:local,sha256:entry.sha256,messagesSha256:entry.messagesSha256});return local;
  }catch{}
  let lastError;
  // Try the bundled copy, then the pinned published file. This also permits
  // corrected packs for the same source revision without rebuilding SWCH.
  for(const url of [BASE+`packs/${encodeURIComponent(locale)}.json`,REMOTE+`${source.revision}/${encodeURIComponent(locale)}-${entry.sha256}.json`]){
    try {
      const download=await boundedJson(url);
      if(await sha256(download.bytes)!==entry.sha256)throw new Error('checksum');
      const pack=await verified(download.value);
      await cached(locale,{pack,sha256:entry.sha256,messagesSha256:entry.messagesSha256});
      return pack;
    }catch(error){lastError=error;}
  }
  throw lastError;
}
function rewrite(node, attribute) {
  if(node.nodeType===Node.TEXT_NODE && node.parentElement?.closest(SKIP))return;
  if(node.nodeType===Node.ELEMENT_NODE && node.closest(SKIP))return;
  const current=attribute?node.getAttribute(attribute):node.nodeValue;
  if(current==null)return;
  let record=originals.get(node);
  if(!record){record=new Map();originals.set(node,record);}
  const key=attribute||'$text', previous=record.get(key);
  const original=previous && previous.output===current?previous.source:current;
  const output=t(original);
  record.set(key,{source:original,output});
  if(output!==current){if(attribute)node.setAttribute(attribute,output);else node.nodeValue=output;}
}
function localize(root=document.documentElement) {
  if(root.nodeType===Node.TEXT_NODE){rewrite(root);return;}
  if(root.nodeType!==Node.ELEMENT_NODE)return;
  if(root.closest(SKIP))return;
  for(const attribute of ATTRIBUTES)rewrite(root,attribute);
  const walker=document.createTreeWalker(root,NodeFilter.SHOW_ELEMENT|NodeFilter.SHOW_TEXT);
  while(walker.nextNode()){
    const node=walker.currentNode;
    if(node.nodeType===Node.TEXT_NODE)rewrite(node);
    else for(const attribute of ATTRIBUTES)rewrite(node,attribute);
  }
}
let observer;
function observe() {
  observer=new MutationObserver(records=>{
    observer.disconnect();
    for(const record of records){
      if(record.type==='characterData')rewrite(record.target);
      else if(record.type==='attributes')rewrite(record.target,record.attributeName);
      else for(const node of record.addedNodes)localize(node);
    }
    observer.observe(document.documentElement,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:ATTRIBUTES});
  });
  observer.observe(document.documentElement,{subtree:true,childList:true,characterData:true,attributes:true,attributeFilter:ATTRIBUTES});
}
function apply(pack) {
  observer?.disconnect();
  active=pack.locale;machine=pack.machine;translate=active==='ru'?value=>String(value??''):createTranslator(source,pack.messages);
  document.documentElement.lang=getLocale();document.documentElement.dir=direction(active);
  localize();observe();
  const badge=document.getElementById('language-machine');
  if(badge){badge.hidden=!machine;badge.textContent='Машинный перевод';}
  document.dispatchEvent(new CustomEvent('swch-language-change',{detail:{locale:active}}));
}
async function choose(locale) {
  if(changing)return;
  changing=true;
  const status=document.getElementById('language-status'),button=document.getElementById('language-apply');
  if(button)button.disabled=true;
  if(status)status.textContent='Загрузка языкового пакета…';
  try {
    const pack=await loadPack(locale);
    // Native startup messages use the same pack, persisted by the local backend.
    const response=await fetch('/api/i18n/selection',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale,revision:pack.revision,machine:pack.machine,messages:pack.messages}),signal:AbortSignal.timeout(10000)});
    if(!response.ok)throw new Error('save');
    localStorage.setItem('swch-language',locale);
    apply(pack);if(status)status.textContent='Язык сохранён';
    renderOptions();
  } catch {
    if(status)status.textContent='Не удалось загрузить язык. Текущий язык сохранён.';
  } finally {changing=false;if(button)button.disabled=false;}
}
function renderOptions() {
  const select=document.getElementById('language-select');if(!select)return;
  const search=document.getElementById('language-search')?.value.trim().toLowerCase()||'';
  const selected=select.value||active;
  const names=new Intl.DisplayNames([getLocale()],{type:'language',fallback:'none'});
  const options=index.languages.map(item=>{
    let display;try{display=names.of(intlLocale(item.locale));}catch{}
    return {...item,label:`${item.nativeName} · ${display||item.name}`,ready:item.locale==='ru'||(item.sha256&&item.revision===source.revision)};
  }).filter(item=>item.ready && (item.locale===active || `${item.label} ${item.name} ${item.locale}`.toLowerCase().includes(search)));
  select.replaceChildren(...options.map(item=>{
    const option=document.createElement('option');option.value=item.locale;option.textContent=item.label;option.setAttribute('translate','no');return option;
  }));
  select.value=options.some(item=>item.locale===selected)?selected:active;
  if(!select.value&&options.length)select.value=options[0].locale;
}
function controls() {
  const select=document.getElementById('language-select');if(!select)return;
  renderOptions();select.value=active;
  document.getElementById('language-search')?.addEventListener('input',renderOptions);
  document.getElementById('language-apply')?.addEventListener('click',()=>choose(select.value));
}
export async function initI18n() {
  try {
    [source,index]=await Promise.all([boundedJson(BASE+'source.json').then(x=>x.value),boundedJson(BASE+'index.json').then(x=>x.value)]);
    index=validateIndex(index,source);
    try {const previous=await cached('@index');if(previous)index=validateIndex(previous,source);}catch{}
    let locale=savedLocale();
    if(locale==='ru'){
      try {const response=await fetch('/api/i18n/selection',{signal:AbortSignal.timeout(2500)});const data=await response.json();if(response.ok && data.locale)locale=data.locale;}catch{}
    }
    let pack;try{pack=await loadPack(locale,true);}catch{pack=await loadPack('ru');}
    apply(pack);controls();
    // Other windows update from their already cached packs.
    window.addEventListener('storage',event=>{if(event.key==='swch-language'&&event.newValue)void loadPack(event.newValue).then(apply).catch(()=>{});});
    // Refresh in the background. Offline startup uses the cached catalog and
    // installed translation; additional languages need no new SWCH release.
    void boundedJson(REMOTE+'index.json').then(async download=>{
      const next=validateIndex(download.value,source);await cached('@index',next);index=next;renderOptions();
    }).catch(()=>{});
  } catch { /* The Russian UI remains available if the catalog itself is missing. */ }
  finally {document.documentElement.classList.remove('i18n-loading');}
}
