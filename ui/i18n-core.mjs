// Packs contain text only. A pack is activated only when every key and slot matches.
export const normalizeText = value => String(value ?? '').replace(/\s+/gu, ' ').trim();
export const slots = value => [...String(value).matchAll(/\{\d+\}/g)].map(m => m[0]).sort();
const CYRILLIC_LOCALES = new Set(['ab','av','be','bg','ba','bua','ce','cv','crh','kk','kv','ky','mk','mn','os','ru','sr','tg','tt','tyv','udm','uk','sah','chm']);
export function validatePack(pack, source, locale = pack?.locale) {
  if (!pack || pack.schema !== 1 || pack.locale !== locale || pack.revision !== source.revision || typeof pack.machine !== 'boolean') throw new Error('pack-version');
  const messages = pack.messages;
  if (!messages || typeof messages !== 'object' || Array.isArray(messages) || Object.keys(messages).length !== Object.keys(source.messages).length) throw new Error('pack-incomplete');
  for (const key of Object.keys(source.messages)) {
    const text = messages[key];
    if (typeof text !== 'string' || !text.trim() || text.length > 16000 || slots(text).join() !== slots(key).join()) throw new Error('pack-message');
    if(locale!=='ru' && /[А-Яа-яЁё]/.test(key)) {
      if(!CYRILLIC_LOCALES.has(locale) && /[А-Яа-яЁё]/.test(text))throw new Error('pack-untranslated');
      if(key.length>35 && text===key)throw new Error('pack-untranslated');
    }
  }
  return pack;
}
export function direction(locale) {
  return /^(ar|fa|iw|he|ur|ps|sd|ug|dv|yi|ckb|bal)(-|$)/i.test(locale) || /-(Arab|Nkoo)(-|$)/i.test(locale) ? 'rtl' : 'ltr';
}
export function intlLocale(locale) {
  const aliases = {iw:'he',jw:'jv','zh-CN':'zh-Hans','zh-TW':'zh-Hant',tl:'fil',no:'nb','ber':'zgh'};
  try { return Intl.getCanonicalLocales(aliases[locale] || locale)[0]; } catch { return 'en'; }
}
export const DEFAULT_LOCALE = 'en';
export const SYSTEM_LANGUAGE = 'system';
// Match OS/browser tags to published pack IDs, including provider aliases and
// regional/script variants. Never pick a different script just by language.
export function systemLocale(preferences, languages) {
  const available = languages.filter(entry => entry.locale === 'ru' || entry.sha256).map(entry => {
    const locale = new Intl.Locale(intlLocale(entry.locale));
    return {id:entry.locale, locale, expanded:locale.maximize()};
  });
  for (const value of preferences || []) {
    let locale;
    try { locale = new Intl.Locale(intlLocale(Intl.getCanonicalLocales(String(value).replace(/_/g,'-'))[0])); }
    catch { continue; }
    const exact = available.find(entry => entry.locale.toString().toLowerCase() === locale.toString().toLowerCase());
    if (exact) return exact.id;
    const expanded = locale.maximize();
    const matches = available.filter(entry => entry.expanded.language === expanded.language && entry.expanded.script === expanded.script);
    const regional = matches.find(entry => entry.expanded.region === expanded.region);
    if (regional || matches.length) return (regional || matches[0]).id;
  }
  return DEFAULT_LOCALE;
}
const escapeRegex = value => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
export function createTranslator(source, messages) {
  const patterns = Object.keys(source.messages).filter(k => /\{\d+\}/.test(k) && (k.replace(/\{\d+\}/g,'').length >= 6 || / (ч|мин|шт|ГБ|МБ|КБ)$/.test(k))).sort((a,b)=>b.length-a.length).map(key => {
    const indices = [];
    let previous = 0, expression = '';
    for (const match of key.matchAll(/\{(\d+)\}/g)) {
      expression += escapeRegex(key.slice(previous, match.index)) + '(.+?)';
      indices.push(match[1]); previous = match.index + match[0].length;
    }
    expression += escapeRegex(key.slice(previous));
    return {key, indices, regex:new RegExp('^'+expression+'$','u')};
  });
  // Legacy renderers concatenate a label with a number or a service diagnostic.
  const fragments = Object.keys(source.messages).filter(k => !/\{\d+\}/.test(k) && k.length >= 5 && /[А-Яа-яЁё]/.test(k)).sort((a,b)=>b.length-a.length);
  return function translate(value) {
    const original = String(value ?? ''), key = normalizeText(original);
    if (!key) return original;
    let translated;
    if (Object.hasOwn(messages,key)) translated = messages[key];
    else {
      for (const pattern of patterns) {
        const match = pattern.regex.exec(key);
        if (!match) continue;
        const captures = Object.fromEntries(pattern.indices.map((index,i)=>[index,match[i+1]]));
        translated = messages[pattern.key].replace(/\{(\d+)\}/g,(_,index)=>captures[index]);
        break;
      }
      if (translated === undefined) {
        for (const fragment of fragments) {
          if (key.startsWith(fragment) && /^[\s·:(),.\d—-]/u.test(key.slice(fragment.length))) { translated = messages[fragment] + key.slice(fragment.length); break; }
          if (key.endsWith(fragment) && /[\s·:(),.\d—-]$/u.test(key.slice(0,-fragment.length))) { translated = key.slice(0,-fragment.length) + messages[fragment]; break; }
        }
      }
    }
    if (translated === undefined) return original;
    return (original.match(/^\s*/u)?.[0] || '') + translated + (original.match(/\s*$/u)?.[0] || '');
  };
}
export async function sha256(bytes) {
  return [...new Uint8Array(await crypto.subtle.digest('SHA-256',bytes))].map(n=>n.toString(16).padStart(2,'0')).join('');
}
export const canonicalMessages = messages => JSON.stringify(Object.fromEntries(Object.keys(messages).sort().map(key=>[key,messages[key]])));
export function validateIndex(index,source) {
  if(index?.schema!==1 || index.revision!==source.revision || !Array.isArray(index.languages) || !index.languages.length || index.languages.length>1000)throw new Error('catalog');
  const ids=new Set();
  for(const entry of index.languages){
    if(!entry || !/^[a-zA-Z]{2,3}(?:-[a-zA-Z0-9]{2,8}){0,2}$/.test(entry.locale) || ids.has(entry.locale))throw new Error('catalog-locale');
    ids.add(entry.locale);
    if(typeof entry.name!=='string'||entry.name.length>256||typeof entry.nativeName!=='string'||entry.nativeName.length>256)throw new Error('catalog-label');
    if(entry.sha256 && (!/^[a-f0-9]{64}$/.test(entry.sha256)||!/^[a-f0-9]{64}$/.test(entry.messagesSha256)||entry.revision!==source.revision||typeof entry.machine!=='boolean'))throw new Error('catalog-digest');
  }
  if(!ids.has('ru'))throw new Error('catalog-source');
  return index;
}
