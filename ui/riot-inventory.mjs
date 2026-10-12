import {VALORANT_WEAPONS} from './valorant-weapons.mjs';

export const WEAPON_CATEGORIES=Object.freeze({sidearm:'Пистолеты',smg:'Пистолеты-пулемёты',rifle:'Винтовки',shotgun:'Дробовики',sniper:'Снайперские винтовки',heavy:'Пулемёты',melee:'Ножи',other:'Прочее оружие'});
const weaponIds=new Map(VALORANT_WEAPONS.flatMap(weapon=>weapon.skins.map(id=>[id,weapon])));
export const riotKinds=app=>app==='valorant'?['skins','buddies','champions']:['skins','champions'];
export const riotKindLabel=(kind,app)=>({all:'Все предметы',skins:'Скины',buddies:'Брелоки',champions:app==='valorant'?'Агенты':'Чемпионы'})[kind]||'Все предметы';

export function weaponForSkin(item) {
  const known=weaponIds.get(String(item.id).toLowerCase());
  if(known)return known;
  // Older snapshots contain English catalog names even when metadata is absent.
  // Unknown names/IDs remain unclassified rather than being guessed as knives.
  const name=String(item.name||'').trim().toLowerCase();
  return VALORANT_WEAPONS.find(w=>name===w.name.toLowerCase()||name.endsWith(' '+w.name.toLowerCase()))||null;
}

export function riotInventoryData(profiles,{accounts=[],app='valorant',account='all',kind='all',search='',category='',weapon=''}={}) {
  const selected=profiles.filter(p=>p.app_id===app&&(account==='all'||p.account_id===account));
  const kinds=kind==='all'?riotKinds(app):riotKinds(app).filter(k=>k===kind);
  const raw=selected.flatMap(p=>kinds.flatMap(k=>(p[k]?.value||[]).map(item=>({...item,kind:k,account:p.display_name,account_id:p.account_id,owners:[{id:p.account_id,name:p.display_name}],weapon:app==='valorant'&&k==='skins'?weaponForSkin(item):null}))));
  const rows=[], heroes=new Map();
  for(const row of raw) {
    if(row.kind!=='champions'){rows.push(row);continue;}
    // Identity and product, not display text, determine a shared hero.
    const key=String(row.id);
    const existing=heroes.get(key);
    if(existing) {
      if(!existing.owners.some(a=>a.id===row.account_id))existing.owners.push(...row.owners);
      existing.image_url ||= row.image_url;
    } else {heroes.set(key,row);rows.push(row);}
  }
  const compare=new Intl.Collator(getLocale(),{numeric:true,sensitivity:'base'});
  rows.sort((a,b)=>riotKinds(app).indexOf(a.kind)-riotKinds(app).indexOf(b.kind)||compare.compare(String(a.name),String(b.name)));
  const skins=selected.flatMap(p=>(p.skins?.value||[]).map(item=>({...item,weapon:weaponForSkin(item)})));
  const categories=Object.entries(WEAPON_CATEGORIES).map(([id,name])=>{
    const items=skins.filter(item=>(item.weapon?.category||'other')===id);
    return {id,name,count:items.length,image:items.find(item=>item.image_url)?.image_url};
  }).filter(c=>c.count>0);
  const weapons=VALORANT_WEAPONS.filter(w=>(!category||w.category===category)&&skins.some(item=>item.weapon?.id===w.id));
  const term=search.trim().toLocaleLowerCase();
  const filtered=rows.filter(item=>(!term||String(item.name).toLocaleLowerCase().includes(term))&&(!category||(item.kind==='skins'&&(item.weapon?.category||'other')===category))&&(!weapon||(item.kind==='skins'&&item.weapon?.id===weapon)));
  const pending=accounts.filter(a=>a.launcher==='riot'&&(account==='all'||a.id===account)).flatMap(a=>kinds.filter(k=>!selected.some(p=>p.account_id===a.id&&p[k]?.value!=null)).map(kind=>({account_id:a.id,kind})));
  const errors=selected.flatMap(p=>kinds.filter(k=>p[k]?.error).map(kind=>({account:p.display_name,kind,message:p[kind].error,cached:p[kind].value!=null})));
  const sections=kinds.map(kind=>({kind,name:riotKindLabel(kind,app),items:filtered.filter(item=>item.kind===kind)})).filter(section=>section.items.length);
  return {rows,filtered,sections,skins,categories,weapons,pending,errors,known:selected.some(p=>kinds.some(k=>p[k]?.value!=null))};
}
import {getLocale} from './i18n.js';
