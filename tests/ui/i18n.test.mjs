import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import crypto from 'node:crypto';
import {createTranslator,validatePack,validateIndex,direction,intlLocale} from '../../ui/i18n-core.mjs';
import {MOD_PROMPT} from '../../ui/mod-prompt.mjs';
const source={revision:'v1',messages:{'Библиотека':'Библиотека','Запуск {0} для {1}':'Запуск {0} для {1}','Цена:':'Цена:'}};
const pack={schema:1,locale:'en',revision:'v1',machine:true,messages:{'Библиотека':'Library','Запуск {0} для {1}':'Launch {0} for {1}','Цена:':'Price:'}};
test('exact text, whitespace, reordered slots and opaque user data',()=>{
 const t=createTranslator(source,{...pack.messages,'Запуск {0} для {1}':'For {1}, launch {0}'});
 assert.equal(t('  Библиотека\n'),'  Library\n');
 assert.equal(t('Запуск My Game для Аккаунты'),'For Аккаунты, launch My Game');
 assert.equal(t('Цена: 12,50 ₽'),'Price: 12,50 ₽');
 assert.equal(t('A player name'),'A player name');
});
test('reject partial, stale, malformed and incorrectly substituted packs',()=>{
 assert.equal(validatePack(pack,source),pack);
 for(const change of [{revision:'old'},{locale:'de'},{machine:'yes'},{messages:[]},{messages:{...pack.messages,'Запуск {0} для {1}':'Launch {0}'}}])assert.throws(()=>validatePack({...pack,...change},source,'en'));
 assert.throws(()=>validatePack({...pack,messages:{...pack.messages,'Библиотека':'Библиотека'}},source),'Copied source text is not an English translation');
});
test('RTL scripts and legacy provider locale codes',()=>{
 for(const language of ['ar','iw','fa-AF','pa-Arab','bm-Nkoo','ckb'])assert.equal(direction(language),'rtl');
 for(const language of ['ru','de','ku','iu-Latn'])assert.equal(direction(language),'ltr');
 assert.equal(intlLocale('iw'),'he');assert.equal(intlLocale('jw'),'jv');assert.equal(intlLocale('zh-CN'),'zh-Hans');
});
test('remote catalogs cannot change revision or introduce path-like IDs and invalid digests',()=>{
 const index={schema:1,revision:'v1',languages:[{locale:'ru',name:'Russian',nativeName:'Русский'}]};
 assert.equal(validateIndex(index,source),index);
 assert.throws(()=>validateIndex({...index,revision:'old'},source));
 assert.throws(()=>validateIndex({...index,languages:[{locale:'../ru',name:'Russian',nativeName:'Русский'}]},source));
 assert.throws(()=>validateIndex({...index,languages:[{...index.languages[0],sha256:'bad'}]},source));
});
test('the English prompt covers the actual SDK and publishing contract',()=>{
 assert.doesNotMatch(MOD_PROMPT,/[А-Яа-яЁё]/);
 for(const text of ['activate(swch)','registerLauncher','mod-sdk/index.d.ts','check my-mod','dist/mod.zip','mod.zip','six hours','personal GitHub','never access SWCH'])assert.ok(MOD_PROMPT.includes(text),text);
});
test('all advertised downloadable packs are complete and pinned',()=>{
 const actual=JSON.parse(fs.readFileSync(new URL('../../ui/lang/source.json',import.meta.url)));
 const index=JSON.parse(fs.readFileSync(new URL('../../ui/lang/index.json',import.meta.url)));
 const ids=new Set();
 for(const entry of index.languages){
  assert.ok(!ids.has(entry.locale));ids.add(entry.locale);
  if(!entry.sha256)continue;
  assert.match(entry.sha256,/^[a-f0-9]{64}$/);
  const bytes=fs.readFileSync(new URL(`../../ui/lang/packs/${entry.locale}.json`,import.meta.url));
  assert.equal(crypto.createHash('sha256').update(bytes).digest('hex'),entry.sha256);
  const value=JSON.parse(bytes);
  validatePack(value,actual,entry.locale);
 }
 assert.equal(ids.size,249);
});
