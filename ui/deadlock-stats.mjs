import {DEADLOCK_HEROES,DEADLOCK_RANKS} from './deadlock-heroes.mjs?v=stats-20261011-1';
export const deadlockHeroName=id=>DEADLOCK_HEROES.find(h=>String(h.id)===String(id))?.name||'Персонаж #'+id;
export const deadlockRankLabel=badge=>{const tier=Math.floor(badge/10),subrank=badge%10;return DEADLOCK_RANKS[tier]&&(subrank>=1&&subrank<=6)?DEADLOCK_RANKS[tier]+' '+['','I','II','III','IV','V','VI'][subrank]:'';};
export function deadlockOverview(detail) {
  const heroes=detail?.heroes||[],requirements=detail?.requirements;
  const played=heroes.filter(h=>h.ranked_matches>0);
  const eligible=requirements?heroes.filter(h=>typeof h.standard_wins==='number'&&h.standard_wins>=requirements.hero_wins):[];
  const met=Boolean(requirements&&typeof detail.standard_wins==='number'&&detail.standard_wins>=requirements.total_wins&&eligible.length>=requirements.heroes);
  return {played,eligible,met,status:played.length?'Есть рейтинговые матчи':met?'Условия выполнены по статистике':'Доступ не подтверждён'};
}
export function deadlockHeroAccess(hero,requirements) {
  if(hero?.ranked_matches>0)return 'Открыт по рейтинговым матчам';
  if(requirements&&typeof hero?.standard_wins==='number'&&hero.standard_wins>=requirements.hero_wins)return 'Требование по победам выполнено';
  return 'Доступ не подтверждён';
}
