import {accountState,indexedResults,runSummary,isBrowserChallenge} from './claim-state.mjs?v=epic-recovery-11';
// Settings live in the backend so closing the window does not stop the job.
let api, esc, dateLabel, toast, demo;
export function initAutoClaim(environment) {
  ({api, esc, dateLabel, toast, demo} = environment);
  selectClaimView('games');
  if(!demo)setInterval(()=>{void notifyNewResults();},60000);
}
let noticeStarted=false,lastFinished=null,lastNotice='';
async function notifyNewResults(){
  try{
    const data=await api('/api/free-games');const run=data.run||{};
    if(!noticeStarted){noticeStarted=true;lastFinished=run.finished_at;return;}
    if(run.running||!run.finished_at||run.finished_at===lastFinished)return;
    lastFinished=run.finished_at;
    if(!data.settings?.notifications)return;
    const important=(run.results||[]).filter(r=>['attention','needs_login','error','code_received'].includes(r.status)||r.status==='claimed'&&r.message==='Игра добавлена в библиотеку.');
    const signature=JSON.stringify(important.map(r=>[r.account_id,r.offer_id,r.status,r.message]));
    if(!important.length||signature===lastNotice)return;lastNotice=signature;
    const added=important.filter(r=>r.status==='claimed').length,keys=important.filter(r=>r.status==='code_received').length,attention=important.length-added-keys;
    const text=`Проверка раздач завершена · игр получено: ${added} · ключей: ${keys} · нужно внимание: ${attention}`;
    if(globalThis.Notification?.permission==='granted')new Notification(t('SWCH — раздачи игр'),{body:t(text)});
    else toast(text);
  }catch{}
}
let autoClaimPoll = null;
let autoClaimBusy = false;
let autoClaimData = null;
const claimLabels = {claimed: 'Получена', owned: 'Уже в библиотеке', key_saved:'Ключ уже сохранён', no_offers: 'Нет раздач',
  needs_login: 'Нужен вход', attention: 'Нужно внимание', error: 'Ошибка проверки',disabled:'Отключён',preview:'Пробный запуск',code_received:'Ключ получен',redeemed:'Активирован',redeeming:'Активация проверяется'};
