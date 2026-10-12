import {metricKey} from './model.mjs';

const knownSeconds=value=>typeof value==='number'&&Number.isFinite(value)&&value>=0;
export function gamePlaytime(game,{accounts=[],steamRows=[],riotProfiles=[],records=[],metrics={},accountId=null,demo=false}={}) {
  let rows=[];
  const app=String(game.app_id);
  if(game.launcher==='steam'&&!demo)rows=steamRows.filter(r=>String(r.app_id)===app&&knownSeconds(r.playtime_minutes)).map(r=>({account_id:r.account_id,seconds:r.playtime_minutes*60,scope:'lifetime',source:'Steam'}));
  else if(game.launcher==='riot')rows=riotProfiles.filter(p=>p.app_id===game.app_id&&knownSeconds(p.playtime?.value?.seconds)).map(p=>({account_id:p.account_id,seconds:p.playtime.value.seconds,scope:'recent_matches',source:'Riot · доступная история матчей',cached:Boolean(p.playtime.error)}));
  else rows=records.filter(r=>r.launcher===game.launcher&&String(r.app_id)===app&&knownSeconds(r.seconds));
  if(rows.some(r=>r.scope!=='galaxy_local'))rows=rows.filter(r=>r.scope!=='galaxy_local');
  // Support explicit playtime fields supplied by a library without guessing units.
  if(!rows.length&&game.launcher!=='steam'&&game.launcher!=='riot') {
    const seconds=knownSeconds(game.playtime_seconds)?game.playtime_seconds:knownSeconds(game.playtime_minutes)?game.playtime_minutes*60:null;
    if(seconds!==null)rows=[{seconds,account_id:game.account_id||game.owner_id||null,scope:game.playtime_scope||'lifetime',source:game.playtime_source||'Данные платформы'}];
  }
  if(accountId!==null)rows=rows.filter(r=>r.account_id===accountId);
  const unique=new Map();
  for(const row of rows) {
    const key=row.identity||row.account_id||'local';
    // Multiple library entries are snapshots of the same total, not additional sessions.
    if(!unique.has(key)||row.seconds>unique.get(key).seconds)unique.set(key,row);
  }
  const available=[...unique.values()];
  if(available.length) {
    const scopes=[...new Set(available.map(r=>r.scope))];
    return {seconds:available.reduce((n,r)=>n+r.seconds,0),scope:scopes.length===1?scopes[0]:'partial',source:[...new Set(available.map(r=>r.source))].join(' · '),accounts:unique.size,cached:available.some(r=>r.cached)};
  }
  // Manual values remain a fallback and never overwrite an imported total.
  const candidates=accounts.filter(a=>a.launcher===game.launcher&&(accountId===null||a.id===accountId)&&(!game.owner_ids?.length||game.owner_ids.includes(a.id))&&(!game.owner_id||game.owner_id===a.id));
  const manual=candidates.map(a=>metrics[metricKey(game,a)]?.hours).filter(knownSeconds);
  return manual.length?{seconds:manual.reduce((n,h)=>n+h*3600,0),scope:'manual',source:demo?'Пример данных':'Введено вручную',accounts:manual.length,cached:false}:null;
}
export function timeLabel(time) {
  if(!time)return '';
  if(time.seconds>0&&time.seconds<60)return '< 1 мин';
  if(time.seconds>0&&time.seconds<360)return new Intl.NumberFormat(getLocale(),{maximumFractionDigits:1}).format(time.seconds/60)+' '+t('мин');
  return new Intl.NumberFormat(getLocale(),{maximumFractionDigits:1}).format(time.seconds/3600)+' '+t('ч');
}
export function timeScopeLabel(time) {
  return ({recent_matches:'по матчам',local_worlds:'в локальных мирах',galaxy_local:'в GALAXY',manual:'вручную',partial:'доступные данные'})[time?.scope]||'';
}
import {t,getLocale} from './i18n.js';
