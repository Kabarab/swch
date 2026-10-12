import test from 'node:test';
import assert from 'node:assert/strict';
import {filterGames, inventoryHeroItems, inventoryTagCounts, csInventoryCategories} from '../../ui/model.mjs';

test('favorites lead the library; each group sorts by installation, descending playtime and name',()=>{
 const games=[
  {launcher:'steam',app_id:'1',name:'Zulu',is_installed:true},
  {launcher:'steam',app_id:'2',name:'Alpha',is_installed:true},
  {launcher:'epic',app_id:'3',name:'Uninstalled favorite',is_installed:false},
  {launcher:'steam',app_id:'4',name:'Beta',is_installed:true},
  {launcher:'epic',app_id:'5',name:'Unplayed',is_installed:false},
 ];
 const sorted=filterGames(games,{favoriteKeys:['epic:3'],playtime:{'steam:1':60,'steam:2':60,'steam:4':120,'epic:3':10000}});
 assert.deepEqual(sorted.map(g=>g.app_id),['3','4','2','1','5']);
 assert.deepEqual(filterGames(games.slice(0,2),{playtime:{'steam:1':null,'steam:2':0}}).map(g=>g.app_id),['2','1']);
 assert.equal(games[0].app_id,'1','Sorting does not mutate the input');
});

const item=(id,hero,tags,amount=1)=>({asset_id:id,name:id,app_id:'570',amount,tags,tag_details:[{category:'Hero',value:hero,name:hero}]});
test('Dota type and quality counts include only the selected heroes, with OR for multiselect',()=>{
 const items=[item('one','pudge',['pudge','Оружие','Immortal'],2),item('two','axe',['axe','Голова','Rare']),item('three','pudge',['pudge','Оружие','Rare'])];
 const selected=inventoryHeroItems(items,{'570:Hero':['pudge'],'570:Rarity':['unrelated']});
 assert.deepEqual(selected.map(i=>i.asset_id),['one','three']);
 assert.deepEqual([...inventoryTagCounts(selected)], [['Оружие',3],['Immortal',2],['Rare',1]]);
 assert.equal(inventoryHeroItems(items,{'570:Hero':['axe','pudge']}).length,3);
 assert.equal(inventoryHeroItems(items,{}).length,3);
});

test('CS categories use real item images and preserve the exact Steam type facet',()=>{
 const items=[
  {app_id:'730',amount:2,icon_url:'https://example.com/rifle.png',tag_details:[{category:'Type',value:'Rifle',name:'Винтовка'}]},
  {app_id:'730',amount:1,icon_url:'javascript:bad',tag_details:[{category:'Type',value:'Rifle',name:'Винтовка'}]},
  {app_id:'570',amount:10,tag_details:[{category:'Type',value:'Rifle',name:'Other game'}]},
 ];
 assert.deepEqual(csInventoryCategories(items),[{key:'730:Type',value:'Rifle',name:'Винтовка',count:3,image:'https://example.com/rifle.png'}]);
});
