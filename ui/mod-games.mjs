export const gameIdentity=game=>JSON.stringify([game?.launcher,String(game?.app_id??'')]);
export const modGames=mod=>mod.games??mod.manifest?.games??[];
export function modMatchesGame(mod,game){
  return modGames(mod).some(target=>gameIdentity(target)===gameIdentity(game));
}
// JSON is embedded in srcdoc; escape HTML delimiters even inside JavaScript strings.
export const bridgeJSON=value=>JSON.stringify(value).replace(/</g,'\\u003c').replace(/>/g,'\\u003e').replace(/&/g,'\\u0026');
