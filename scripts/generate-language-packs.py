"""One-time, optional maintainer tool. No translation service is used by SWCH.

Uses Google's public web translation endpoint (no key, no service guarantee).
Only repository UI strings are sent. Never send account data or runtime errors.
Checkpoints allow an interrupted generation to resume; malformed batches are
retried individually. No incomplete dictionary is published as a language pack.
"""
import argparse, concurrent.futures, hashlib, json, re, time, threading, urllib.parse, urllib.request, urllib.error
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STOP = threading.Event()
APPROVED_KEYS = None
ENGLISH_MESSAGES = {}
DIAGNOSTIC_TEXT = re.compile(r'ошиб|не удалось|предупреж|вниман|недоступ|не найден',re.I)
class ServiceUnavailable(RuntimeError):pass
LANGUAGES = '''ab|Abkhaz;ace|Acehnese;ach|Acholi;aa|Afar;af|Afrikaans;sq|Albanian;alz|Alur;am|Amharic;ar|Arabic;hy|Armenian;as|Assamese;av|Avar;awa|Awadhi;ay|Aymara;az|Azerbaijani;ban|Balinese;bal|Baluchi;bm|Bambara;bci|Baoulé;ba|Bashkir;eu|Basque;btx|Batak Karo;bts|Batak Simalungun;bbc|Batak Toba;be|Belarusian;bem|Bemba;bn|Bengali;bew|Betawi;bho|Bhojpuri;bik|Bikol;bs|Bosnian;br|Breton;bg|Bulgarian;bua|Buryat;yue|Cantonese;ca|Catalan;ceb|Cebuano;ch|Chamorro;ce|Chechen;ny|Chichewa;zh-CN|Chinese (Simplified);zh-TW|Chinese (Traditional);chk|Chuukese;cv|Chuvash;co|Corsican;crh|Crimean Tatar (Cyrillic);crh-Latn|Crimean Tatar (Latin);hr|Croatian;cs|Czech;da|Danish;fa-AF|Dari;dv|Dhivehi;din|Dinka;doi|Dogri;dov|Dombe;nl|Dutch;dyu|Dyula;dz|Dzongkha;en|English;eo|Esperanto;et|Estonian;ee|Ewe;fo|Faroese;fj|Fijian;tl|Filipino;fi|Finnish;fon|Fon;fr|French;fr-CA|French (Canada);fy|Frisian;fur|Friulian;ff|Fulani;gaa|Ga;gl|Galician;ka|Georgian;de|German;el|Greek;gn|Guarani;gu|Gujarati;ht|Haitian Creole;cnh|Hakha Chin;ha|Hausa;haw|Hawaiian;iw|Hebrew;hil|Hiligaynon;hi|Hindi;hmn|Hmong;hu|Hungarian;hrx|Hunsrik;iba|Iban;is|Icelandic;ig|Igbo;ilo|Ilocano;id|Indonesian;iu-Latn|Inuktut (Latin);iu|Inuktut (Syllabics);ga|Irish;it|Italian;jam|Jamaican Patois;ja|Japanese;jw|Javanese;kac|Jingpo;kl|Kalaallisut;kn|Kannada;kr|Kanuri;pam|Kapampangan;kk|Kazakh;kha|Khasi;km|Khmer;cgg|Kiga;kg|Kikongo;rw|Kinyarwanda;ktu|Kituba;trp|Kokborok;kv|Komi;gom|Konkani;ko|Korean;kri|Krio;ku|Kurdish (Kurmanji);ckb|Kurdish (Sorani);ky|Kyrgyz;lo|Lao;ltg|Latgalian;la|Latin;lv|Latvian;lij|Ligurian;li|Limburgish;ln|Lingala;lt|Lithuanian;lmo|Lombard;lg|Luganda;luo|Luo;lb|Luxembourgish;mk|Macedonian;mad|Madurese;mai|Maithili;mak|Makassar;mg|Malagasy;ms|Malay;ms-Arab|Malay (Jawi);ml|Malayalam;mt|Maltese;mam|Mam;gv|Manx;mi|Maori;mr|Marathi;mh|Marshallese;mwr|Marwadi;mfe|Mauritian Creole;chm|Meadow Mari;mni-Mtei|Meiteilon (Manipuri);min|Minang;lus|Mizo;mn|Mongolian;my|Myanmar (Burmese);nhe|Nahuatl (Eastern Huasteca);ndc-ZW|Ndau;nr|Ndebele (South);new|Nepalbhasa (Newari);ne|Nepali;bm-Nkoo|NKo;no|Norwegian;nus|Nuer;oc|Occitan;or|Odia (Oriya);om|Oromo;os|Ossetian;pag|Pangasinan;pap|Papiamento;ps|Pashto;fa|Persian;pl|Polish;pt|Portuguese (Brazil);pt-PT|Portuguese (Portugal);pa|Punjabi (Gurmukhi);pa-Arab|Punjabi (Shahmukhi);qu|Quechua;kek|Qʼeqchiʼ;rom|Romani;ro|Romanian;rn|Rundi;ru|Russian;se|Sami (North);sm|Samoan;sg|Sango;sa|Sanskrit;sat-Latn|Santali (Latin);sat|Santali (Ol Chiki);gd|Scots Gaelic;nso|Sepedi;sr|Serbian;st|Sesotho;crs|Seychellois Creole;shn|Shan;sn|Shona;scn|Sicilian;szl|Silesian;sd|Sindhi;si|Sinhala;sk|Slovak;sl|Slovenian;so|Somali;es|Spanish;su|Sundanese;sus|Susu;sw|Swahili;ss|Swati;sv|Swedish;ty|Tahitian;tg|Tajik;ber-Latn|Tamazight;ber|Tamazight (Tifinagh);ta|Tamil;tt|Tatar;te|Telugu;tet|Tetum;th|Thai;bo|Tibetan;ti|Tigrinya;tiv|Tiv;tpi|Tok Pisin;to|Tongan;lua|Tshiluba;ts|Tsonga;tn|Tswana;tcy|Tulu;tum|Tumbuka;tr|Turkish;tk|Turkmen;tyv|Tuvan;ak|Twi;udm|Udmurt;uk|Ukrainian;ur|Urdu;ug|Uyghur;uz|Uzbek;ve|Venda;vec|Venetian;vi|Vietnamese;war|Waray;cy|Welsh;wo|Wolof;xh|Xhosa;sah|Yakut;yi|Yiddish;yo|Yoruba;yua|Yucatec Maya;zap|Zapotec;zu|Zulu'''
def slots(text): return sorted(re.findall(r'\{\d+\}',text))
def encode(value): return (json.dumps(value,ensure_ascii=False,separators=(',',':'))+'\n').encode('utf-8')
def write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True)
    temporary=path.with_suffix('.tmp');temporary.write_bytes(encode(value));temporary.replace(path)
