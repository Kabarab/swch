/** @param {import('../../index.d.ts').SDK} swch */
export default async function activate(swch){
  swch.registerTab({id:'library',name:'Обзор библиотеки'});
  swch.registerAction({id:'count',name:'Увеличить счётчик'},async()=>{
    const count=Number(await swch.storage.get('count')||0)+1;
    await swch.storage.set('count',count);swch.events.emit('count.changed',count);return count;
  });
}
