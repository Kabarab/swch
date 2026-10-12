const {spawnSync}=require('node:child_process');
const path=require('node:path');
const root=path.resolve(__dirname,'../..');
const jobs=[
 ['--test','tests/ui/i18n.test.mjs'],
 ['tests/ui/i18n-browser.cjs'],
 ['--test','tests/mod-sdk.test.mjs'],
 ['tests/ui/mods-browser.cjs'],
 ['--test','tests/ui/model.test.mjs','tests/ui/inventory-layout.test.mjs','tests/ui/platforms.test.mjs','tests/ui/claim-state.test.mjs','tests/ui/playtime.test.mjs','tests/ui/riot-inventory.test.mjs','tests/ui/deadlock-stats.test.mjs'],
 ...['startup-browser','account-session-regressions','steam-download-modal','riot-inventory-batch','accounts-design','inventory-layout-browser','ux-browser','gog-browser','ubisoft-browser','riot-browser','playtime-browser','inventory-platforms-browser','riot-inventory-browser','deadlock-browser','game-stats'].map(name=>['tests/ui/'+name+'.cjs']),
];
for(const args of jobs){
 const result=spawnSync(process.execPath,args,{cwd:root,stdio:'inherit',timeout:180000,env:process.env});
 if(result.error){console.error(result.error.message);process.exit(1);}
 if(result.status!==0)process.exit(result.status||1);
}
const connected=spawnSync(process.execPath,['tests/ui/run-connected.cjs','epic-browser-login','auto-claim-browser'],{cwd:root,stdio:'inherit',timeout:180000,env:process.env});
if(connected.error||connected.status!==0)process.exit(connected.status||1);
console.log('All UI regression checks passed.');
