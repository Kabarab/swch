"""Inventory app-owned display text; never read credentials or installed copies.

Run with the project's Graphify Python, which supplies the tree-sitter grammars.
The resulting dictionary is independent of any translation service.
"""
from pathlib import Path
from html.parser import HTMLParser
from html import unescape
import hashlib, json, re, sys
from tree_sitter import Language, Parser
import tree_sitter_javascript, tree_sitter_rust

ROOT=Path(__file__).resolve().parents[1]
OUT=ROOT/'ui/lang/source.json'
BRANDS={'SWCH','swch','swch Hub','Steam','Epic Games','Epic Games Store','Riot Games','Roblox','Minecraft','Ubisoft Connect','GOG GALAXY','GALAXY','HoYoverse','HoYoPlay','VALORANT','Fortnite','League of Legends','Dota 2','Counter-Strike 2','CS2','RAP','Windows','GitHub','NEW'}
records={}
def normalize(text): return re.sub(r'\s+',' ',unescape(text)).strip()
def add(text,path):
    text=normalize(text)
    if not text or text in BRANDS or not re.search(r'[A-Za-zА-Яа-яЁё]', re.sub(r'\{\d+\}','',text)):return
    if len(text)>4000:return
    records.setdefault(text,set()).add(str(path).replace('\\','/'))

class Texts(HTMLParser):
    def __init__(self,path): super().__init__(convert_charrefs=True);self.path=path;self.skip=[]
    def handle_starttag(self,tag,attrs):
        if tag in ('script','style'):self.skip.append(tag)
        for name,value in attrs:
            if name in ('title','aria-label','placeholder','alt') and value:add(value,self.path)
    def handle_endtag(self,tag):
        if self.skip and self.skip[-1]==tag:self.skip.pop()
    def handle_data(self,text):
        if not self.skip:add(text,self.path)

def display(text,path,english=False):
    if re.search(r'<[a-zA-Z][\s\S]*?>',text):
        try:Texts(path).feed(text)
        except Exception:pass
    elif re.search(r'[А-Яа-яЁё]',text) or english or re.match(r'^(?:Failed|Cannot|Could not|Invalid|Missing|Unsupported|Unknown|Download|Mod |Package |Catalog |Duplicate|Entry point|Message |Installation |Expanded|Unexpected|Incomplete|Confirm |Game or account|ID belongs)',text):
        add(text,path)

def js_unescape(text):
    def replace(m):
        value=m.group(1)
        if value.startswith('u{'):return chr(int(value[2:-1],16))
        if value.startswith('u'):return chr(int(value[1:],16))
        if value.startswith('x'):return chr(int(value[1:],16))
        return {'n':'\n','r':'\r','t':'\t','b':'\b','f':'\f','v':'\v','0':'\0'}.get(value,value)
    return re.sub(r'\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|.)',replace,text)

def scan_js(path, diagnostics_only=False):
    raw=path.read_bytes();parser=Parser(Language(tree_sitter_javascript.language()));tree=parser.parse(raw)
    if tree.root_node.has_error:raise ValueError(f'JavaScript parse failed: {path}')
    def walk(node):
        # Error constructors also contain English diagnostics outside the UI.
        ancestor=node.parent
        error_context=False
        for _ in range(3):
            if ancestor is None:break
            if ancestor.type=='new_expression' and re.match(rb'new\s+(?:Error|TypeError|RangeError)\b',raw[ancestor.start_byte:ancestor.end_byte]):error_context=True;break
            ancestor=ancestor.parent
        if node.type=='string' and (not diagnostics_only or error_context):display(js_unescape(raw[node.start_byte+1:node.end_byte-1].decode('utf-8')),path.relative_to(ROOT),english=error_context)
        elif node.type=='template_string' and (not diagnostics_only or error_context):
            parts=[];counter=0
            for child in node.named_children:
                if child.type=='template_substitution':parts.append('{'+str(counter)+'}');counter+=1
                else:parts.append(js_unescape(raw[child.start_byte:child.end_byte].decode('utf-8')))
            display(''.join(parts),path.relative_to(ROOT),english=error_context)
        for child in node.named_children:walk(child)
    walk(tree.root_node)

