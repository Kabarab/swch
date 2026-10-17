import {createTranslator, validatePack, validateIndex, direction, intlLocale, sha256, canonicalMessages, DEFAULT_LOCALE, SYSTEM_LANGUAGE, systemLocale} from './i18n-core.mjs';

const BASE = '/ui/lang/';
const REMOTE = 'https://raw.githubusercontent.com/Kabarab/swch/language-packs/';
let source, index, active = DEFAULT_LOCALE, preference = SYSTEM_LANGUAGE, systemLanguages = [], machine = false, translate = value => String(value ?? ''), changing = false;
const originals = new WeakMap();
const ATTRIBUTES = ['title','aria-label','placeholder','alt'];
const SKIP = 'script,style,textarea,code,pre,[translate="no"],[data-i18n-skip],[data-mod-tab-nav],[data-mod-platform-nav],[data-contribution],.game-name,.poster-name,.poster-platform,.poster-art,.game-modal-poster-img,.roblox-item-name,.account-identity,.account-name,.hero-chip-name,.fortnite-card-title,.card-caption h3,.hero-copy h2,.account-row-identity h3,.account-info strong,.account-selector strong,.launch-account-options strong,.mod-card-heading h3,#game-modal-title,.game-list strong,.account-valuation h3,#steam-account-filter option:not([value="all"]),#gog-account-filter option:not([value="all"])';
export const t = value => translate(value);
export const getLocale = () => intlLocale(active);
export const numberFormat = options => new Intl.NumberFormat(getLocale(),options);
export const localizedConfirm = message => window.confirm(t(message));
function savedLocale() { try { return localStorage.getItem('swch-language'); } catch { return null; } }
function remember(locale) { try { localStorage.setItem('swch-language',locale); } catch {} }
function requestedLocale(value) { return value === SYSTEM_LANGUAGE ? systemLocale(systemLanguages,index.languages) : value; }
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
async function boundedJson(url, max = 4000000, timeout = 45000) {
  const response = await fetch(url,{signal:AbortSignal.timeout(timeout),cache:'no-cache'});
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
async function loadPack(locale,allowInstalled=false,timeout=45000) {
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
    const local=await verified((await boundedJson(`/api/i18n/packs/${encodeURIComponent(locale)}`,4000000,timeout)).value);
    await cached(locale,{pack:local,sha256:entry.sha256,messagesSha256:entry.messagesSha256}).catch(()=>{});return local;
  }catch{}
  let lastError;
  // Try the bundled copy, then the pinned published file. This also permits
  // corrected packs for the same source revision without rebuilding SWCH.
  for(const url of [BASE+`packs/${encodeURIComponent(locale)}.json`,REMOTE+`${source.revision}/${encodeURIComponent(locale)}-${entry.sha256}.json`]){
    try {
      const download=await boundedJson(url,4000000,timeout);
      if(await sha256(download.bytes)!==entry.sha256)throw new Error('checksum');
      const pack=await verified(download.value);
      await cached(locale,{pack,sha256:entry.sha256,messagesSha256:entry.messagesSha256}).catch(()=>{});
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
async function persist(pack, choice) {
  const response=await fetch('/api/i18n/selection',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:pack.locale,preference:choice,revision:pack.revision,machine:pack.machine,messages:pack.messages}),signal:AbortSignal.timeout(10000)});
  if(!response.ok)throw new Error('save');
}
async function choose(choice) {
  if(changing)return;
  changing=true;
  const status=document.getElementById('language-status'),button=document.getElementById('language-apply');
  if(button)button.disabled=true;
  if(status)status.textContent='Загрузка языкового пакета…';
  try {
    const pack=await loadPack(requestedLocale(choice));
    // Native startup messages use the same pack, persisted by the local backend.
    await persist(pack,choice);
    preference=choice;remember(choice);
    apply(pack);if(status)status.textContent='Язык сохранён';
    renderOptions();
  } catch {
    if(status)status.textContent='Не удалось загрузить язык. Текущий язык сохранён.';
  } finally {changing=false;if(button)button.disabled=false;}
}
function renderOptions() {
  const select=document.getElementById('language-select');if(!select)return;
  const search=document.getElementById('language-search')?.value.trim().toLowerCase()||'';
  const selected=select.value||preference;
  const names=new Intl.DisplayNames([getLocale()],{type:'language',fallback:'none'});
  const options=index.languages.map(item=>{
    let display;try{display=names.of(intlLocale(item.locale));}catch{}
    return {...item,label:`${item.nativeName} · ${display||item.name}`,ready:item.locale==='ru'||(item.sha256&&item.revision===source.revision)};
  }).filter(item=>item.ready && (item.locale===active || `${item.label} ${item.name} ${item.locale}`.toLowerCase().includes(search)));
  const automatic=document.createElement('option');automatic.value=SYSTEM_LANGUAGE;automatic.setAttribute('translate','no');
  const systemTag=systemLanguages[0];let systemName;try{systemName=names.of(intlLocale(systemTag));}catch{}
  automatic.textContent=`${t('По умолчанию')} · ${systemName||systemTag||'English'}`;
  select.replaceChildren(automatic,...options.map(item=>{
    const option=document.createElement('option');option.value=item.locale;option.textContent=item.label;option.setAttribute('translate','no');return option;
  }));
  select.value=selected===SYSTEM_LANGUAGE||options.some(item=>item.locale===selected)?selected:preference;
  if(!select.value&&options.length)select.value=options[0].locale;
}
function controls() {
  const select=document.getElementById('language-select');if(!select)return;
  renderOptions();select.value=preference;
  document.getElementById('language-search')?.addEventListener('input',renderOptions);
  document.getElementById('language-apply')?.addEventListener('click',()=>choose(select.value));
}
export async function initI18n() {
  try {
    [source,index]=await Promise.all([boundedJson(BASE+'source.json').then(x=>x.value),boundedJson(BASE+'index.json').then(x=>x.value)]);
    index=validateIndex(index,source);
    try {const previous=await cached('@index');if(previous)index=validateIndex(previous,source);}catch{}
    let saved;
    try {const response=await fetch('/api/i18n/selection',{signal:AbortSignal.timeout(2500)});if(response.ok)saved=await response.json();}catch{}
    systemLanguages=Array.isArray(saved?.systemLanguages)&&saved.systemLanguages.length?saved.systemLanguages:(navigator.languages?.length?[...navigator.languages]:[navigator.language]);
    const remoteChoice=saved?.preference || (saved?.saved!==false?saved?.locale:null);
    preference=(saved?.saved===true?remoteChoice:null)||savedLocale()||remoteChoice||SYSTEM_LANGUAGE;
    const startupPreference=preference, locale=requestedLocale(preference);
    // English is bundled and immediately usable while an optional system pack
    // downloads. A failed download must never restore Russian by accident.
    let pack=locale==='ru'?await loadPack('ru'):await loadPack(DEFAULT_LOCALE,true,6000);
    apply(pack);controls();document.documentElement.classList.remove('i18n-loading');
    if(locale!==pack.locale){
      try {const requested=await loadPack(locale,true,6000);if(!changing&&preference===startupPreference){pack=requested;apply(pack);renderOptions();}}catch{}
    }
    if(!changing&&preference===startupPreference&&(saved?.locale!==pack.locale||remoteChoice!==preference||saved?.revision!==pack.revision)){
      await persist(pack,preference).catch(()=>{});
    }
    // Other windows update from their already cached packs.
    window.addEventListener('storage',event=>{if(event.key==='swch-language'){preference=event.newValue||SYSTEM_LANGUAGE;void loadPack(requestedLocale(preference)).then(pack=>{apply(pack);renderOptions();const select=document.getElementById('language-select');if(select)select.value=preference;}).catch(()=>{});}});
    // Refresh in the background. Offline startup uses the cached catalog and
    // installed translation; additional languages need no new SWCH release.
    void boundedJson(REMOTE+'index.json').then(async download=>{
      const next=validateIndex(download.value,source);await cached('@index',next);index=next;renderOptions();
    }).catch(()=>{});
  } catch { /* Keep the window available if the bundled assets are damaged. */ }
  finally {document.documentElement.classList.remove('i18n-loading');}
}
