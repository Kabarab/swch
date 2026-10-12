import test from 'node:test';
import assert from 'node:assert/strict';
import {PLATFORMS,PLATFORM_IDS,platformNavigation,inventoryPlatforms} from '../../ui/platforms.mjs';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';

test('navigation uses games in the library, and includes accounts without games only in Accounts',()=>{
  const games=[{launcher:'steam'},{launcher:'steam'},{launcher:'minecraft'}];
  const accounts=[{launcher:'steam'},{launcher:'epic'}];
  const library=platformNavigation(games,accounts,'library');
  assert.deepEqual(library.filter(p=>p.visible).map(p=>[p.id,p.count]),[['steam',2],['minecraft',1]]);
  const directory=platformNavigation(games,accounts,'accounts');
  assert.deepEqual(directory.filter(p=>p.visible).map(p=>[p.id,p.count]),[['steam',1],['epic',1],['minecraft',0]]);
  assert.ok(platformNavigation([],[],'accounts').every(p=>!p.visible));
  assert.deepEqual(platformNavigation(games,accounts,'favorites'),library);
});
test('all eight official platform logos are local files',()=>{
  assert.equal(PLATFORM_IDS.length,8);
  for(const platform of Object.values(PLATFORMS))assert.ok(existsSync(fileURLToPath(new URL('../../'+platform.logo.slice(1),import.meta.url))));
});
test('inventory includes only supported platforms with connected accounts',()=>{
  assert.deepEqual(inventoryPlatforms([]),[]);
  assert.deepEqual(inventoryPlatforms([{launcher:'steam'},{launcher:'riot'},{launcher:'minecraft'}]),['steam','riot']);
  assert.deepEqual(inventoryPlatforms([{launcher:'roblox'},{launcher:'roblox'}]),['roblox']);
});