def request(text,locale,source_language='ru'):
    url='https://translate.googleapis.com/translate_a/single?'+urllib.parse.urlencode(dict(client='gtx',sl=source_language,tl=locale,dt='t',q=text))
    for attempt in range(5):
        if STOP.is_set():raise ServiceUnavailable('Translation service unavailable; resume from checkpoints later')
        try:
            req=urllib.request.Request(url,headers={'User-Agent':'SWCH-language-pack-builder/1'})
            with urllib.request.urlopen(req,timeout=35) as response:result=json.load(response)
            return ''.join(row[0] or '' for row in result[0])
        except urllib.error.HTTPError as error:
            if error.code==429:
                # Honor service throttling; never rotate proxies or evade a challenge.
                if attempt>=2:
                    STOP.set();raise ServiceUnavailable('Google rate limit persists; generation stopped and checkpoints retained') from error
                wait=max(60,int(error.headers.get('Retry-After','60')))
                print(f'{locale}: rate limited, waiting {wait}s',flush=True);STOP.wait(wait)
            elif error.code in (403,405):
                STOP.set();raise ServiceUnavailable(f'Translation service rejected requests ({error.code}); checkpoints retained') from error
            elif error.code==400:raise
            if attempt==4:raise
            time.sleep(min(30,2**attempt))
        except Exception:
            if attempt==4:raise
            time.sleep(min(30,2**attempt))
