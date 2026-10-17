const fs=require('node:fs'),path=require('node:path'),http=require('node:http');
const root=path.resolve(__dirname,'../..');
function emptyApi(endpoint) {
  // Existing feature regressions explicitly use Russian; first-run language
  // detection is exercised separately by the localization browser tests.
  if(endpoint==='/api/i18n/selection')return {locale:'ru',preference:'ru',saved:true,revision:require('../../ui/lang/source.json').revision};
  if(endpoint==='/api/steam/library')return {games:[],accounts:[],warnings:[]};
  if(endpoint==='/api/epic/accounts')return {accounts:[]};
  if(endpoint==='/api/epic/library')return [];
  if(endpoint==='/api/riot/accounts')return {accounts:[],profiles:[]};
  if(endpoint==='/api/riot/sessions')return {sessions:[]};
  if(endpoint==='/api/roblox/accounts')return [];
  if(endpoint.endsWith('/library'))return {games:[],accounts:[]};
  if(endpoint==='/api/accounts/restrictions')return {restrictions:[]};
  if(endpoint==='/api/modules')return {modules:[]};
  return {};
}
async function staticServer(handleRequest) {
  const server=http.createServer((req,res)=>{
    const pathname=new URL(req.url,'http://localhost').pathname;
    if(handleRequest?.(req,res,pathname))return;
    const file=path.resolve(root,'.'+(pathname==='/'?'/index.html':pathname));
    if(!file.startsWith(root+path.sep)){res.writeHead(403);res.end();return;}
    fs.readFile(file,(error,bytes)=>{
      if(error){res.writeHead(404);res.end();return;}
      const mime={'.html':'text/html','.js':'text/javascript','.mjs':'text/javascript','.css':'text/css','.svg':'image/svg+xml','.png':'image/png','.ico':'image/x-icon','.jpg':'image/jpeg'};
      res.writeHead(200,{'Content-Type':mime[path.extname(file)]||'application/octet-stream'});res.end(bytes);
    });
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  return {root,origin:'http://127.0.0.1:'+server.address().port,close:()=>new Promise(resolve=>server.close(resolve))};
}
const launchOptions={headless:true,channel:process.env.PLAYWRIGHT_CHANNEL==='chromium'?undefined:(process.env.PLAYWRIGHT_CHANNEL||'chrome')};
module.exports={emptyApi,staticServer,launchOptions};
