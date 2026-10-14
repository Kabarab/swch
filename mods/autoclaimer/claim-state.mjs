const key=a=>`${a.launcher}:${a.id||a.account_id}`;
export const isBrowserChallenge=row=>row.status==='attention'&&/captcha|капч|cloudflare|проверк[а-я]*\s+(браузера|безопасности)|security check/i.test(row.message);
export function indexedResults(data){
  const map=new Map();
  for(const source of [data.account_results||[],data.run?.results||[]]){
    const group=new Map();for(const row of source){const id=key(row);if(!group.has(id))group.set(id,[]);group.get(id).push(row);}
    for(const [id,rows] of group)map.set(id,rows);
  }
  return map;
}
export function accountState(account,data,index=indexedResults(data)){
  const config=data.settings||{};
  const enabled=account.launcher==='steam'?config.steam:account.launcher==='epic'?config.epic:(config.platforms||[]).includes(account.launcher);
  const verified=account.store_verified===false?null:(data.verified_accounts||[]).find(v=>v.launcher===account.launcher&&v.account_id===account.id)?.at;
  const rows=(index.get(key(account))||[]).filter(r=>!verified||!r.updated_at||Date.parse(r.updated_at)>Date.parse(verified));
  if(account.disabled)return {kind:'off',label:'Отключён для раздач',detail:'Этот аккаунт пропускается при проверке.',action:'connect'};
  if(!enabled)return {kind:'off',label:'Магазин выключен',detail:'Включи магазин во вкладке «Автоматизация».',action:'options'};
  if(account.launcher==='epic'&&config.epic_launcher_claims){
    const problem=rows.find(r=>['attention','error','needs_login'].includes(r.status));
    return {kind:problem?'attention':'saved',label:problem?'Есть остановленная попытка':'Аккаунт для Epic Launcher',detail:config.browser_visible?'SWCH проверит сохранённый доступ и попробует получение в окне Epic Launcher. Оставь магазин в фокусе и не используй мышь и клавиатуру во время оформления.':'Обычная проверка работает в фоне. Кнопка «Получить через Epic Launcher» отдельно запускает получение с окном; на это время оставь компьютер свободным.',action:'check',message:problem?.message};
  }
  const challenge=rows.find(isBrowserChallenge);
  if(challenge)return {kind:'attention',label:'Нужна проверка магазина',detail:account.launcher==='epic'?'Открой обычное окно магазина этого аккаунта. Повторная проверка в управляемом браузере может снова показать ту же CAPTCHA.':'Магазин требует проверки безопасности. Вход и получение пока не подтверждены.',action:account.launcher==='epic'?'manual':'verify',message:challenge.message};
  const login=rows.find(r=>r.status==='needs_login');
  if(login||!account.connected&&(!account.automatic||account.launcher==='epic'))return {kind:'login',label:'Нужен вход в магазин',detail:'Профиль лаунчера добавлен, но вход для получения раздач не подтверждён.',action:account.launcher==='epic'?'verify':'connect',message:login?.message};
  const problem=rows.find(r=>['attention','error'].includes(r.status));
  if(problem)return {kind:'attention',label:'Получение не подтверждено',detail:/библиотек/i.test(problem.message)?'Магазин не ответил на проверку библиотеки. Лицензия пока не подтверждена.':'Последняя попытка остановилась. Посмотри причину и повтори проверку.',action:'retry',message:problem.message};
  if(!account.connected)return {kind:'automatic',label:'Можно подключить автоматически',detail:'SWCH использует сохранённый вход этого аккаунта при проверке.',action:'connect'};
  if(account.launcher==='epic'&&!verified&&!account.store_verified&&(account.store_verified===false||!rows.some(r=>['claimed','owned'].includes(r.status))))return {kind:'saved',label:'Доступ Epic сохранён',detail:'SWCH может подключить магазин по сохранённому ключу. Вход в браузере ещё не подтверждён.',action:'verify'};
  return {kind:'ready',label:account.source==='native'?'Подключён автоматически':'Готов к проверке',detail:rows.some(r=>r.status==='no_offers')?'Вход работает. Сейчас подходящих раздач в регионе нет.':'Вход магазина сохранён. SWCH проверит условия раздачи перед получением.',action:'check'};
}
export function runSummary(data){
  const rows=data.run?.results||[];
  return {added:rows.filter(r=>r.status==='claimed'&&r.message==='Игра добавлена в библиотеку.').length,
    owned:rows.filter(r=>r.status==='owned'||r.status==='claimed'&&r.message!=='Игра добавлена в библиотеку.').length,
    keys:rows.filter(r=>r.status==='code_received').length,issues:rows.filter(r=>['attention','needs_login','error'].includes(r.status)).length,
    noOffers:new Set(rows.filter(r=>r.status==='no_offers').map(r=>key(r))).size};
}
