import {spawn} from 'node:child_process';
import {randomBytes, randomUUID} from 'node:crypto';
import {fileURLToPath} from 'node:url';

const routes = new Map([
  ['/api/free-games','GET'], ['/api/free-games/catalog','GET'],
  ['/api/free-games/notifications','GET,POST'],
  ...['settings','check','cancel','notifications/disconnect','web/connect','web/verify',
    'web/manual','web/library','web/disconnect','web/add','keys/reveal','keys/redeem',
    'steam/start','steam/automatic','steam/complete','steam/disconnect']
    .map(path=>['/api/free-games/'+path,'POST'])
]);

export default async function activate(swch) {
  const platform = process.platform === 'win32' && process.arch === 'x64' ? 'windows-x64'
    : process.platform === 'darwin' && process.arch === 'arm64' ? 'macos-arm64' : null;
  if (!platform) throw new Error('Автоклеймер поддерживает Windows x64 и macOS ARM64.');
  const executable = fileURLToPath(new URL(`./native/${platform}/swch-autoclaimer${platform==='windows-x64'?'.exe':''}`,import.meta.url));
  const token = randomBytes(32).toString('hex');
  const child = spawn(executable, [], {windowsHide:true, stdio:['pipe','pipe','pipe'],
    env:{...process.env, SWCH_AUTO_CLAIM_TOKEN:token}});
  let exited = false;
  const exit = new Promise(resolve=>child.once('exit',()=>{exited=true;resolve();}));
  child.stderr.on('data',chunk=>swch.log(String(chunk).trim()));
  let port;
  try {
    port = await new Promise((resolve,reject)=>{
      let output='';
      const timer=setTimeout(()=>reject(new Error('Автоклеймер не запустился за 15 секунд.')),15000);
      const finish=(error,value)=>{clearTimeout(timer);child.stdout.off('data',read);error?reject(error):resolve(value);};
      const read=chunk=>{
        output+=chunk;
        if(output.length>4096)return finish(new Error('Некорректный ответ автоклеймера.'));
        if(output.includes('\n'))try{
          const ready=JSON.parse(output.split('\n')[0]);
          if(!Number.isInteger(ready.port)||ready.port<1||ready.port>65535)throw new Error('Invalid port');
          finish(null,ready.port);
        }catch(error){finish(error);}
      };
      child.stdout.on('data',read);
      child.once('error',error=>finish(error));
      child.once('exit',()=>finish(new Error('Процесс автоклеймера завершился.')));
    });
  }catch(error){child.stdin.end();child.kill();throw error;}
  // The native process also watches this pipe: even a forced Node termination stops it.
  const jobs = new Map();
  const prune=()=>{for(const [id,job] of jobs)if(job.done&&Date.now()-job.finished>600000)jobs.delete(id);};
  swch.registerTab({id:'giveaways',name:'Автоклеймер'});
  const request = input=>{
    if(exited)throw new Error('Автоклеймер остановлен. Перезапусти мод на странице «Моды».');
    const {path,body}=input,method=body===undefined?'GET':'POST';
    if(!routes.get(path)?.split(',').includes(method))throw new Error('Недоступная операция автоклеймера.');
    prune();if(jobs.size>=32)throw new Error('Слишком много запросов. Дождись завершения проверки.');
    const id=randomUUID(),job={done:false};jobs.set(id,job);
    void fetch(`http://127.0.0.1:${port}${path}`,{method,
      headers:{Authorization:`Bearer ${token}`,'Content-Type':'application/json'},
      ...(body!==undefined?{body:JSON.stringify(body)}:{}),signal:AbortSignal.timeout(660000)})
      .then(async response=>{const data=await response.json();if(!response.ok)throw new Error(data.error||'Ошибка автоклеймера');job.result=data;})
      .catch(error=>{job.error=error.message;})
      .finally(()=>{job.done=true;job.finished=Date.now();});
    return {job:id};
  };
  const poll = ({job:id})=>{
    const job=jobs.get(id);if(!job)throw new Error('Запрос истёк. Повтори операцию.');
    if(job.done)jobs.delete(id);
    return {done:job.done,...(job.done?{result:job.result,error:job.error}:{})};
  };
  const clipboard = async ({text})=>{
    if(typeof text!=='string'||text.length>16384)throw new Error('Некорректный текст.');
    const command=platform==='windows-x64'?'powershell.exe':'/usr/bin/pbcopy';
    const args=platform==='windows-x64'?['-NoProfile','-NonInteractive','-Command',
      '[Console]::InputEncoding = [System.Text.UTF8Encoding]::new(); Set-Clipboard -Value ([Console]::In.ReadToEnd())']:[];
    await new Promise((resolve,reject)=>{
      const copy=spawn(command,args,{windowsHide:true,stdio:['pipe','ignore','ignore']});
      copy.once('error',reject);copy.once('exit',code=>code===0?resolve():reject(new Error('Не удалось скопировать текст.')));
      copy.stdin.on('error',reject);copy.stdin.end(text,'utf8');
    });return {ok:true};
  };
  swch.registerAction({id:'control',name:'Проверить состояние'}, input=>{
    if(input.operation==='request')return request(input);
    if(input.operation==='poll')return poll(input);
    if(input.operation==='clipboard')return clipboard(input);
    return {message:exited?'Автоклеймер остановлен':'Автоклеймер работает'};
  });
  swch.log('Автоклеймер запущен отдельным процессом.');
  return async ()=>{
    child.stdin.end();
    let timer;
    await Promise.race([exit,new Promise(resolve=>{timer=setTimeout(()=>{child.kill();resolve();},7000);})]);
    clearTimeout(timer);
  };
}
