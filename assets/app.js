'use strict';
(() => {
  const $ = id => document.getElementById(id);
  const el = (tag, text, cls) => { const n = document.createElement(tag); if (text !== undefined) n.textContent = text; if (cls) n.className = cls; return n; };
  const link = (text, href, cls) => { const n = el('a', text, cls); n.href = href; return n; };
  const button = (text, fn, cls) => { const n = el('button', text, cls); n.type = 'button'; n.onclick = fn; return n; };
  const labels = {up:'Up',down:'Down',pending_failure:'Pending failure',unknown:'Unknown',paused:'Paused'};
  const stamp = value => value == null ? 'Not observed' : new Date(value).toLocaleString(undefined, {timeZoneName:'short'});
  const pct = (n,d) => d ? `${(100*n/d).toFixed(2)}%` : 'N/A';
  const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const fmt = value => String(value ?? 'N/A').replaceAll('_', ' ');
  let data, csrf, timer, busy = false, stopped = false, failed = false, lastRefresh = null, range = 86400000;
  let editing = null, deleting = null, createKey = null, settingsLoaded = false;
  let historyBusy = false;
  const incidentPages = new WeakMap();
  const pendingControls = new WeakSet();
  const path = location.pathname;
  const selected = /^\/monitors\/(-?\d+)$/.exec(path)?.[1];
  const view = selected ? 'detail' : path === '/settings' ? 'settings' : path === '/incidents' ? 'incidents' : 'overview';
  document.body.dataset.view = view;
  document.querySelectorAll('.top nav a').forEach(a => { if (a.pathname === path) a.setAttribute('aria-current','page'); });
  // Update bounded display trees in place. Stable keys retain control identity,
  // open disclosures and focus; event properties receive the latest snapshot.
  function reconcile(target, source) {
    const active=document.activeElement, hadFocus=target.contains(active);
    function key(node){return node.nodeType===1 ? node.dataset.key||node.id||null : null;}
    function compatible(a,b){return a.nodeType===b.nodeType && (a.nodeType!==1||a.tagName===b.tagName);}
    function sync(parent,fresh){
      const old=[...parent.childNodes], used=new Set();let index=0;
      for(const next of [...fresh.childNodes]){
        const id=key(next);
        let node=id ? old.find(n=>key(n)===id&&compatible(n,next)) : old.find((n,i)=>!used.has(n)&&!key(n)&&i>=index&&compatible(n,next));
        if(!node){node=next;parent.insertBefore(node,parent.childNodes[index]||null);}
        else {
          used.add(node);
          if(parent.childNodes[index]!==node)parent.insertBefore(node,parent.childNodes[index]||null);
          if(node.nodeType===3){if(node.nodeValue!==next.nodeValue)node.nodeValue=next.nodeValue;}
          else {
            for(const attr of [...node.attributes])if(!next.hasAttribute(attr.name)&&!(attr.name==='open'&&node.tagName==='DETAILS')&&!(attr.name==='aria-busy'&&next.dataset.retain==='true')&&!(attr.name==='disabled'&&pendingControls.has(node)))node.removeAttribute(attr.name);
            for(const attr of [...next.attributes])if(node.getAttribute(attr.name)!==attr.value){
              if(attr.name==='style'){for(const property of [...next.style])node.style.setProperty(property,next.style.getPropertyValue(property));}
              else node.setAttribute(attr.name,attr.value);
            }
            node.onclick=next.onclick;
            if(next.dataset.retain!=='true')sync(node,next);
          }
        }
        index++;
      }
      for(const node of old)if(!used.has(node)&&node.parentNode===parent)node.remove();
    }
    sync(target,source);
    if(hadFocus && active.isConnected && document.activeElement!==active)active.focus({preventScroll:true});
    else if(hadFocus&&!active.isConnected){target.tabIndex=-1;target.focus({preventScroll:true});}
  }
  function status(m) { return el('span', labels[m.state.status] || 'Unknown', `status ${m.state.status}`); }
  function metric(value, label) { const n = el('div',undefined,'metric'); n.append(el('strong',value),el('span',label)); return n; }
  function announce(text) { $('announcement').textContent = text; }
  function message(text, error = false) { $('message').hidden = false; $('message').textContent = text; $('message').className = `notice ${error ? 'warning' : ''}`; }
  function parse(text) {
    // Preserve opaque signed 64-bit identifiers before JSON number conversion.
    return JSON.parse(text.replace(/"(?:\\.|[^"\\])*"|-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/g, token => /^-?\d+$/.test(token) && !Number.isSafeInteger(Number(token)) ? `"${token}"` : token));
  }
  function errorText(response) {
    const wait = response.headers.get('Retry-After') || '10';
    return ({401:'Your session expired. Sign in again to continue.',403:'Security check failed. Reload this page before trying again.',404:'This item no longer exists. Refresh the overview.',409:'This item changed, or scheduler capacity is full. Reload current values before saving again.',413:'The request is too large. Shorten the submitted values.',422:'Check the fields and allowed destinations. Private targets need an operator grant in the server configuration.',400:'The request is invalid. Check the entered values.',429:`Too many requests. Wait ${wait} seconds before trying again.`,503:`Service or storage is busy. Try again after ${wait} seconds.`})[response.status] || 'The request failed. Reload and try again.';
  }
  async function api(url, method = 'GET', body, key) {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 6500);
    try {
      const headers = {};
      if (method !== 'GET') { headers['Content-Type']='application/json'; headers['X-CSRF-Token']=csrf; }
      if (key) headers['Idempotency-Key']=key;
      const response = await fetch(url, {method, credentials:'same-origin',headers,body:body === undefined ? undefined : JSON.stringify(body),signal:controller.signal,cache:'no-store'});
      if (!response.ok) {
        if(response.status===401 && url==='/api/v1/account/password'){await api('/api/v1/account/session');throw new Error('Current password is incorrect. Try again.');}
        if (response.status === 401) { stopped=true; clearTimeout(timer); $('connection').hidden=false; $('connection').replaceChildren(el('span','Session expired. '),link('Sign in again','/login')); document.querySelectorAll('main button,#add,#logout').forEach(b => b.disabled=true); }
        throw new Error(response.status===422&&url.includes('/history')?'History range is invalid or outside retained observations. Reload the range and try again.':errorText(response));
      }
      const text = await response.text();
      return text ? parse(text) : null;
    } catch (e) { if (e instanceof TypeError || e.name === 'AbortError') throw new Error('Connection interrupted. Existing data may be stale. A submitted change may have committed; refresh before retrying.'); throw e; }
    finally { clearTimeout(timeout); }
  }
  function age(m) { const last=m.state.last_completed_ms; if(last==null)return 'No observation yet'; const seconds=Math.max(0,Math.floor((data.server_time_ms-last)/1000));return `${seconds < 60 ? seconds+' s' : Math.floor(seconds/60)+' min'} ago`; }
  function stale(m) { return m.config.enabled && (m.state.last_completed_ms == null || data.server_time_ms < m.state.last_completed_ms || data.server_time_ms-m.state.last_completed_ms > (2*m.config.interval_seconds+m.config.deadline_seconds)*1000); }
  function observed(m) { return `${age(m)}${stale(m) ? ' · Stale / unknown coverage' : ''}`; }
  function visible() { const term=$('search').value.toLocaleLowerCase();const filter=$('filter').value;return (data?.monitors||[]).filter(m => m.config.name.toLocaleLowerCase().includes(term)&&(filter==='all'||m.state.status===filter)).sort((a,b)=>['down','pending_failure','unknown','up','paused'].indexOf(a.state.status)-['down','pending_failure','unknown','up','paused'].indexOf(b.state.status)); }
  function latest(m,interactive=false) {
    const box=el('div',undefined,'heartbeat'), samples=data.recent?.[m.id]||[];
    box.setAttribute('aria-label','Recent individual observations, oldest to newest; no samples means unknown coverage');
    if(!samples.length){const bar=el('span',undefined,'beat unknown');bar.title='No recent observations';box.append(bar);return box;}
    for(const sample of samples){const state={success:'up',failure:'down',unknown:'unknown'}[sample.outcome]||'unknown';const text=`Scheduled ${stamp(sample.scheduled_ms)}; observed ${stamp(sample.observed_ms)}: ${fmt(sample.outcome)}`;const bar=interactive?button('',()=>message(text)):el('span');bar.className=`beat ${state}`;bar.dataset.key=`observation-${sample.scheduled_ms}`;bar.title=text;if(interactive)bar.setAttribute('aria-label',text);box.append(bar);}return box;
  }
  function renderList() {
    const monitors=visible();$('list-count').textContent=`${monitors.length} of ${data.monitors.length} monitors`;
    const list=el('div');list.append(...monitors.map(m => {const a=link('',`/monitors/${m.id}`,'row');a.dataset.key=`monitor-${m.id}`;if(String(m.id)===selected)a.setAttribute('aria-current','page');a.append(el('strong',m.config.name),status(m),el('small',observed(m)),latest(m));return a;}));reconcile($('monitor-list'),list);
  }
  function warnings() {
    const messages=[];
    if(data.scheduler_degraded)messages.push('Scheduler degraded. Observations may be missing; target status is separate.');
    if(data.persistence_faults || data.persistence_faults_this_process)messages.push(`Storage observation faults: ${data.persistence_faults}; this process: ${data.persistence_faults_this_process}. Missing observations are unknown.`);
    if(data.dropped_alerts)messages.push(`Suppressed alerts: ${data.dropped_alerts}. Incident history is retained.`);
    const t=data.telegram,s=t?.settings;
    if(t?.persistence_degraded)messages.push('Telegram acknowledgement storage degraded. Delivery is uncertain.');
    if(s?.configuration_failed)messages.push('Telegram configuration failed. Correct settings to resume delivery.');
    if(t?.delivery_delayed)messages.push(`Telegram delivery delayed: ${s.pending} pending since ${stamp(s.oldest_pending_ms)}.`);
    if(s?.cooldown_until_ms>data.server_time_ms)messages.push(`Telegram provider cooldown until ${stamp(s.cooldown_until_ms)}.`);
    if(s?.last_error)messages.push(`Telegram last error: ${fmt(s.last_error)}.`);
    const n=el('div');n.dataset.key='operational-warnings';for(const text of messages)n.append(el('p',text,'notice warning'));return n;
  }
  function refreshLine() { const n=el('p',`Last refreshed ${stamp(lastRefresh)} · ${timeZone} · refreshes every 15 seconds while visible`,'muted');n.dataset.key='refreshed';return n; }
  function overview() {
    const main=el('div');const monitors=visible();main.replaceChildren(el('div','YOUR SERVICES, AT A GLANCE','eyebrow'),el('h1','Overview'),refreshLine(),warnings());
    const metrics=el('div',undefined,'metrics');for(const key of Object.keys(labels))metrics.append(metric(String(monitors.filter(m=>m.state.status===key).length),labels[key]));main.append(metrics);
    if(!monitors.length){const c=el('section',undefined,'card empty');c.append(el('div','◒','symbol'),el('h2',data.monitors.length?'No matching monitors':'A quiet place for your services'),el('p',data.monitors.length?'Try another search or status filter.':'Add an endpoint to start collecting real observations. Availability appears after checks complete.'),button('Add monitor',()=>openEditor(), 'primary'));main.append(c);reconcile($('main'),main);return;}
    const grid=el('div',undefined,'monitor-grid');grid.dataset.key='monitor-grid';for(const m of monitors){const c=el('article',undefined,'card');c.dataset.key=`monitor-${m.id}`;c.append(status(m),el('h2'),link(m.config.name,`/monitors/${m.id}`),el('p',`${m.config.kind.toUpperCase()} · ${observed(m)}`,'muted'),latest(m,true),el('small','Recent individual observations · oldest → newest'));if(m.state.status==='pending_failure')c.append(el('p',`${m.state.failure_streak}/${m.config.failure_threshold} failures · next ${stamp(m.state.next_due_ms)}`));if(m.state.system_error)c.append(el('p',`Monitor system fault: ${fmt(m.state.system_error)}`,'error'));grid.append(c);}main.append(grid);reconcile($('main'),main);
  }
  function current() { return data?.monitors.find(m=>String(m.id)===selected); }
  async function act(n, work, errorNode) { pendingControls.add(n);n.disabled=true;try{await work();}catch(e){if(errorNode)errorNode.textContent=e.message;else message(e.message,true);}finally{pendingControls.delete(n);if(!stopped)n.disabled=false;} }
  async function detail() {
    const m=current(),main=el('div');main.replaceChildren(link('← Back to overview','/','back'));
    if(!m){main.append(el('h1','Monitor unavailable'),el('p','This monitor may have been deleted.'),link('Return to overview','/'));reconcile($('main'),main);return;}
    document.title=`${m.config.name} · minimalMonitor`;
    const head=el('div',undefined,'section-head'),title=el('div'),actions=el('div',undefined,'actions');title.append(el('div',`${m.config.kind.toUpperCase()} MONITOR`,'eyebrow'),el('h1',m.config.name),status(m));
    head.dataset.key='detail-heading';actions.append(button('Edit',()=>openEditor(m)),button(m.config.enabled?'Pause':'Resume',event=>act(event.currentTarget,async()=>{await api(`/api/v1/monitors/${m.id}/${m.config.enabled?'pause':'resume'}`,'POST',{revision:m.revision});await refresh(true);message(m.config.enabled?'Monitor paused.':'Monitor resumed.');})),button('Delete',()=>openDelete(m),'danger'));head.append(title,actions);main.append(head,refreshLine(),warnings());
    if(stale(m))main.append(el('p','Stale / unknown coverage. A previous success does not prove current reachability.','notice warning'));
    if(m.state.system_error)main.append(el('p',`Monitor system fault: ${fmt(m.state.system_error)}. This is separate from target downtime.`,'notice warning'));
    main.append(el('p',`Last observation: ${stamp(m.state.last_completed_ms)} · ${observed(m)}`,'muted'));
    if(m.state.status==='pending_failure')main.append(el('p',`Pending: ${m.state.failure_streak}/${m.config.failure_threshold} failures. Next confirmation: ${stamp(m.state.next_due_ms)} (30-second wait).`,'notice warning'));
    const info=el('section',undefined,'card');info.dataset.key='check-details';info.append(el('h2','Check details'),el('p',`Normal interval ${m.config.interval_seconds}s · deadline ${m.config.deadline_seconds}s · down after ${m.config.failure_threshold} failures · confirmation wait 30s`),el('p',`Last result: ${fmt(m.state.last_result?.outcome)} · reason: ${fmt(m.state.last_result?.error)} · HTTP status: ${fmt(m.state.last_result?.status_code)}`),el('p',`Next check: ${m.config.enabled?stamp(m.state.next_due_ms):'Paused'} · Telegram alerts ${m.config.alerts?'on':'off'}`));
    const target=el('details');target.append(el('summary','Configured target (may contain sensitive query values)'),el('code',`${m.config.target}${m.config.port?':'+m.config.port:''}`));info.append(target);main.append(info);
    const recent=el('section',undefined,'card');recent.dataset.key='recent-checks';recent.append(el('h2','Recent checks'),latest(m,true),el('p','Up to 12 individual observations, oldest to newest. Select or focus a bar for its timestamp; hatched bars mean unknown coverage.','muted'));main.append(recent);
    const history=el('section',undefined,'card');history.id='history';history.dataset.retain='true';main.append(history);
    const incidents=el('section',undefined,'card');incidents.id='detail-incidents';incidents.dataset.retain='true';main.append(incidents);reconcile($('main'),main);await loadHistory(m,$('history'));await refreshIncidents($('detail-incidents'),m.id);
  }
  async function loadHistory(m,targetBox) {
    if(historyBusy||stopped||document.hidden)return;historyBusy=true;targetBox.setAttribute('aria-busy','true');
    const box=el('div');
    box.replaceChildren(el('h2','Latency & observation coverage'));
    const ranges=el('div',undefined,'range');for(const [label,ms] of [['1h',3600000],['24h',86400000],['7d',604800000],['90d',7776000000]]){const b=button(label,async()=>{if(historyBusy)return;range=ms;targetBox.querySelectorAll('[data-range]').forEach(n=>n.setAttribute('aria-pressed',String(Number(n.dataset.range)===range)));await loadHistory(current()||m,targetBox);});b.dataset.range=ms;b.dataset.key=`range-${ms}`;b.setAttribute('aria-pressed',String(range===ms));ranges.append(b);}box.append(ranges,el('p','Loading history…'));
    const requestedRange=range;
    try{
      const h=await api(`/api/v1/monitors/${m.id}/history?range_ms=${requestedRange}`);
      const start=h.start_ms,end=h.end_ms;
      if(!targetBox.isConnected||requestedRange!==range||stopped)return;
      box.lastChild.remove();
      const counts=h.points.reduce((a,p)=>{for(const k of ['expected','successful','failed','unknown'])a[k]+=p.counts[k];return a;},{expected:0,successful:0,failed:0,unknown:0});
      m=current()||m;const metrics=el('div',undefined,'metrics');metrics.append(metric(m.state.last_result && m.state.last_result.outcome!=='unknown'?`${m.state.last_result.elapsed_ms} ms`:'N/A','Latest latency'),metric(pct(counts.successful,counts.successful+counts.failed),'Sample availability'),metric(pct(counts.successful+counts.failed,counts.expected),'Observation coverage'));box.append(metrics);
      box.append(el('p',`Aggregated buckets (${h.granularity}); ${stamp(h.start_ms)} – ${stamp(h.end_ms)}. Availability = successful / completed target checks. Coverage = completed / expected enabled slots. Paused time is excluded.`,'muted'));
      if(h.partial_boundary_buckets)box.append(el('p','Partial boundary buckets are included in this older window.','notice warning'));
      if(h.coverage_discontinuity)box.append(el('p','Observation gap: restart or clock change occurred in this window.','notice warning'));
      if(!h.points.length){box.append(el('p','No observations in this range. Availability and coverage are N/A.'));return;}
      const chart=el('div',undefined,'chart');chart.setAttribute('aria-label','Mean latency by aggregated bucket. Hatched buckets have unknown coverage. Values are also in the table below.');
      const max=Math.max(1,...h.points.map(p=>p.latency_count?p.latency_sum_ms/p.latency_count:0));
      box.append(el('p',`Mean latency scale: 0–${Math.ceil(max)} ms. Each column is one aggregated bucket; hatched columns show unknown coverage.`,'muted'));
      const rows=[];let previous=Math.floor(start/3600000)*3600000;const width=Math.max(3600000,Math.ceil(Math.ceil((end-start)/3600000)/299)*3600000);
      function gap(text){const wrap=el('div',undefined,'bucket');wrap.dataset.key=text;const b=button('');b.className='gap';b.title=text;b.setAttribute('aria-label',text);b.onclick=()=>message(text);wrap.append(b);chart.append(wrap);}
      previous=Math.floor(start/width)*width-width;
      for(const p of h.points){for(let t=previous+width;t<p.hour_ms&&t<end;t+=width)gap(`No observations: ${stamp(t)} – ${stamp(t+width)}`);previous=p.hour_ms;
        const c=p.counts,mean=p.latency_count?Math.round(p.latency_sum_ms/p.latency_count):null;
        const summary=`${stamp(p.hour_ms)}: mean latency ${mean==null?'N/A':mean+' ms'}, availability ${pct(c.successful,c.successful+c.failed)}, coverage ${pct(c.successful+c.failed,c.expected)}, unknown ${c.unknown}`;
        const wrap=el('div',undefined,'bucket'),b=button('',()=>message(summary));wrap.dataset.key=`bucket-${p.hour_ms}`;b.title=summary;b.setAttribute('aria-label',summary);if(c.unknown||!p.latency_count)b.className='gap';else b.style.height=`${Math.max(5,mean/max*100)}%`;wrap.append(b);chart.append(wrap);
        rows.push([stamp(p.hour_ms),mean==null?'N/A':`${mean} ms`,pct(c.successful,c.successful+c.failed),pct(c.successful+c.failed,c.expected),String(c.unknown)]);
      }
      for(let t=previous+width;t<end;t+=width)gap(`No observations: ${stamp(t)} – ${stamp(t+width)}`);
      box.append(chart);const d=el('details');d.append(el('summary','View timestamped values as a table'),table(['Bucket start','Mean latency','Sample availability','Coverage','Unknown slots'],rows,'Aggregated observations; missing buckets are unknown, never connected as successful checks.'));box.append(d);
    }catch(e){box.append(el('p',`History unavailable. ${e.message}`,'error'),button('Retry history',()=>loadHistory(current()||m,targetBox)));}
    finally { if(stopped)box.querySelectorAll('button').forEach(b=>b.disabled=true);if(targetBox.isConnected){reconcile(targetBox,box);targetBox.removeAttribute('aria-busy');}historyBusy=false; }
  }
  function table(headers,rows,caption){const wrap=el('div',undefined,'table-wrap'),t=el('table'),head=el('thead'),tr=el('tr');t.append(el('caption',caption));headers.forEach(v=>{const th=el('th',v);th.scope='col';tr.append(th);});head.append(tr);t.append(head);const body=el('tbody');for(const row of rows){const r=el('tr');for(const value of row){const td=el('td');if(value instanceof Node)td.append(value);else td.textContent=value;r.append(td);}body.append(r);}t.append(body);wrap.append(t);return wrap;}
  function incidentState(box){
    if(!incidentPages.has(box))incidentPages.set(box,{before:null,busy:false,page:null,receipts:null,updated:null,error:null});
    return incidentPages.get(box);
  }
  function renderIncidents(box,monitorId){
    const state=incidentState(box),content=el('div');
    content.append(el('h2',monitorId===null?'Incidents':'Recent incidents'));
    content.append(el('p',`Incident snapshot: ${stamp(state.updated)} · ${timeZone}`,'muted'));
    if(state.before!==null)content.append(el('p','Older page retained. Automatic incident and delivery refresh is paused for this snapshot; refresh this page or return to newest incidents.','notice warning'));
    else content.append(el('p','Newest incidents and delivery states refresh with each visible-tab poll.','muted'));
    if(state.error)content.append(el('p',`Incident snapshot is stale. ${state.error}`,'notice warning'));
    const page=state.page;
    if(page){
      const incidents=page.incidents.filter(i=>monitorId===null||String(i.monitor_id)===String(monitorId));
      const rows=incidents.map(i=>{const m=data.monitors.find(m=>String(m.id)===String(i.monitor_id));const notices=state.receipts.notifications.filter(n=>String(n.incident_id)===String(i.id));return [link(m?.config.name||'Removed monitor',`/monitors/${i.monitor_id}`),stamp(i.first_failure_at_ms),stamp(i.confirmed_at_ms),i.ended_at_ms?stamp(i.ended_at_ms):'Open',i.observation_gap?'Unknown observation gap':'No recorded gap',i.end_reason?fmt(i.end_reason):'Target failure',notices.length?notices.map(n=>`${fmt(n.kind)}: ${fmt(n.state)}${n.error_category?' ('+fmt(n.error_category)+')':''}`).join('; '):'No receipt in retained page; delivery unconfirmed'];});
      content.append(table(['Monitor','First observed','Confirmed','Ended','Coverage','Reason / closure','Alert delivery'],rows,`Times in ${timeZone}. Alert receipts are separate from incidents; no receipt does not prove delivery.`));
      if(!rows.length)content.append(el('p',monitorId===null?'No incidents on this page.':'No incidents for this monitor on this global page. Older pages may contain earlier incidents.'));
    }else if(!state.error)content.append(el('p','Loading incidents…'));
    const actions=el('div',undefined,'actions');actions.dataset.key='incident-actions';
    const refresh=button(state.error?'Retry incidents':'Refresh this incident page',()=>loadIncidents(box,monitorId,state.before));refresh.dataset.key='refresh-incidents';refresh.setAttribute('aria-disabled',String(state.busy));actions.append(refresh);
    if(state.before!==null){const b=button('Newest incidents',()=>loadIncidents(box,monitorId,null));b.dataset.key='newest-incidents';actions.append(b);}
    if(page?.incidents.length===25&&page.next_before!==null){const b=button('Older incidents',()=>loadIncidents(box,monitorId,page.next_before));b.dataset.key='older-incidents';actions.append(b);}
    content.append(actions);reconcile(box,content);
  }
  async function refreshIncidents(box,monitorId=null){
    const state=incidentState(box);
    if(state.before===null)await loadIncidents(box,monitorId,null);
    else renderIncidents(box,monitorId);
  }
  async function loadIncidents(box,monitorId=null,before=null){
    const state=incidentState(box);
    if(state.busy||stopped||document.hidden)return;
    state.busy=true;box.setAttribute('aria-busy','true');
    if(!state.page)renderIncidents(box,monitorId);
    try{
      const results=await Promise.allSettled([api(`/api/v1/incidents?limit=25${before!==null?'&before='+before:''}`),api('/api/v1/notifications?limit=100')]);
      for(const result of results)if(result.status==='rejected')throw result.reason;
      const [page,receipts]=results.map(result=>result.value);
      if(!box.isConnected||stopped)return;
      state.before=before;state.page=page;state.receipts=receipts;state.updated=Date.now();state.error=null;
    }catch(e){state.error=e.message;}
    finally{state.busy=false;if(box.isConnected){box.removeAttribute('aria-busy');renderIncidents(box,monitorId);}}
  }
  function field(label,name,type='text',value=''){const l=el('label',label),i=el('input');i.name=name;i.type=type;i.value=value;l.append(i);return l;}
  async function settings(){
    const main=$('main');main.classList.add('settings');main.replaceChildren(link('← Back to overview','/','back'),el('div','YOUR WORKSPACE','eyebrow'),el('h1','Settings'));
    const health=el('section',undefined,'card');health.id='service-health';main.append(health);renderHealth();
    const appearance=el('section',undefined,'card');appearance.append(el('h2','Appearance'),el('p','Choose Light, Dark or System in the header. Your preference is stored in this browser and also applies to sign in.'));main.append(appearance);
    const telegram=el('section',undefined,'card');telegram.append(el('h2','Telegram'),el('p','Loading Telegram settings…'));main.append(telegram);await loadTelegram(telegram);
    const password=el('section',undefined,'card');password.append(el('h2','Change password'),el('p','Changing the password signs out every session. Use at least 12 characters.'));
    const f=el('form');f.id='password-form';f.append(field('Current password','current_password','password'),field('New password','new_password','password'),field('Confirm new password','confirm','password'));f.elements.current_password.autocomplete='current-password';for(const name of ['new_password','confirm']){f.elements[name].autocomplete='new-password';f.elements[name].minLength=12;}for(const i of f.querySelectorAll('input')){i.required=true;i.maxLength=128;}
    const error=el('p',undefined,'error');error.setAttribute('role','alert');const save=el('button','Change password','primary');save.type='submit';f.append(error,save);f.addEventListener('submit',e=>{e.preventDefault();error.textContent='';if(f.elements.new_password.value!==f.elements.confirm.value){error.textContent='New passwords must match.';return;}act(save,async()=>{try{await api('/api/v1/account/password','POST',{current_password:f.elements.current_password.value,new_password:f.elements.new_password.value});stopped=true;location.assign('/login');}finally{f.reset();}},error);});password.append(f);main.append(password);
    const retention=el('section',undefined,'card');retention.append(el('h2','Retention'),el('p','Raw observations: 7 days. Hourly aggregates: 90 days. Closed incidents: 180 days; open incidents are preserved. Delivery receipts: 7 days, at most 2,000. Missing observations remain unknown.'));main.append(retention);settingsLoaded=true;
  }
  function renderHealth(){const target=$('service-health');if(!target)return;const box=el('div');box.replaceChildren(el('h2','Service health'),refreshLine(),el('p',data.scheduler_degraded?'Scheduler degraded':'Scheduler reporting'),el('p',`Stored observation faults: ${data.persistence_faults} · suppressed alerts: ${data.dropped_alerts}`),warnings());reconcile(target,box);}
  async function loadTelegram(box){try{const t=await api('/api/v1/settings/telegram'),s=t.settings;box.replaceChildren(el('h2','Telegram'),el('p',`Token ${t.token_present?'configured':'not configured — provision using the operator secret file'}. Tokens are never entered here.`),el('p',`Pending: ${s.pending} · oldest: ${stamp(s.oldest_pending_ms)} · last success: ${stamp(s.last_success_ms)} · dropped: ${s.dropped_alerts}`));
      if(s.last_error||s.configuration_failed||t.persistence_degraded)box.append(el('p',`Delivery fault: ${fmt(s.last_error)}. Configuration failed: ${s.configuration_failed}. Persistence degraded: ${t.persistence_degraded}.`,'notice warning'));
      if(s.cooldown_until_ms>data.server_time_ms)box.append(el('p',`Provider cooldown until ${stamp(s.cooldown_until_ms)}.`,'notice warning'));
      const f=el('form'),enabled=field('Enable Telegram','enabled','checkbox'),chat=field('Chat / channel ID (signed decimal)','chat_id','text',s.chat_id||'');enabled.className='check';enabled.querySelector('input').checked=s.enabled;chat.querySelector('input').inputMode='text';chat.querySelector('input').maxLength=20;chat.querySelector('input').pattern='-?[1-9][0-9]*';f.append(enabled,chat,el('p','Changing destination or enablement cancels pending jobs for the old destination. A request already submitted may still arrive.','muted'));
      const error=el('p',undefined,'error');error.setAttribute('role','alert');const save=el('button','Save Telegram settings','primary');save.type='submit';f.append(error,save);f.addEventListener('submit',e=>{e.preventDefault();error.textContent='';act(save,async()=>{await api('/api/v1/settings/telegram','PATCH',{revision:s.revision,enabled:f.elements.enabled.checked,chat_id:f.elements.chat_id.value||null});message('Telegram settings saved.');await loadTelegram(box);},error);});box.append(f);
      const actions=el('div',undefined,'actions');const test=button('Send test message',e=>act(e.currentTarget,async()=>{const queued=await api('/api/v1/settings/telegram/test','POST');message(`Test ${queued.id} queued. Queued does not mean delivered.`);await loadReceipts(receipts);},error));test.disabled=!t.token_present||!s.enabled;actions.append(test,button('Reload Telegram status',event=>act(event.currentTarget,()=>loadTelegram(box),error)));box.append(actions);const receipts=el('div');box.append(receipts);await loadReceipts(receipts);
    }catch(e){box.replaceChildren(el('h2','Telegram'),el('p',e.message,'error'),button('Retry Telegram settings',()=>loadTelegram(box)));}}
  async function loadReceipts(box,before=null){try{const page=await api(`/api/v1/notifications?limit=25${before!==null?'&before='+before:''}`);if(!box.isConnected)return;box.replaceChildren(table(['Created','Kind','State','Attempts','Next retry','Error'],page.notifications.map(n=>[stamp(n.created_at_ms),fmt(n.kind),fmt(n.state),String(n.attempts),n.state==='pending'?stamp(n.next_attempt_at_ms):'—',fmt(n.error_category)]),'Recent delivery receipts; a crash after submission can cause duplicate delivery.'));if(before!==null)box.append(button('Newest receipts',()=>loadReceipts(box)));if(page.notifications.length===25)box.append(button('Older receipts',()=>loadReceipts(box,page.notifications.at(-1).id)));}catch(e){box.replaceChildren(el('p',`Receipts unavailable. ${e.message}`,'error'),button('Retry receipts',()=>loadReceipts(box,before)));}}
  function configureFields(){const f=$('monitor-form'),http=f.elements.kind.value==='http',tcp=f.elements.kind.value==='tcp';$('http-field').hidden=!http;f.elements.accepted_statuses.disabled=!http;$('port-field').hidden=!tcp;f.elements.port.required=tcp;f.elements.port.disabled=!tcp;$('target-label').textContent=http?'URL (HTTP / HTTPS)':'Host or IP address';f.elements.target.placeholder=http?'https://service.example':'service.example';f.elements.target.maxLength=http?2048:253;}
  function openEditor(m=null){editing=m;createKey=crypto.randomUUID();const f=$('monitor-form');f.reset();$('form-error').textContent='';$('editor-title').textContent=m?'Edit monitor':'Add monitor';if(m){for(const [key,value] of Object.entries(m.config)){const input=f.elements[key];if(!input)continue;if(input.type==='checkbox')input.checked=value;else if(key==='accepted_statuses')input.value=value.map(([a,b])=>a===b?a:`${a}-${b}`).join(', ');else input.value=value??'';}}configureFields();$('editor').showModal();f.elements.name.focus();}
  function openDelete(m){deleting=m;$('delete-name').textContent=`Delete “${m.config.name}”?`;$('delete-error').textContent='';$('delete-dialog').showModal();$('cancel-delete').focus();}
  $('monitor-form').elements.kind.addEventListener('change',configureFields);
  $('monitor-form').addEventListener('input',()=>{createKey=crypto.randomUUID();});
  $('monitor-form').addEventListener('submit',event=>{event.preventDefault();const f=event.currentTarget;const get=name=>f.elements[name].value;const config={name:get('name'),kind:get('kind'),target:get('target'),port:get('kind')==='tcp'?Number(get('port')):null,interval_seconds:Number(get('interval_seconds')),deadline_seconds:Number(get('deadline_seconds')),failure_threshold:Number(get('failure_threshold')),enabled:f.elements.enabled.checked,alerts:f.elements.alerts.checked,address_family:get('address_family'),accepted_statuses:[[200,299]]};
    $('form-error').textContent='';if(config.deadline_seconds>=config.interval_seconds){$('form-error').textContent='Deadline must be shorter than the normal interval.';f.elements.deadline_seconds.focus();return;}
    if(config.kind==='http'){const parts=get('accepted_statuses').split(',').map(s=>s.trim());if(parts.length>32||parts.some(s=>!/^\d{3}(?:\s*-\s*\d{3})?$/.test(s))){$('form-error').textContent='Enter at most 32 HTTP codes or ranges, e.g. 200-299, 301.';return;}config.accepted_statuses=parts.map(s=>{const [a,b]=s.split('-').map(Number);return [a,b??a];});if(config.accepted_statuses.some(([a,b])=>a<100||b>599||a>b)){$('form-error').textContent='HTTP status ranges must be ordered within 100–599.';return;}}
    act(f.querySelector('[type=submit]'),async()=>{const result=await api(editing?`/api/v1/monitors/${editing.id}`:'/api/v1/monitors',editing?'PATCH':'POST',editing?{revision:editing.revision,config}:config,editing?undefined:createKey);$('editor').close();message('Monitor saved.');if(!editing)location.assign(`/monitors/${result.id}`);else await refresh(true);},$('form-error'));
  });
  document.querySelectorAll('[data-close]').forEach(b=>b.addEventListener('click',()=>$('editor').close()));
  for (const dialog of document.querySelectorAll('dialog')) dialog.addEventListener('keydown',event=>{
    if(event.key!=='Tab')return;
    const items=[...dialog.querySelectorAll('button,input,select,a[href],[tabindex="0"]')].filter(n=>!n.disabled&&n.getClientRects().length);
    const first=items[0],last=items.at(-1);
    if(event.shiftKey&&document.activeElement===first){event.preventDefault();last.focus();}
    else if(!event.shiftKey&&document.activeElement===last){event.preventDefault();first.focus();}
  });
  $('cancel-delete').addEventListener('click',()=>$('delete-dialog').close());
  $('confirm-delete').addEventListener('click',e=>act(e.currentTarget,async()=>{await api(`/api/v1/monitors/${deleting.id}`,'DELETE',{revision:deleting.revision,confirm_history_deletion:true});location.assign('/');},$('delete-error')));
  $('add').addEventListener('click',()=>openEditor());
  $('logout').addEventListener('click',e=>act(e.currentTarget,async()=>{await api('/auth/logout','POST');stopped=true;location.assign('/login');}));
  for(const id of ['search','filter'])$(id).addEventListener('input',()=>{if(!data)return;renderList();if(view==='overview')overview();});
  async function refresh(force=false){
    if(busy||stopped||document.hidden)return;busy=true;clearTimeout(timer);
    try{if(!csrf)csrf=(await api('/api/v1/account/session')).csrf_token;const next=await api('/api/v1/summary');const changes=data?next.monitors.filter(m=>data.monitors.find(old=>String(old.id)===String(m.id))?.state.status!==m.state.status):[];data=next;lastRefresh=Date.now();failed=false;$('connection').hidden=true;if(changes.length)announce(changes.map(m=>`${m.config.name}: ${labels[m.state.status]}`).join('. '));renderList();
      if(view==='overview')overview();
      else if(view==='detail')await detail();
      else if(view==='settings'){if(!settingsLoaded)await settings();else renderHealth();}
      else {
        if(!$('incident-page')){$('main').replaceChildren(link('← Back to overview','/','back'),el('h1','Incident history'));const box=el('section',undefined,'card');box.id='incident-page';$('main').append(box);}
        await refreshIncidents($('incident-page'));
      }
    }catch(e){failed=true;if(!stopped){$('connection').hidden=false;$('connection').replaceChildren(el('strong',data?'STALE DATA — ':'Unable to load data — '),el('span',`${e.message} Last successful refresh: ${stamp(lastRefresh)}. `),button('Retry refresh',()=>refresh(true)));if(!data)$('main').replaceChildren(el('h1','Monitoring unavailable'),el('p','No monitoring values are shown until data is available.'));}}
    finally{busy=false;if(!stopped&&!document.hidden)timer=setTimeout(()=>refresh(),(failed?60000:15000)+Math.random()*750);}
  }
  document.addEventListener('visibilitychange',()=>{clearTimeout(timer);if(!document.hidden)refresh();});
  refresh(true);
})();
