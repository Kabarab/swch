import fs from 'node:fs/promises';
import path from 'node:path';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {validatePack,intlLocale,direction,canonicalMessages} from '../ui/i18n-core.mjs';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const source=JSON.parse(await fs.readFile(path.join(root,'ui/lang/source.json'),'utf8'));
const languages=JSON.parse(await fs.readFile(path.join(root,'ui/lang/languages.json'),'utf8')).languages;
const args=process.argv.slice(2), strict=args.includes('--check'), requireAll=args.includes('--require-all');
const entries=[];
for(const item of languages){
  let nativeName;try{nativeName=new Intl.DisplayNames([intlLocale(item.locale)],{type:'language',fallback:'none'}).of(intlLocale(item.locale));}catch{}
  const entry={...item,nativeName:nativeName||item.name,direction:direction(item.locale)};
  try {
    const bytes=await fs.readFile(path.join(root,'ui/lang/packs',`${item.locale}.json`));
    const pack=validatePack(JSON.parse(bytes),source,item.locale);
    entry.sha256=crypto.createHash('sha256').update(bytes).digest('hex');entry.revision=pack.revision;entry.machine=pack.machine;entry.bytes=bytes.length;
    entry.messagesSha256=crypto.createHash('sha256').update(canonicalMessages(pack.messages)).digest('hex');
  }catch(error){if(strict&&(error.code!=='ENOENT'||requireAll||['ru','en'].includes(item.locale)))throw new Error(`Incomplete language ${item.locale}: ${error.message}`);}
  entries.push(entry);
}
const index={schema:1,revision:source.revision,languages:entries};
await fs.writeFile(path.join(root,'ui/lang/index.json'),JSON.stringify(index,null,2)+'\n');
const bundle=args.indexOf('--bundle');
if(bundle>=0){
  const destination=path.resolve(args[bundle+1]);
  // The assembled release keeps RU/EN. Other packs are fetched from the static branch.
  const folder=path.join(destination,'packs');
  for(const filename of await fs.readdir(folder))if(filename.endsWith('.json')&&!['ru.json','en.json'].includes(filename))await fs.unlink(path.join(folder,filename));
}
console.log(`Language packs: ${entries.filter(e=>e.sha256).length}/${entries.length}, ${Object.keys(source.messages).length} strings`);