def clean(value):
    # Some scripts localize digits inside placeholders; IDs remain ASCII.
    value=re.sub(r'\{\s*(\d+)\s*\}',lambda m:'{'+str(int(m[1]))+'}',value)
    return re.sub(r'\s+',' ',value).strip()
def translate_batch(keys,locale,source_language):
    if APPROVED_KEYS is not None and not set(keys).issubset(APPROVED_KEYS):
        raise ValueError('Translation batch contains text outside the approved message list')
    text='\n'.join(f'[{i:04d}] {key}' for i,key in enumerate(keys))
    translated=request(text,locale,source_language)
    matches=list(re.finditer(r'\[\s*(\d{4})\s*\]\s*',translated))
    values={int(m[1]):clean(translated[m.end():matches[i+1].start() if i+1<len(matches) else len(translated)]) for i,m in enumerate(matches)}
    output={}
    for i,key in enumerate(keys):
        value=values.get(i,'')
        if not value or slots(value)!=slots(key) or source_language=='en' and locale!='en' and value==key:
            value=clean(request(key,locale,source_language))
        if not value or slots(value)!=slots(key):raise ValueError(f'Untranslated slots in {locale}: {key[:60]}')
        output[key]=value
    return output
def generate(locale,source):
    checkpoint=ROOT/'output/languages/checkpoints'/f'{locale}.json'
    bundled=ROOT/'ui/lang/packs'/f'{locale}.json'
    previous=json.loads(bundled.read_text(encoding='utf-8'))['messages'] if bundled.exists() else {}
    if checkpoint.exists():previous.update(json.loads(checkpoint.read_text(encoding='utf-8')))
    overrides=ROOT/'ui/lang/overrides'/f'{locale}.json'
    if overrides.exists():
        for key,value in json.loads(overrides.read_text(encoding='utf-8')).items():
            if key not in source['messages'] or not isinstance(value,str) or not value.strip() or slots(key)!=slots(value):raise ValueError(f'Invalid override: {key}')
            previous[key]=value
    def diagnostic(key):
        return not re.search('[А-Яа-яЁё]',key) and not re.fullmatch(r'HTTP \{\d+\}',key) and bool(re.search(r'[A-Za-z]{3,}[ !:]',key)) and any(path.startswith(('src/','mod-sdk/')) for path in source.get('locations',{}).get(key,[]))
    cyrillic_targets={'ab','av','be','bg','ba','bua','ce','cv','crh','kk','kv','ky','mk','mn','os','ru','sr','tg','tt','tyv','udm','uk','sah','chm'}
    def complete(key,value):
        if not value or slots(key)!=slots(value):return False
        if locale=='ru':return not diagnostic(key) or value!=key
        if locale!='en' and diagnostic(key) and value==key:return False
        if locale!='en' and DIAGNOSTIC_TEXT.search(key) and value==ENGLISH_MESSAGES.get(key) and not (locale=='es' and value in ('Error','Error:','Error: {0}')):return False
        if locale not in ('en','es') and re.match(r'^(Ошибка|Не удалось|Предупреждение)',key) and value in ('Error','Warning'):return False
        if locale not in cyrillic_targets and re.search('[А-Яа-яЁё]',value):return False
        return not (len(key)>35 and re.search('[А-Яа-яЁё]',key) and key==value)
    keys=list(source['messages']); messages={k:v for k,v in previous.items() if k in source['messages'] and complete(k,v)}
    if locale=='ru':messages.update({k:k for k in keys if not diagnostic(k)})
    # Reuse existing local translations for labels outside a diagnostic-only run.
    if APPROVED_KEYS is not None:
        if 'HTTP {0}' in keys:messages.setdefault('HTTP {0}','HTTP {0}')
        if 'Steam progress unavailable' in keys and previous.get('Нет данных'):
            messages.setdefault('Steam progress unavailable','Steam: '+previous['Нет данных'])
        if 'Ошибка' not in messages and previous.get('Ошибка:'):
            error_label=previous['Ошибка:'].rstrip(':：').strip()
            if complete('Ошибка',error_label):messages['Ошибка']=error_label
    pending=[k for k in keys if k not in messages]
    if APPROVED_KEYS is not None and not set(pending).issubset(APPROVED_KEYS):
        raise ValueError(f'Unapproved missing translations: {sorted(set(pending)-APPROVED_KEYS)}')
    batches=[]
    for source_language in ('ru','en'):
        batch=[];size=0;characters=0
        for key in [k for k in pending if bool(re.search('[А-Яа-яЁё]',k))==(source_language=='ru')]:
            length=len(urllib.parse.quote(key))+30
            if batch and (size+length>11000 or characters+len(key)>3000):batches.append((source_language,batch));batch=[];size=0;characters=0
            batch.append(key);size+=length;characters+=len(key)+8
        if batch:batches.append((source_language,batch))
    for source_language,batch in batches:
        if not batch:continue
        messages.update(translate_batch(batch,locale,source_language));write(checkpoint,messages)
        print(f'{locale}: {len(messages)}/{len(keys)}',flush=True)
        time.sleep(2)
    overrides=ROOT/'ui/lang/overrides'/f'{locale}.json'
    if overrides.exists():
        for key,value in json.loads(overrides.read_text(encoding='utf-8')).items():
            if key not in source['messages'] or not isinstance(value,str) or not value.strip() or slots(key)!=slots(value):raise ValueError(f'Invalid override: {key}')
            messages[key]=value
    if any(not complete(key,messages[key]) for key in keys):raise ValueError(f'Incomplete translation in {locale}; draft remains in checkpoints')
    pack={'schema':1,'locale':locale,'revision':source['revision'],'machine':locale!='ru','provider':'Google Translate (offline generation)' if locale!='ru' else 'SWCH','messages':{k:messages[k] for k in keys}}
    write(ROOT/'ui/lang/packs'/f'{locale}.json',pack)
    print(f'COMPLETE {locale}: {len(keys)}',flush=True)
