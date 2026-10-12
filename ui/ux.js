import {PLATFORMS, platformNavigation} from './platforms.mjs?v=mods-1';

export function platformLogo(id) {
  const platform=PLATFORMS[id];
  return platform?`<span class="platform-mark platform-logo"><img src="${platform.logo}" width="24" height="24" alt=""></span>`:'';
}
export function hydratePlatformLogos(root=document) {
  root.querySelectorAll('[data-platform], [data-tab], [data-inventory-launcher]').forEach(button=>{
    const id=button.dataset.platform||button.dataset.tab||button.dataset.inventoryLauncher;
    if(!PLATFORMS[id]||button.querySelector('.platform-logo'))return;
    const mark=button.querySelector('.platform-mark');
    if(mark)mark.outerHTML=platformLogo(id);
    else button.insertAdjacentHTML('afterbegin',platformLogo(id));
  });
}
export function renderPlatformNavigation(games,accounts,page,selected) {
  const entries=platformNavigation(games,accounts,page);
  for(const {id,count,visible} of entries) {
    const button=document.querySelector(`.sidebar [data-platform="${id}"]`);
    if(button)button.hidden=!visible;
    const counter=document.getElementById(id+'-count');if(counter)counter.textContent=count;
  }
  const select=document.getElementById('mobile-platform');
  select.closest('label').hidden=page==='inventory';
  select.replaceChildren(new Option('Все платформы','all'),...entries.filter(p=>p.visible).map(p=>new Option(`${PLATFORMS[p.id].name} · ${p.count}`,p.id)));
  select.value=entries.some(p=>p.visible&&p.id===selected)?selected:'all';
  hydratePlatformLogos();
}

const disclosures=new Map();
document.addEventListener('toggle',event=>{
  if(event.target.matches('details[data-disclosure]'))disclosures.set(event.target.dataset.disclosure,event.target.open);
},true);
function disclosure(key,label,content,className='') {
  const details=document.createElement('details');details.className='ux-disclosure '+className;
  details.dataset.disclosure=key;details.innerHTML=`<summary>${label}</summary>`;
  details.open=disclosures.get(key)||false;
  content.forEach(el=>{if(el)details.append(el);});
  return details;
}
export function simplifyInventory(root,{hasItems,complete}) {
  root.classList.add('inventory-workspace');
  const toolbar=root.querySelector('.inventory-toolbar');if(!toolbar)return;
  const shortcuts=root.querySelector('.steam-game-logos-bar');
  if(shortcuts){const d=disclosure('inventory-games','Другие игры с предметами',[shortcuts]);root.querySelector('.inventory-bottom-info')?.append(d);}
  [...root.children].filter(el=>el.matches('small.muted')).forEach(el=>el.remove());
  if(!complete&&hasItems){const note=document.createElement('span');note.className='inventory-partial';note.textContent='Загружена часть инвентаря';toolbar.append(note);}
  const actions=root.querySelector('.inventory-actions');
  if(actions){
    const more=actions.querySelector('[data-action="inventory-more"]');
    if(more){more.classList.add('inventory-more');root.querySelector('#inventory-items')?.after(more);}
    const all=actions.querySelector('[data-action="inventory-all"]');
    if(all){const d=disclosure('inventory-loading','Параметры загрузки',[all]);root.querySelector('.inventory-bottom-info')?.prepend(d);}
    toolbar.after(actions);
  }
  const setting=root.querySelector('.inventory-setting-box');
  const value=root.querySelector('#inventory-valuation');
  const existing=root.querySelector('.cs-valuation-details');
  if(existing){existing.classList.add('ux-disclosure');existing.dataset.disclosure='inventory-value';existing.open=disclosures.get('inventory-value')||false;if(setting)existing.append(setting);root.querySelector('#inventory-items')?.after(existing);}
  else if(value){const d=disclosure('inventory-value','Стоимость и параметры оценки',[value,setting]);root.querySelector('#inventory-items')?.after(d);}
  const pricing=actions?.querySelector('[data-action="inventory-value"]');
  if(pricing)root.querySelector('[data-disclosure="inventory-value"]')?.append(pricing);
  if(actions&&!actions.children.length)actions.remove();
  // Keep errors above items even when source diagnostics are collapsed.
  const status=root.querySelector('.inventory-account-status');
  if(status){
    const errors=new Set([...status.querySelectorAll('[role="alert"], .error-text')].map(el=>el.textContent));
    errors.forEach(text=>{const p=document.createElement('p');p.className='inventory-warning';p.setAttribute('role','alert');p.textContent=text;toolbar.after(p);});
    const d=disclosure('inventory-status','Состояние загрузки по аккаунтам',[status]);root.querySelector('.inventory-bottom-info')?.append(d);
  }
  const note=root.querySelector('.inventory-note');
  if(note){const d=disclosure('inventory-help','Как считается стоимость',[note]);root.querySelector('.inventory-bottom-info')?.append(d);}
  const advanced=root.querySelector('.cs-advanced-filters');
  if(advanced){advanced.classList.add('ux-disclosure');advanced.dataset.disclosure='inventory-filters';if(!advanced.open)advanced.open=disclosures.get('inventory-filters')||false;}
  else if(hasItems){
    const secondary=[...root.querySelectorAll('.inventory-toolbar')].find(el=>el!==toolbar);
    const search=secondary?.querySelector('#inventory-search')?.closest('label');
    if(search)toolbar.append(search);
    const facets=root.querySelector('.inventory-facets');
    const tags=root.querySelector('.inventory-tag-filter-box');
    if(secondary){const d=disclosure('inventory-filters','Дополнительные фильтры',[secondary,facets,tags]);root.querySelector('#inventory-items')?.before(d);}
  }
  const summary=root.querySelector('.inventory-summary');
  if(summary){summary.classList.add('inventory-summary-compact');summary.querySelector('h3')?.remove();}
  hydratePlatformLogos(root);
}

export function simplifyRiotInventory(root) {
  const accounts=root.querySelector('.riot-inventory-accounts');
  const heading=root.querySelector('.riot-inventory-heading');
  if(accounts){const d=disclosure('riot-inventory-accounts','Коллекции по аккаунтам',[accounts]);heading?heading.replaceWith(d):root.append(d);}
  hydratePlatformLogos(root);
}