const boolOptions=['run_on_startup','dry_run','gamerpower','epic_mobile','browser_enabled','browser_visible','notifications','steamdb','external_notifications','epic_launcher_claims'];
const numberOptions=['retry_attempts','retry_backoff_seconds','browser_login_timeout_seconds','browser_operation_timeout_seconds'];
function renderAutoClaim(data) {
  autoClaimData = data;
  const config = data.settings || {};
  for (const name of ['enabled', 'steam', 'epic']) {
    const input = document.getElementById(`auto-claim-${name}`);
    input.checked = config[name] === true;
    input.disabled = autoClaimBusy || demo;
  }
  const interval = document.getElementById('auto-claim-interval');
  if (config.interval_minutes && !Array.from(interval.options).some(o => +o.value === config.interval_minutes)) {
    interval.add(new Option(`${config.interval_minutes} минут`, String(config.interval_minutes)));
  }
  interval.value = String(config.interval_minutes || 60);
  interval.disabled = autoClaimBusy || demo;
  for(const name of boolOptions) {const e=document.getElementById(`auto-claim-${name}`);if(e){e.checked=!!config[name];e.disabled=autoClaimBusy||demo;}}
  for(const name of [...numberOptions,'schedule_mode','timezone','daily_times','browser_name']) {const e=document.getElementById(`auto-claim-${name}`);if(e){if(document.activeElement!==e)e.value=name==='daily_times'?(config[name]||[]).join(', '):config[name]??(name==='browser_name'?'auto':'');e.disabled=autoClaimBusy||demo;}}
  const specs=data.platforms||[];
  const platformNames=new Map(specs.map(p=>[p.id,p.name]));
  const platforms=document.getElementById('auto-claim-platforms');
  if(platforms)platforms.innerHTML=specs.filter(p=>!['steam','epic'].includes(p.id)).map(p=>`<label><input type="checkbox" data-claim-platform="${esc(p.id)}" ${(config.platforms||[]).includes(p.id)?'checked':''} ${autoClaimBusy||demo?'disabled':''}> ${esc(p.name)}${p.delivery==='notify_only'?' · вручную':''}</label>`).join('');
  const addPlatform=document.getElementById('auto-claim-add-platform');
  if(addPlatform&&!addPlatform.options.length)addPlatform.innerHTML=specs.filter(p=>!['steam','epic'].includes(p.id)).map(p=>`<option value="${esc(p.id)}">${esc(p.name)}</option>`).join('');
  const run=data.run||{},index=indexedResults(data),states=(data.accounts||[]).map(a=>({account:a,state:accountState(a,data,index)}));
  const summary=runSummary(data),attention=states.filter(v=>['attention','login'].includes(v.state.kind)),available=states.filter(v=>['ready','saved','automatic'].includes(v.state.kind));
  document.getElementById('claim-summary').innerHTML=[['Можно проверить',available.length,'Сохранённый или доступный автовход'],['Требуют действий',attention.length,'Нужен вход или проверка магазина'],['Получено в этот запуск',summary.added,'Только подтверждённые лицензии']].map(([label,value,detail])=>`<div class="claim-stat"><small>${esc(label)}</small><strong>${value}</strong><span>${esc(detail)}</span></div>`).join('');
  document.getElementById('claim-accounts-count').textContent=states.length;
  document.getElementById('claim-automation-badge').textContent=config.enabled?(config.browser_visible?'Автоматически · с окнами':'Автоматически · в фоне'):'Только ручной запуск';
  const check=document.getElementById('auto-claim-check');check.disabled=!!run.running||autoClaimBusy||demo;check.textContent=config.browser_visible?'Проверить и забрать сейчас':'Проверить и забрать в фоне';
  const stop=document.getElementById('auto-claim-stop');stop.hidden=!run.running;stop.disabled=autoClaimBusy;
  const current=run.current;const stages={login:'Проверяем вход',discovery:'Ищем раздачи',claim:'Проверяем владение и оформляем игру'};
  let headline=run.running?'Проверка выполняется':run.cancelled?'Проверка остановлена':run.finished_at?`Проверка завершена · получено игр: ${summary.added}`:'Проверь раздачи для своих аккаунтов';
  if(config.dry_run)headline+=' · пробный режим без получения';
  const status=document.getElementById('auto-claim-status');if(status.textContent!==headline)status.textContent=headline;
  let detail=run.running?(current?`${stages[current.stage]||'Проверяем'} · ${current.account_name}${current.title?' · '+current.title:''}`:`Есть результаты для ${new Set((run.results||[]).map(r=>r.launcher+':'+r.account_id)).size} аккаунтов. Остальные ожидают своей очереди.`):run.error||`Последняя проверка: ${escTextDate(run.finished_at)}${run.next_check_at?' · следующая '+dateLabel(run.next_check_at):''}`;
  document.getElementById('claim-run-detail').textContent=detail;
  const next=document.getElementById('claim-next-steps');next.hidden=!attention.length&&!summary.noOffers&&!config.dry_run;
  next.innerHTML=`${config.dry_run?'<p><strong>Включён пробный режим.</strong> Он ищет раздачи, но не получает игры. Выключи его во вкладке «Автоматизация».</p>':''}${attention.length?`<p><strong>${attention.length} аккаунтов требуют действий.</strong> Открой «Аккаунты»: у каждого есть причина и кнопка для продолжения. <button type="button" class="text-button" data-claim-show="accounts">Перейти к аккаунтам →</button></p>`:''}${summary.noOffers?`<p>Для ${summary.noOffers} Steam/других аккаунтов подходящих раздач сейчас нет. Это не ошибка входа.</p>`:''}`;
  renderAccounts(states,platformNames,run);
  const groups=new Map();for(const row of run.results||[]){if(!row.title)continue;const key=row.launcher+':'+row.offer_id;if(!groups.has(key))groups.set(key,[]);groups.get(key).push(row);}
  const results=document.getElementById('auto-claim-results');
  results.innerHTML=[...groups.values()].map(rows=>{const first=rows[0];return `<article class="claim-game"><div class="claim-game-heading"><span class="claim-store-logo">${esc((platformNames.get(first.launcher)||first.launcher).slice(0,1))}</span><div><strong>${esc(first.title)}</strong><small>${esc(platformNames.get(first.launcher)||first.launcher)} · игра из текущей проверки</small></div>${safeOfferLink(first)?`<a class="claim-store-link" href="${esc(first.url)}" target="_blank" rel="noopener noreferrer">Страница игры ↗</a>`:''}</div><div class="claim-game-accounts">${rows.map(row=>`<div class="auto-claim-result ${['attention','error','needs_login'].includes(row.status)?'claim-attention':''}"><strong>${esc(row.account_name)}</strong><span class="claim-pill ${['attention','error','needs_login'].includes(row.status)?'warning':''}">${esc(claimLabels[row.status]||row.status)}</span><small>${esc(row.message)}</small>${epicRecoveryActions(row,config)}</div>`).join('')}</div></article>`;}).join('')||`<div class="claim-empty"><span>◇</span><strong>${run.running?'Ищем подходящие раздачи':summary.noOffers?'Подходящих раздач не найдено':'Здесь появятся игры из проверки'}</strong><p>${run.running?'Результаты появляются по мере проверки аккаунтов.':summary.noOffers?'Ищем временные скидки 100% на платные игры. Постоянные бесплатные игры пропускаем.':'Нажми «Проверить и забрать сейчас». Для получения нужен вход в магазин.'}</p></div>`;
  if(run.warnings?.length)results.insertAdjacentHTML('beforeend',`<details class="claim-source-notes"><summary>Некоторые дополнительные источники недоступны (${run.warnings.length})</summary>${run.warnings.map(w=>`<p>${esc(w)}</p>`).join('')}</details>`);
  const names = new Map((data.accounts || []).map(a => [a.id, a.name]));
  document.getElementById('auto-claim-history').innerHTML = (data.history || []).slice(0, 100).map(row => `<div class="auto-claim-result"><strong>${esc(row.game_title)}</strong><small>${esc(names.get(row.account_id) || row.account_id)} · ${esc(row.launcher)} · ${esc(dateLabel(row.claimed_at))}</small></div>`).join('') || '<p class="muted small-text">Здесь появятся подтверждённые получения игр.</p>';
  const keys=document.getElementById('auto-claim-keys');
  if(keys)keys.innerHTML=(data.keys||[]).map(r=>{
    const targets=(data.accounts||[]).filter(a=>!a.disabled&&['steam','gog','microsoft'].includes(a.launcher)&&(a.launcher!=='gog'||a.id.startsWith('gog_'))&&(!r.target_platform||r.target_platform===a.launcher));
    const options=targets.map(a=>`<option value="${esc(a.launcher+':'+a.id)}" ${r.target_account===a.id?'selected':''}>${esc(platformNames.get(a.launcher)||a.launcher)} · ${esc(a.name)}</option>`).join('');
    return `<div class="auto-claim-result"><strong>${esc(r.title)}</strong><small>${esc(names.get(r.source_account)||r.source_account)} · ${esc(r.target_platform||'Магазин указан на странице выдачи')} · ${esc(claimLabels[r.status]||r.status)}</small><button type="button" class="text-button" data-claim-key="${esc(r.id)}">Копировать ключ</button>${targets.length&&r.status!=='redeemed'?`<select data-claim-key-account="${esc(r.id)}" ${r.target_account?'disabled':''}><option value="">Один аккаунт для этого ключа</option>${options}</select><button type="button" class="text-button" data-claim-key-redeem="${esc(r.id)}">Активировать ключ</button>`:''}</div>`;
  }).join('')||'<p class="muted small-text">Полученные одноразовые ключи появятся здесь.</p>';
  document.querySelectorAll('#auto-claim-settings button[data-claim-repair],#auto-claim-settings button[data-claim-account-check],#auto-claim-settings button[data-claim-manual],#auto-claim-settings button[data-claim-library]').forEach(b=>b.disabled=autoClaimBusy||demo);
}
function epicRecoveryActions(row,config){
  if(row.launcher!=='epic'||!['attention','needs_login'].includes(row.status))return '';
  const id=esc(row.account_id),manual=`<button type="button" class="text-button" data-claim-manual="${id}" data-platform="epic">Открыть обычный браузер</button>`;
  if(config.epic_launcher_claims)return `<button type="button" class="text-button" data-claim-account-check="${id}" data-claim-window="true" data-platform="epic">Получить через Epic Launcher</button>${manual}`;
  if(isBrowserChallenge(row))return `${manual}<button type="button" class="text-button" data-claim-library="${id}" data-platform="epic">Проверить библиотеку</button>`;
  return `<button type="button" class="text-button" data-claim-repair="${id}" data-platform="epic">Проверить вход автоматически</button>${manual}`;
}
function safeOfferLink(row){try{const u=new URL(row.url);const domains={steam:['steampowered.com'],epic:['epicgames.com'],gog:['gog.com'],prime:['amazon.com'],microsoft:['microsoft.com','xbox.com'],ubisoft:['ubisoft.com'],fanatical:['fanatical.com'],itchio:['itch.io'],indiegala:['indiegala.com'],alienware:['alienwarearena.com']}[row.launcher]||[];return u.protocol==='https:'&&!u.username&&!u.password&&!u.port&&domains.some(d=>u.hostname===d||u.hostname.endsWith('.'+d));}catch{return false;}}
export async function loadAutoClaim() {
  if (!document.getElementById('auto-claim-settings')) return;
  if (demo) {
    renderAutoClaim({settings: {enabled: false, steam: true, epic: true, interval_minutes: 60}, accounts: [], run: {}, history: []});
    document.getElementById('auto-claim-status').textContent = 'Автоклеймер доступен при подключении к серверу SWCH.';
    return;
  }
  try { renderAutoClaim(await api('/api/free-games')); }
  catch (error) { document.getElementById('auto-claim-status').textContent = error.message; }
  if (!autoClaimPoll) autoClaimPoll = setInterval(() => {
    if (!document.getElementById('auto-claim-settings')) { clearInterval(autoClaimPoll); autoClaimPoll = null; return; }
    if (!autoClaimBusy) void loadAutoClaim();
  }, 4000);
}
document.getElementById('auto-claim-settings')?.addEventListener('change', async event => {
  if ((!event.target.id.startsWith('auto-claim-')&&!event.target.dataset.claimPlatform)||event.target.id.startsWith('auto-claim-add-') || demo || autoClaimBusy) return;
  const settings = {...autoClaimData?.settings,enabled: document.getElementById('auto-claim-enabled').checked,
    steam: document.getElementById('auto-claim-steam').checked, epic: document.getElementById('auto-claim-epic').checked,
    interval_minutes: Number(document.getElementById('auto-claim-interval').value)};
  for(const name of boolOptions)settings[name]=document.getElementById(`auto-claim-${name}`).checked;
  for(const name of numberOptions)settings[name]=Number(document.getElementById(`auto-claim-${name}`).value);
  settings.browser_name=document.getElementById('auto-claim-browser_name').value;
  settings.schedule_mode=document.getElementById('auto-claim-schedule_mode').value;
  settings.timezone=document.getElementById('auto-claim-timezone').value.trim()||'local';
  settings.daily_times=document.getElementById('auto-claim-daily_times').value.split(',').map(s=>s.trim()).filter(Boolean);
  settings.platforms=Array.from(document.querySelectorAll('[data-claim-platform]:checked')).map(e=>e.dataset.claimPlatform);
  if(event.target.id==='auto-claim-notifications'&&event.target.checked&&globalThis.Notification?.permission==='default')await Notification.requestPermission().catch(()=>{});
  autoClaimBusy = true;
  try { await api('/api/free-games/settings', settings); toast('Настройки автоклеймера сохранены'); }
  catch (error) { toast(error.message); }
  finally { autoClaimBusy = false; await loadAutoClaim(); }
});
document.getElementById('auto-claim-settings')?.addEventListener('click', async event => {
  const button = event.target.closest('button');
  if (!button || demo || autoClaimBusy) return;
  if(button.dataset.claimShow){selectClaimView(button.dataset.claimShow);return;}
  if(button.id==='auto-claim-export'){const blob=new Blob([JSON.stringify({run:autoClaimData?.run,history:autoClaimData?.history},null,2)],{type:'application/json'});const url=URL.createObjectURL(blob);const a=document.createElement('a');a.href=url;a.download='swch-giveaways.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);return;}
  if (!['auto-claim-check','auto-claim-stop','auto-claim-add','auto-claim-catalog-refresh'].includes(button.id) && !button.dataset.claimManual&&!button.dataset.claimLibrary&&!button.dataset.claimRepair&&!button.dataset.claimAccountCheck && !button.dataset.claimConnect && !button.dataset.claimDisconnect && !button.dataset.claimBrowser&&!button.dataset.claimWeb&&!button.dataset.claimWebDisconnect&&!button.dataset.claimVerify&&!button.dataset.claimKey&&!button.dataset.claimKeyRedeem) return;
  autoClaimBusy = true; button.disabled = true;
  try {
    if(button.dataset.claimManual||button.dataset.claimLibrary){
      const manual=!!button.dataset.claimManual,id=button.dataset.claimManual||button.dataset.claimLibrary,platform=button.dataset.platform;
      if(manual&&autoClaimData?.run?.running){await api('/api/free-games/cancel',{});for(let i=0;i<30;i++){const state=await api('/api/free-games');if(!state.run?.running)break;await new Promise(resolve=>setTimeout(resolve,300));}}
      const data=await api(`/api/free-games/web/${manual?'manual':'library'}`,{platform,account_id:id});
      selectClaimView('accounts');const panel=document.getElementById('claim-manual-status'),games=data.games||[],owned=games.filter(g=>g.owned).length;
      panel.hidden=false;panel.innerHTML=`<strong>${esc(data.account_name||'Epic Games')}</strong><p>${manual?'Открыто обычное окно без управления SWCH. Войди в этот аккаунт и получи игры кнопками Get → Add to library. Другие игры открывай в этом же окне магазина. Затем нажми «Проверить библиотеку» в SWCH. Перед автоматическим повтором закрой это окно.':`Epic подтвердил в библиотеке этого аккаунта ${owned} из ${games.length} текущих раздач. Эта проверка не оформляет заказов.`}</p>${games.map(g=>`<div><span>${esc(g.title)} — ${g.owned?'в библиотеке':'лицензия пока не найдена'}</span></div>`).join('')}${!games.length?'<p>Подходящих раздач в регионе этого аккаунта сейчас нет.</p>':''}`;
      panel.insertAdjacentHTML('beforeend',`<button type="button" class="secondary" data-claim-library="${esc(id)}" data-platform="${esc(platform)}">Проверить библиотеку</button>`);panel.scrollIntoView({block:'nearest'});
      toast(manual?'Открыто обычное окно выбранного аккаунта':`В библиотеке: ${owned} из ${games.length}`);
    }else if(button.id==='auto-claim-stop'){await api('/api/free-games/cancel',{});toast('Останавливаем текущую проверку');
    }else if(button.dataset.claimRepair||button.dataset.claimAccountCheck){
      if(autoClaimData?.run?.running){await api('/api/free-games/cancel',{});for(let i=0;i<30;i++){const state=await api('/api/free-games');if(!state.run?.running)break;await new Promise(resolve=>setTimeout(resolve,300));}}
      const id=button.dataset.claimRepair||button.dataset.claimAccountCheck,platform=button.dataset.platform;
      if(button.dataset.claimRepair){document.getElementById('claim-manual-status').hidden=true;document.getElementById('auto-claim-status').textContent='Подключаем вход по сохранённому ключу и проверяем магазин в отдельном окне…';toast('Проверяем вход выбранного аккаунта');await api('/api/free-games/web/verify',{platform,account_id:id});selectClaimView('accounts');toast('Вход в магазин подтверждён. Можно запускать получение игр.');}
      else {
      const visible=!!button.dataset.claimRepair||button.dataset.claimWindow==='true'||autoClaimData?.settings?.browser_visible===true;
      await api('/api/free-games/check',{platform,account_id:id,visible});selectClaimView('games');toast(visible?'Получение запущено для выбранного аккаунта':'Фоновое получение запущено для выбранного аккаунта');
      }
    }else if(button.id==='auto-claim-catalog-refresh'){
      const data=await api('/api/free-games/catalog');
      document.getElementById('auto-claim-catalog').innerHTML=(data.candidates||[]).map(c=>`<div class="auto-claim-result"><strong>${esc(c.title)}</strong><small>${esc(c.platform)} · до ${esc(dateLabel(c.ends_at*1000))} · цену проверим в магазине</small>${safeOfferLink({launcher:c.platform,url:c.claim_url})?`<a href="${esc(c.claim_url)}" target="_blank" rel="noopener noreferrer">Открыть раздачу ↗</a>`:''}</div>`).join('')||'<p class="muted small-text">Нет подходящих временных раздач.</p>';
    }else if(button.dataset.claimKeyRedeem){
      const account=button.closest('.auto-claim-result').querySelector('[data-claim-key-account]').value;
      if(!account){toast('Выбери один аккаунт для активации ключа');return;}
      const separator=account.indexOf(':');await api('/api/free-games/keys/redeem',{id:button.dataset.claimKeyRedeem,platform:account.slice(0,separator),account_id:account.slice(separator+1)});toast('Лицензия выбранного аккаунта подтверждена');
    }else if(button.dataset.claimKey){
      const data=await api('/api/free-games/keys/reveal',{id:button.dataset.claimKey});
      await copyText(data.code);toast('Ключ скопирован для активации на одном аккаунте');
    }else if(button.id==='auto-claim-add'){
      toast('Открываем отдельный браузер: выполни вход и открой профиль магазина');
      await api('/api/free-games/web/add',{platform:document.getElementById('auto-claim-add-platform').value,identity:document.getElementById('auto-claim-add-identity').value.trim(),country:document.getElementById('auto-claim-add-country').value.trim().toUpperCase()});
      toast('Магазин подключён');
    }else if(button.dataset.claimVerify){
      toast('Пройди проверку безопасности в отдельном окне магазина');await api('/api/free-games/web/verify',{platform:button.dataset.platform,account_id:button.dataset.claimVerify});toast('Сессия магазина сохранена');
    }else if(button.dataset.claimWeb||button.dataset.claimWebDisconnect){
      const disconnect=!!button.dataset.claimWebDisconnect;
      await api(`/api/free-games/web/${disconnect?'disconnect':'connect'}`,{platform:button.dataset.platform,account_id:button.dataset.claimWeb||button.dataset.claimWebDisconnect,...(!disconnect?{identity:button.closest('.auto-claim-account').querySelector('[data-claim-login]')?.value?.trim()||undefined}:{})});toast(disconnect?'Вход магазина отключён':'Вход магазина подключён');
    }else if (button.id === 'auto-claim-check') {
      await api('/api/free-games/check', {visible:autoClaimData?.settings?.browser_visible===true}); toast('Проверка запущена. Причина остановки будет показана для каждого аккаунта.');
    } else if (button.dataset.claimDisconnect) {
      await api('/api/free-games/steam/disconnect', {account_id: button.dataset.claimDisconnect}); toast('Вход магазина Steam отключён');
    } else if (button.dataset.claimConnect) {
      document.getElementById('auto-claim-status').textContent = 'Подключаем сохранённый вход Steam автоматически…';
      await api('/api/free-games/steam/automatic',{account_id:button.dataset.claimConnect});
      toast('Steam подключён автоматически');
    } else {
      const connection = await api('/api/free-games/steam/start', {account_id: button.dataset.claimBrowser});
      const panel = document.getElementById('auto-claim-status');
      panel.textContent = 'Открой подключение в Chrome или Edge с расширением SWCH: ';
      const link = document.createElement('a');
      link.href = connection.url; link.target = '_blank'; link.rel = 'noopener noreferrer';
      link.textContent = 'Подключить магазин Steam ↗'; panel.append(link);
      // Allow copying into the browser when the desktop WebView owns the link.
      const copy = document.createElement('button'); copy.className = 'text-button'; copy.type = 'button'; copy.textContent = 'Копировать ссылку';
      copy.addEventListener('click', () => copyText(connection.url).then(() => toast('Ссылка скопирована')).catch(() => toast('Скопируй адрес ссылки через меню браузера')));
      panel.append(' ', copy);
      clearInterval(autoClaimPoll); autoClaimPoll = null;
      return;
    }
  } catch (error) {
    toast(error.message);
    if(button.dataset.claimRepair){
      selectClaimView('accounts');const panel=document.getElementById('claim-manual-status');
      const account=autoClaimData?.accounts?.find(a=>a.id===button.dataset.claimRepair&&a.launcher===button.dataset.platform);
      panel.hidden=false;panel.textContent=`${account?.name||'Epic Games'} · Вход не подтверждён. ${error.message}`;panel.scrollIntoView({block:'nearest'});
    }
  }
  finally { autoClaimBusy = false; button.disabled = false; }
  await loadAutoClaim();
});
async function loadNotificationChannels(){
  if(demo)return;
  const data=await api('/api/free-games/notifications');
  document.getElementById('claim-notify-status').textContent=data.channels?.length?'Подключены: '+data.channels.join(', '):'Внешние каналы не подключены';
}
document.getElementById('claim-notify-panel')?.addEventListener('toggle',event=>{if(event.target.open)void loadNotificationChannels().catch(error=>toast(error.message));});
document.getElementById('claim-notify-save')?.addEventListener('click',async event=>{
  if(demo)return;const button=event.target;button.disabled=true;
  const value=name=>document.getElementById('claim-notify-'+name).value.trim();
  const update={};if(value('discord'))update.discord=value('discord');if(value('webhook'))update.webhook=value('webhook');
  if(value('token')||value('chat'))update.telegram={token:value('token'),chat_id:value('chat')};
  if(value('host')||value('user')||value('pass')||value('from')||value('to'))update.email={host:value('host'),port:Number(value('port')),username:value('user'),password:document.getElementById('claim-notify-pass').value,from:value('from'),to:value('to')};
  try{await api('/api/free-games/notifications',update);for(const input of document.querySelectorAll('#claim-notify-panel input'))input.value='';await loadNotificationChannels();toast('Каналы сохранены. Включи отправку сводки в параметрах автоклеймера.');}catch(error){toast(error.message);}finally{button.disabled=false;}
});
document.getElementById('claim-notify-clear')?.addEventListener('click',async event=>{
  if(demo)return;event.target.disabled=true;
  try{await api('/api/free-games/notifications/disconnect',{});for(const input of document.querySelectorAll('#claim-notify-panel input'))input.value='';await loadNotificationChannels();toast('Все внешние каналы удалены');}catch(error){toast(error.message);}finally{event.target.disabled=false;}
});

function escTextDate(value){return value?dateLabel(value):'ещё не запускалась';}
let claimView='games';

function selectClaimView(view){claimView=view;document.getElementById('auto-claim-settings').dataset.view=view;for(const button of document.querySelectorAll('[data-claim-view]')){const active=button.dataset.claimView===view;button.setAttribute('aria-selected',String(active));button.tabIndex=active?0:-1;document.getElementById('claim-panel-'+button.dataset.claimView).hidden=!active;}}
function epicAccountExtras(account,state){
  if(account.launcher!=='epic'||state.kind==='off')return '';
  const id=esc(account.id),library=`<button type="button" class="text-button" data-claim-library="${id}" data-platform="epic">Проверить библиотеку</button>`;
  return state.action==='manual'?library:`<button type="button" class="secondary" data-claim-manual="${id}" data-platform="epic">Открыть обычный браузер</button>${library}`;
}
function renderAccounts(states,names,run){
  const root=document.getElementById('auto-claim-accounts');if(root.contains(document.activeElement)&&document.activeElement.matches('input,select'))return;
  const search=document.getElementById('claim-account-search').value.trim().toLowerCase();
  const filtered=states.filter(({account:a})=>(a.name+' '+(names.get(a.launcher)||a.launcher)).toLowerCase().includes(search));
  const groups=[['Требуют твоего действия',filtered.filter(v=>['attention','login'].includes(v.state.kind))],['Доступны для проверки',filtered.filter(v=>['ready','saved','automatic'].includes(v.state.kind))],['Отключены',filtered.filter(v=>v.state.kind==='off')]];
  root.innerHTML=groups.filter(([,list])=>list.length).map(([heading,list])=>`<section class="claim-account-group"><h5>${esc(heading)} <span>${list.length}</span></h5><div class="claim-account-grid">${list.map(({account:a,state})=>{
    const rowKey=esc(a.id),platform=esc(a.launcher),name=esc(names.get(a.launcher)||a.launcher);
    let primary=state.action==='options'?'<button type="button" class="secondary" data-claim-show="options">Включить магазин</button>':state.action==='verify'&&a.launcher==='epic'?`<button type="button" class="primary" data-claim-repair="${rowKey}" data-platform="epic">Проверить вход автоматически</button>`:state.action==='connect'?a.launcher==='steam'?`<button type="button" class="primary" data-claim-connect="${rowKey}">Подключить автоматически</button>`:`<button type="button" class="primary" data-claim-web="${rowKey}" data-platform="${platform}">${a.automatic?'Подключить автоматически':'Войти в магазин'}</button>`:`<button type="button" class="secondary" data-claim-account-check="${rowKey}" data-platform="${platform}">${state.action==='retry'?'Повторить для аккаунта':'Проверить и забрать'}</button>`;
    if(state.action==='manual')primary=`<button type="button" class="primary" data-claim-manual="${rowKey}" data-platform="epic">Открыть обычный браузер</button>`;
    if(a.launcher==='epic'&&autoClaimData?.settings?.epic_launcher_claims&&state.kind!=='off')primary=`<button type="button" class="primary" data-claim-account-check="${rowKey}" data-claim-window="true" data-platform="epic">Получить через Epic Launcher</button>`;
    return `<article class="auto-claim-account claim-account-card"><div class="claim-account-heading"><span class="claim-store-logo">${name.slice(0,1)}</span><div><strong>${esc(a.name)}</strong><small>${name}</small></div><span class="claim-pill ${['attention','login'].includes(state.kind)?'warning':state.kind==='ready'?'success':''}">${esc(state.label)}</span></div><p>${esc(state.detail)}</p>${a.connected&&state.kind==='attention'?'<small class="claim-session-note">Данные доступа сохранены. Вход и получение пока не подтверждены.</small>':''}${state.message?`<details class="claim-account-reason"><summary>Причина последней попытки</summary><p>${esc(state.message)}</p></details>`:''}${!a.connected&&!a.automatic&&a.launcher!=='gog'?`<label class="claim-login-field">Логин или email этого аккаунта<input data-claim-login="${rowKey}" placeholder="Укажи перед входом" maxlength="256"></label>`:''}<div class="claim-account-actions">${primary}${epicAccountExtras(a,state)}${a.launcher==='epic'&&state.action!=='verify'&&a.connected?`<button type="button" class="text-button" data-claim-repair="${rowKey}" data-platform="epic">Проверить вход после восстановления</button>`:''}${!a.disabled&&a.connected?`<button type="button" class="text-button" ${a.launcher==='steam'?'data-claim-disconnect':'data-claim-web-disconnect'}="${rowKey}" data-platform="${platform}">Отключить</button>`:''}</div>${a.launcher==='steam'?`<details class="auto-claim-fallback"><summary>Другие способы входа</summary><button type="button" class="text-button" data-claim-browser="${rowKey}">Через браузер</button>${state.action!=='connect'?`<button type="button" class="text-button" data-claim-connect="${rowKey}">Подключить автоматически</button>`:''}</details>`:''}</article>`;
  }).join('')}</div></section>`).join('')||'<p class="muted">Аккаунты не найдены.</p>';
}
document.querySelector('.claim-tabs')?.addEventListener('click',event=>{const b=event.target.closest('[data-claim-view]');if(b)selectClaimView(b.dataset.claimView);});
for(const selector of ['.claim-tabs'])document.querySelector(selector)?.addEventListener('keydown',event=>{if(!['ArrowLeft','ArrowRight','Home','End'].includes(event.key))return;const buttons=[...event.currentTarget.querySelectorAll('[role="tab"]')];let index=buttons.indexOf(document.activeElement);if(index<0)return;event.preventDefault();index=event.key==='Home'?0:event.key==='End'?buttons.length-1:(index+(event.key==='ArrowRight'?1:-1)+buttons.length)%buttons.length;buttons[index].click();buttons[index].focus();});
document.getElementById('claim-account-search')?.addEventListener('input',()=>{if(autoClaimData)renderAccounts((autoClaimData.accounts||[]).map(account=>({account,state:accountState(account,autoClaimData)})),new Map((autoClaimData.platforms||[]).map(p=>[p.id,p.name])),autoClaimData.run||{});});
import {t} from './i18n.js';