def main():
    global APPROVED_KEYS, ENGLISH_MESSAGES
    parser=argparse.ArgumentParser();parser.add_argument('--locales');parser.add_argument('--workers',type=int,default=1);parser.add_argument('--approved-messages',type=Path);args=parser.parse_args()
    if args.approved_messages:
        approved=json.loads(args.approved_messages.read_text(encoding='utf-8'))
        if not isinstance(approved,list) or not all(isinstance(key,str) for key in approved):raise ValueError('Approved messages must be a list of strings')
        APPROVED_KEYS=frozenset(approved)
    source=json.loads((ROOT/'ui/lang/source.json').read_text(encoding='utf-8'))
    english_pack=ROOT/'ui/lang/packs/en.json'
    if english_pack.exists():ENGLISH_MESSAGES=json.loads(english_pack.read_text(encoding='utf-8'))['messages']
    languages=[dict(locale=code,name=name,nativeName=name) for code,name in (item.split('|',1) for item in LANGUAGES.split(';'))]
    write(ROOT/'ui/lang/languages.json',{'schema':1,'provider':'Google Translate','languages':languages})
    locales=args.locales.split(',') if args.locales else [i['locale'] for i in languages]
    # Ship familiar and RTL languages first; restart resumes all cached strings.
    priority=['ru','en','be','uk','de','ar','es','fr','pl','ja','zh-CN','pt','iw']
    locales.sort(key=lambda l:priority.index(l) if l in priority else len(priority))
    failures=[]
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures={pool.submit(generate,locale,source):locale for locale in locales}
        for future in concurrent.futures.as_completed(futures):
            try:future.result()
            except Exception as error:
                failures.append(futures[future]);print('FAILED',futures[future],str(error),flush=True)
                if isinstance(error,ServiceUnavailable):
                    STOP.set()
                    for pending in futures:pending.cancel()
                    break
    print('Failed locales:',','.join(failures),flush=True)
    if failures:raise SystemExit(1)
if __name__=='__main__':main()
