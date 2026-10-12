import test from 'node:test';
import assert from 'node:assert/strict';
import {deadlockOverview,deadlockHeroAccess,deadlockRankLabel} from '../../ui/deadlock-stats.mjs';
const requirements={total_wins:60,hero_wins:15,heroes:3};
test('Deadlock access distinguishes known ranked play, observed requirements and unknown partial history',()=>{
 assert.equal(deadlockOverview(null).status,'Доступ не подтверждён');
 const detail={requirements,standard_wins:60,heroes:[1,2,3].map(hero_id=>({hero_id,standard_wins:15,ranked_matches:0}))};
 assert.equal(deadlockOverview(detail).met,true);assert.equal(deadlockOverview(detail).eligible.length,3);
 assert.equal(deadlockOverview({...detail,standard_wins:59}).met,false);
 assert.equal(deadlockOverview({...detail,heroes:[{hero_id:1,standard_wins:null,ranked_matches:2}]}).status,'Есть рейтинговые матчи');
 assert.equal(deadlockHeroAccess({standard_wins:14,ranked_matches:0},requirements),'Доступ не подтверждён');
 assert.equal(deadlockHeroAccess({standard_wins:15},requirements),'Требование по победам выполнено');
 assert.equal(deadlockHeroAccess({ranked_matches:1},null),'Открыт по рейтинговым матчам');
 assert.equal(deadlockRankLabel(75),'Emissary V');assert.equal(deadlockRankLabel(0),'');
});
