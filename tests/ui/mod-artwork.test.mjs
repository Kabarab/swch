import test from 'node:test';
import assert from 'node:assert/strict';
import {modArtwork} from '../../ui/mod-artwork.mjs';

test('mod artwork uses the installed cover and preserves GitHub provenance without a catalog',()=>{
  const source={repository:'alice/mod',owner_id:42,manifest:{id:'alice.mod',cover_url:'https://example.com/new.png'}};
  const entry={manifest:{id:'alice.mod',cover_url:'https://example.com/installed.png'},source};
  assert.deepEqual(modArtwork(entry),{src:'https://example.com/installed.png',avatar:'https://avatars.githubusercontent.com/u/42?s=256',isAvatar:false});
  delete entry.manifest.cover_url;
  assert.equal(modArtwork(entry).src,'https://avatars.githubusercontent.com/u/42?s=256');
  assert.equal(modArtwork(entry).isAvatar,true);
});

test('catalog metadata supplies avatars for ZIP installs; unknown authors use the GitHub avatar',()=>{
  const entry={manifest:{id:'alice.mod'}};
  const catalog=[{...entry,repository:'alice/mod',author:'Display name'}];
  assert.equal(modArtwork(entry,catalog).src,'https://github.com/alice.png?size=256');
  assert.equal(modArtwork(entry).src,'https://github.com/github.png?size=256');
  for(const cover_url of ['javascript:alert(1)','http://example.com/cover.png','https://user:pass@example.com/x','https://example.com/a b','https://example.com/a\n.png'])assert.equal(modArtwork({...entry,manifest:{...entry.manifest,cover_url}},catalog).isAvatar,true);
});
