export const MOD_ARTWORK_PLACEHOLDER='/ui/artwork/mod-platform.svg';

function coverURL(value){
  if(typeof value!=='string'||new TextEncoder().encode(value).length>2048||/[\s\p{Cc}]/u.test(value))return '';
  try{const url=new URL(value);return url.protocol==='https:'&&url.hostname&&!url.username&&!url.password?url.href:'';}catch{return '';}
}

export function modArtwork(entry,catalog=[]){
  const source=entry.source||catalog.find(item=>item.manifest.id===entry.manifest.id)||entry;
  const owner=source.repository?.split('/')[0]||source.author;
  const avatar=Number.isSafeInteger(source.owner_id)&&source.owner_id>0
    ?`https://avatars.githubusercontent.com/u/${source.owner_id}?s=256`
    :`https://github.com/${typeof owner==='string'&&/^[a-z\d-]{1,39}$/i.test(owner)?owner:'github'}.png?size=256`;
  const cover=coverURL(entry.manifest.cover_url);
  return {src:cover||avatar,avatar,isAvatar:!cover};
}

// Image errors do not bubble; register this handler with capture enabled.
export function fallbackModArtwork(event){
  const img=event.target;
  if(!img.matches?.('img.mod-card-artwork'))return;
  const avatar=img.dataset.modAvatar;
  if(avatar){
    delete img.dataset.modAvatar;
    img.classList.add('is-avatar');
    if(img.getAttribute('src')!==avatar){img.src=avatar;return;}
  }
  if(img.getAttribute('src')!==MOD_ARTWORK_PLACEHOLDER)img.src=MOD_ARTWORK_PLACEHOLDER;
}
