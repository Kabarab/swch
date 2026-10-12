import {spawn} from 'node:child_process';
import {fileURLToPath} from 'node:url';
export default async function activate(swch){
  const accounts=async()=>await swch.storage.get('accounts')||[];
  const library=async()=>[{id:'demo',app_id:'demo',name:'SWCH Mod Demo',is_installed:true,owner_ids:(await accounts()).map(a=>a.id)}];
  swch.registerLauncher({id:'demo',name:'Demo Launcher',capabilities:['library','sync','accounts','connect','disconnect','switch','launch']},{
    library,sync:library,accounts,
    async connect(){const rows=await accounts();const id=rows.some(a=>a.id==='player-one')?'player-two':'player-one';if(!rows.some(a=>a.id===id))rows.push({id,username:id,display_name:id==='player-one'?'Первый игрок':'Второй игрок',is_active:rows.length===0});await swch.storage.set('accounts',rows);return {message:'Демо-аккаунт подключён'};},
    async disconnect({account_id}){const rows=(await accounts()).filter(a=>a.id!==account_id);if(rows.length&&!rows.some(a=>a.is_active))rows[0].is_active=true;await swch.storage.set('accounts',rows);},
    async switch({account_id}){const rows=await accounts();if(!rows.some(a=>a.id===account_id))throw new Error('Unknown account');for(const a of rows)a.is_active=a.id===account_id;await swch.storage.set('accounts',rows);},
    async launch({game_id,account_id}){if(game_id!=='demo')throw new Error('Unknown game');await swch.storage.set('last-launch',{game_id,account_id});const child=spawn(process.execPath,[fileURLToPath(new URL('./demo-game.mjs',import.meta.url))],{stdio:'ignore',windowsHide:true});await new Promise((resolve,reject)=>{child.once('spawn',resolve);child.once('error',reject);});return {message:'Демонстрационная игра запущена отдельным процессом.'};},
  });
}
