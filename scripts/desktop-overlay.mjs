// Actual Win32 target, WGC, PP-OCR and Hy-MT2; no synthetic inference events.
import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import {createServer} from 'node:net';
import {createHash} from 'node:crypto';
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
const out=path.resolve(process.env.GAMEOCR_OVERLAY_OUTPUT??`artifacts/validation/overlay-${Date.now()}`);fs.mkdirSync(out,{recursive:true});
const data=path.join(out,'data'),control=path.join(out,'control.txt'),log=fs.openSync(path.join(out,'desktop.log'),'w');
const reportPath=path.resolve(process.env.GAMEOCR_OVERLAY_REPORT??path.join(out,'report.json'));
const binary=path.resolve(process.env.GAMEOCR_APP??'target/debug/gameocr.exe');
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function until(fn,timeout=60000){let last;const end=Date.now()+timeout;while(Date.now()<end){try{const result=await fn();if(result)return result;}catch(e){last=e;}await delay(80);}throw Error('Timed out: '+(last?.message??''));}
const checks=[],samples=[],errors=[];let app,fixture,browser,page,overlay,invoke,vite;
const pass=(name,details={})=>{checks.push({name,...details});console.log('PASS',name);};
const native=(action,owner,hwnd='0',extra=[])=>JSON.parse(execFileSync('pwsh.exe',['-NoProfile','-File','scripts/overlay-native.ps1','-Action',action,'-OwnerPid',String(owner),'-Hwnd',hwnd,...extra],{encoding:'utf8',windowsHide:true}));
async function fixtureCommand(action){fs.writeFileSync(control+'.tmp',action);fs.renameSync(control+'.tmp',control);await until(()=>!fs.existsSync(control));await delay(180);}
async function recognize(){await invoke('recognize_once');return (await invoke('get_history'))[0];}
async function currentVisible(){return until(async()=>{const s=await invoke('overlay_snapshot');return s.visible&&s.items.length?s:null;});}
async function hidden(){return until(async()=>{const s=await invoke('overlay_snapshot');return !s.visible&&s.items.length===0?s:null;});}
async function front(target){native('focus',fixture.pid,target.hwnd);await delay(150);}
try{
  if(!process.env.GAMEOCR_APP){vite=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1420','--strictPort'],{windowsHide:true,stdio:['ignore',log,log]});await until(async()=>{try{return (await fetch('http://127.0.0.1:1420')).ok;}catch{return false;}});}
  const port=await new Promise(resolve=>{const s=createServer();s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolve(p));});});
  app=spawn(binary,[],{windowsHide:true,stdio:['ignore',log,log],env:{...process.env,GAMEOCR_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:path.join(data,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:`--remote-debugging-port=${port}`}});
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));
  page=await until(()=>browser.contexts()[0]?.pages().find(p=>/1420|tauri.localhost/.test(p.url())&&!p.url().includes('overlay')));await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  page.on('pageerror',e=>errors.push(e.message));invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  assert.equal((await invoke('overlay_settings')).overlayEnabled,false);
  let settings={...await invoke('get_settings'),modelDir:path.resolve('.cache/test-models'),device:'cpu',changeDetection:false,ocrIntervalMs:300};settings.onnx={...settings.onnx,detectionRegion:[0,.10,1,.90]};
  await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});
  const model=(await invoke('translation_models')).find(m=>m.id==='hy-mt2-1.8b-q4-k-m');
  await invoke('import_translation_model',{path:path.resolve('.cache/translation/Hy-MT2-1.8B-Q4_K_M.gguf')});
  let translation={...await invoke('translation_settings'),enabled:true,device:'cpu',grouping:'block',debounceMs:0};await invoke('update_translation_settings',{settings:translation});await invoke('load_translation_model',{id:model.id});
  fixture=spawn(path.resolve('target/debug/capture-fixture.exe'),[control],{windowsHide:true,stdio:['ignore',log,log]});
  const target=await until(async()=>(await invoke('list_windows',{showAll:false})).find(w=>w.pid===fixture.pid));
  await invoke('select_window',{id:target.id});await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  await fixtureCommand('text:New Game\nLoad Game\nSettings');await front(target);await recognize();
  await until(async()=>(await invoke('translation_history'))[0]?.results.every(r=>r.status==='complete'));
  const baseline=path.join(out,'wgc-without-overlay.png');await invoke('save_screenshot',{path:baseline,recognized:false});
  const idleBeforeStart=native('stats',app.pid);await delay(5000);const memoryBefore=native('stats',app.pid);
  let style={...await invoke('overlay_settings'),overlayEnabled:true,overlayStaleMs:30000};await invoke('update_overlay_settings',{settings:style});
  overlay=await until(()=>browser.contexts()[0]?.pages().find(p=>p.url().includes('overlay')));overlay.on('pageerror',e=>errors.push(e.message));
  const initialFrame=await recognize();await until(async()=>(await invoke('translation_history'))[0]?.frameId===initialFrame.result.frameId);
  fs.writeFileSync(path.join(out,'initial-diagnostics.json'),JSON.stringify({ocr:initialFrame.result,translation:(await invoke('translation_history'))[0],overlay:await invoke('overlay_snapshot'),windows:native('inspect',app.pid),target:native('inspect',fixture.pid)},null,2));
  const initial=await currentVisible();await until(async()=>(await overlay.locator('.subtitle').count())===initial.items.length);
  await delay(250);const windows=native('inspect',app.pid),nativeOverlay=windows.windows.find(w=>w.title==='GameOCR 翻译字幕');
  assert(nativeOverlay?.visible);assert.equal(windows.foreground,target.hwnd);assert.equal(nativeOverlay.affinity,0x11);assert(nativeOverlay.style&0x20);assert(nativeOverlay.style&0x80000);assert(nativeOverlay.style&0x8000000);
  assert(initial.items.some(item=>/[\u3400-\u9fff]/u.test(item.text)));assert.equal(initial.geometry.mapping,'dwm-frame');
  samples.push({case:'initial',snapshot:initial,native:nativeOverlay});pass('Transparent topmost nonactivating native overlay displays real Chinese translation');
  await overlay.screenshot({path:path.join(out,'overlay-subtitles.png'),omitBackground:true});
  const enabled=path.join(out,'wgc-with-overlay.png');await delay(250);await invoke('save_screenshot',{path:enabled,recognized:false});
  assert(fs.readFileSync(baseline).equals(fs.readFileSync(enabled)),'WGC source changed with the overlay visible');
  pass('Real WGC PNG is byte-identical with overlay hidden/visible (capture feedback excluded)',{sha256:createHash('sha256').update(fs.readFileSync(enabled)).digest('hex')});
  const panel=await overlay.locator('.subtitle').first().boundingBox();assert(panel);
  native('click',fixture.pid,target.hwnd,['-X',String(Math.round(initial.geometry.rect[0]+(panel.x+panel.width/2)*await overlay.evaluate(()=>devicePixelRatio))),'-Y',String(Math.round(initial.geometry.rect[1]+(panel.y+panel.height/2)*await overlay.evaluate(()=>devicePixelRatio)))]);
  const counts=await until(()=>{try{return JSON.parse(fs.readFileSync(control.replace('.txt','.input.json'),'utf8'));}catch{return null;}});assert(counts[0]>0&&counts[1]>0&&counts[2]>0);pass('Actual desktop mouse down/up and wheel pass through subtitle to Win32 target',{counts});
  await overlay.evaluate(async()=>{
    window.__overlayNodes=[...document.querySelectorAll('.subtitle')];window.__overlayMutations=0;window.__overlayLatencies=[];
    window.__observer=new MutationObserver(records=>window.__overlayMutations+=records.filter(r=>r.type==='childList'&&r.target===document.querySelector('main')).length);window.__observer.observe(document.querySelector('main'),{childList:true,subtree:true});
    const handler=window.__TAURI_INTERNALS__.transformCallback(event=>{const start=performance.now(),serial=event.payload.serial;requestAnimationFrame(()=>{if(Number(document.querySelector('main')?.dataset.serial)>=serial){window.__overlayLatencies.push(performance.now()-start);if(window.__overlayLatencies.length>256)window.__overlayLatencies.shift();}});});
    await window.__TAURI_INTERNALS__.invoke('plugin:event|listen',{event:'overlay-state',target:{kind:'Any'},handler});
  });
  for(let i=0;i<3;i++)await recognize();await currentVisible();assert(await overlay.evaluate(()=>window.__overlayNodes.every((node,i)=>node===document.querySelectorAll('.subtitle')[i])));pass('Repeated OCR keeps subtitle DOM nodes; no clear/recreate flicker');
  await fixtureCommand('offset');await recognize();const shifted=await currentVisible();assert(shifted.items[0].bbox[0]>initial.items[0].bbox[0]+150);pass('Identical text with moved OCR boxes relocates captions');
  await fixtureCommand('move');const moved=await until(async()=>{const s=await invoke('overlay_snapshot');return s.visible&&s.geometry.rect[0]>initial.geometry.rect[0]+100?s:null;});pass('Overlay follows target window movement',{rect:moved.geometry.rect});
  await fixtureCommand('resize');await hidden();await recognize();const resized=await currentVisible();assert(resized.sourceWidth>initial.sourceWidth);samples.push({case:'resized',snapshot:resized});pass('Resize hides mismatched geometry until new WGC/OCR dimensions arrive');
  const main=native('inspect',app.pid).windows.find(w=>w.title==='GameOCR');assert(main);native('background',app.pid,main.hwnd);await hidden();assert(!native('inspect',app.pid).windows.find(w=>w.hwnd===nativeOverlay.hwnd).visible);await front(target);await currentVisible();pass('Foreground changes hide native window and valid current subtitles resume');
  translation={...translation,debounceMs:2000};await invoke('update_translation_settings',{settings:translation});await fixtureCommand('text:Where are you going?');await recognize();await hidden();await fixtureCommand('text:Quest complete!');await recognize();await hidden();const replacement=await currentVisible();assert(!replacement.items.some(i=>initial.items.some(old=>old.text===i.text)));pass('New text immediately clears old subtitles; superseded translation never reappears');
  await invoke('pause_capture');await hidden();await invoke('resume_capture');await front(target);await recognize();await currentVisible();await invoke('stop_ocr');await hidden();pass('Pause and OCR stop clear live subtitles');
  await recognize();await currentVisible();await invoke('stop_capture');await hidden();await invoke('start_capture');await until(()=>invoke('get_capture_preview'));await front(target);await recognize();await currentVisible();pass('Capture stop clears; restarted generation uses fresh translation');
  const callsBefore=await invoke('translation_status'),modelBefore=await invoke('get_model_status');style={...style,overlayFontSize:22,overlayOpacity:.65};await invoke('update_overlay_settings',{settings:style});await currentVisible();assert.equal((await invoke('translation_status')).calls,callsBefore.calls);assert.deepEqual(await invoke('get_model_status'),modelBefore);pass('Style persistence does not reload models or invoke translation');
  const idleStart=native('stats',app.pid);const mutationsBefore=await overlay.evaluate(()=>window.__overlayMutations);await delay(5000);const idleEnd=native('stats',app.pid),mutationsAfter=await overlay.evaluate(()=>window.__overlayMutations);
  const {layoutSubtitles}=await import('../src/lib/overlayGeometry.ts');const geometryStart=performance.now();for(let i=0;i<10000;i++)layoutSubtitles(initial.items,initial.sourceWidth,initial.sourceHeight,initial.sourceWidth,initial.sourceHeight,style);const mappingMs=(performance.now()-geometryStart)/10000;
  samples.push({case:'idle',idleBeforeStart,memoryBefore,idleStart,idleEnd,baselineCpuOneCorePercent:(memoryBefore.cpuSeconds-idleBeforeStart.cpuSeconds)/((memoryBefore.timestamp-idleBeforeStart.timestamp)/1000)*100,idleCpuOneCorePercent:(idleEnd.cpuSeconds-idleStart.cpuSeconds)/((idleEnd.timestamp-idleStart.timestamp)/1000)*100,extraWorkingSetBytes:idleEnd.workingSetBytes-memoryBefore.workingSetBytes,subtitleRootMutations:mutationsAfter-mutationsBefore,mappingMsPerLayout:mappingMs,eventToAnimationFrameMs:await overlay.evaluate(()=>window.__overlayLatencies)});assert.equal(mutationsAfter,mutationsBefore);pass('Idle subtitles do not rebuild DOM; process CPU/memory sampled');
  style={...style,overlayEnabled:false};await invoke('update_overlay_settings',{settings:style});await hidden();native('click',fixture.pid,target.hwnd,['-X','400','-Y','300']);pass('Disabling overlay hides window and target still receives input');
  style={...style,overlayEnabled:true};await invoke('update_overlay_settings',{settings:style});await recognize();await currentVisible();assert.equal(native('inspect',app.pid).windows.filter(w=>w.title==='GameOCR 翻译字幕').length,1);pass('Repeated enable/disable reuses a single overlay window');
  await fixtureCommand('minimize');await hidden();pass('Minimized target immediately hides subtitles');
  await fixtureCommand('restore');await front(target);if((await invoke('get_status')).capture!=='running')await invoke('start_capture');await recognize();await currentVisible();await fixtureCommand('close');await hidden();pass('Closed target cannot leave a visible subtitle window');
  const owner=app.pid;await invoke('plugin:window|close',{label:'main'});await until(()=>app.exitCode!==null,15000);assert.equal(native('inspect',owner).windows.length,0);pass('Closing main shuts down overlay and all owned top-level windows');
  assert.equal(JSON.parse(fs.readFileSync(path.join(data,'overlay/settings.json'),'utf8')).overlayFontSize,22);assert.deepEqual(errors,[]);
  fs.writeFileSync(reportPath,JSON.stringify({passed:true,app:binary,appSha256:createHash('sha256').update(fs.readFileSync(binary)).digest('hex'),checks,samples,errors,limits:['Controlled Win32 test window, not a real game','Single monitor / current desktop DPI only','Visual simulated frost, no cross-window blur','No exclusive fullscreen or ADB overlay']},null,2));
  console.log('Overlay report:',reportPath);
}catch(e){fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({error:String(e.stack),checks,samples,errors,overlay:await invoke?.('overlay_snapshot').catch(()=>null),translation:await invoke?.('translation_history').catch(()=>null)},null,2));throw e;}
finally{try{await invoke?.('plugin:window|close',{label:'main'});}catch{}await browser?.close().catch(()=>{});fixture?.kill();if(app?.exitCode===null)app.kill();vite?.kill();fs.closeSync(log);}
