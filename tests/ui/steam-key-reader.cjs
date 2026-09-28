const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const source = fs.readFileSync('src/steam_api_key.swift','utf8');
const js = source.split('web.evaluateJavaScript("""')[1].split('""")')[0]
 .replaceAll('\\(expected)', '76561197960265729').replaceAll('\\\\','\\');
const key = '0123456789ABCDEF0123456789ABCDEF';
function read(id, paragraphs, host='steamcommunity.com', path='/dev/apikey') {
 return vm.runInNewContext(js, {location:{protocol:'https:',hostname:host,pathname:path},window:{g_steamID:id},document:{querySelectorAll:()=>paragraphs.map(textContent=>({textContent}))}});
}
assert.equal(read('76561197960265729',[`Key: ${key}`]),key);
assert.equal(read('76561197960265730',[`Key: ${key}`]),'wrong-account');
assert.equal(read(undefined,[`Key: ${key}`]),'wrong-account');
assert.equal(read('76561197960265729',[`Key: ${key}`],'example.com'),null);
assert.equal(read('76561197960265729',[`Key: ${key}`],'steamcommunity.com','/login'),null);
assert.equal(read('76561197960265729',[`Domain Name: ${key}`]),null);
assert.equal(read('76561197960265729',['Key: invalid']),null);
console.log('PASS: key extraction, account isolation, official page restriction, missing/invalid key');