def scan_rust(path):
    raw=path.read_bytes().split(b'#[cfg(test)]',1)[0];tree=Parser(Language(tree_sitter_rust.language())).parse(raw)
    def walk(node):
        if node.type=='string_literal':
            text=js_unescape(raw[node.start_byte+1:node.end_byte-1].decode('utf-8'))
            # Rust's named, positional and debug format parameters become opaque slots.
            index=[0]
            def slot(m):value='{'+str(index[0])+'}';index[0]+=1;return value
            if node.parent and node.parent.type=='token_tree':text=re.sub(r'(?<!\{)\{[^{}]*\}(?!\})',slot,text)
            # Include every explicit error, not just messages beginning with a
            # small English verb list. Keep protocol values and paths out.
            ancestor=node.parent
            error_context=False
            for _ in range(5):
                if ancestor is None:break
                code=raw[ancestor.start_byte:ancestor.end_byte]
                if ancestor.type in ('call_expression','macro_invocation') and re.match(rb'(?:Err\s*\(|[^\n]*\.ok_or(?:_else)?\s*\(|(?:bail|anyhow|warn|error)!\s*\()',code):error_context=True;break
                if ancestor.type=='attribute_item' and re.match(rb'#\s*\[\s*error\s*\(',code):error_context=True;break
                ancestor=ancestor.parent
            display(text,path.relative_to(ROOT),english=error_context and bool(re.search(r'[A-Za-z]{3,} ',text)))
        for child in node.named_children:walk(child)
    walk(tree.root_node)

def run():
    for path in [ROOT/'index.html',*sorted((ROOT/'ui').glob('*.html'))]:Texts(path.relative_to(ROOT)).feed(path.read_text(encoding='utf-8'))
    for path in [*sorted((ROOT/'ui').glob('*.js')),*sorted((ROOT/'ui').glob('*.mjs'))]:
        if path.name not in ('i18n.js','i18n-core.mjs','mod-prompt.mjs'):scan_js(path)
    for path in sorted((ROOT/'mod-sdk').glob('*.mjs')):
        if path.name in ('index.mjs','runtime.mjs'):scan_js(path,diagnostics_only=True)
    for path in sorted((ROOT/'src').rglob('*.rs')):
        if 'webview2' not in path.parts and 'i18n' not in path.name:scan_rust(path)
    for path in [ROOT/'src/windows_app.cpp',ROOT/'src/mac_app.swift']:
        content=path.read_text(encoding='utf-8')
        for text in re.findall(r'LR"rawhtml\(([\s\S]*?)\)rawhtml"',content)+re.findall(r'"""([\s\S]*?)"""',content):
            if '<html' in text:Texts(path.relative_to(ROOT)).feed(text)
    # Stable Node/libuv diagnostics. The operation and path remain opaque slots.
    for text in ['EISDIR: illegal operation on a directory, {0}', 'ENOENT: no such file or directory, {0}', 'ENOTDIR: not a directory, {0}', 'EACCES: permission denied, {0}', 'EPERM: operation not permitted, {0}', 'EEXIST: file already exists, {0}', 'EBUSY: resource busy or locked, {0}', 'ENOSPC: no space left on device, {0}', 'ENOTEMPTY: directory not empty, {0}', 'EMFILE: too many open files, {0}', 'ECONNREFUSED: connection refused, {0}', 'ETIMEDOUT: connection timed out, {0}']:
        add(text,'mod-sdk/runtime.mjs')
    # App-owned English labels and localization controls are explicitly included.
    for text in ['NEW','PLAY YOUR WAY','Library','Language','Search languages','Machine translation','Download language pack','Language pack is incomplete','Русский','Язык интерфейса','Найти язык','Скачать и применить','Машинный перевод','Пакет установлен','Пакет доступен для скачивания','Загрузка языкового пакета…','Не удалось загрузить язык. Текущий язык сохранён.','Языковой пакет неполный или повреждён.','Для первой загрузки нужен интернет. Затем перевод работает без интернета.','Перевод подготовлен автоматически. Возможны неточности.','Выберите язык интерфейса','Язык сохранён','Непереведённое сообщение сервиса','Языковые пакеты','Отмена']:
        if text in BRANDS:records.setdefault(text,set()).add('ui/i18n.js')
        else:add(text,'ui/i18n.js')
    keys=sorted(records)
    source={'schema':1,'sourceLocale':'ru','revision':hashlib.sha256(json.dumps(keys,ensure_ascii=False).encode()).hexdigest(),'messages':{text:text for text in keys},'locations':{text:sorted(records[text]) for text in keys}}
    serialized=json.dumps(source,ensure_ascii=False,indent=2)+'\n'
    if '--check' in sys.argv:
        if not OUT.exists() or OUT.read_text(encoding='utf-8')!=serialized:raise SystemExit('The UI text catalog is stale. Run scripts/extract_i18n.py and update affected language packs.')
    else:
        OUT.parent.mkdir(parents=True,exist_ok=True);OUT.write_text(serialized,encoding='utf-8')
    print(f'Catalog: {len(keys)} messages, {sum(len(x) for x in keys)} characters, revision {source["revision"][:12]}')

if __name__=='__main__':run()
