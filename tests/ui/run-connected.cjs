const {spawn}=require('node:child_process');
const path=require('node:path');
const {staticServer}=require('./harness.cjs');
(async()=>{
 const server=await staticServer();
 try{
  for(const name of process.argv.slice(2)){
   const result=await new Promise(resolve=>{
    const child=spawn(process.execPath,[path.join(__dirname,name+'.cjs')],{cwd:server.root,stdio:'inherit',env:{...process.env,SWCH_UI_URL:server.origin}});
    child.on('error',error=>{console.error(error);resolve(1);});child.on('exit',code=>resolve(code||0));
   });
   if(result){process.exitCode=result;break;}
  }
 }finally{await server.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
