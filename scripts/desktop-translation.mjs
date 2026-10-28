// Real WGC -> PP-OCR -> Hy-MT2 -> Tauri UI. No generated timing / OCR mocks.
import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import {createServer} from 'node:net';
import {createHash} from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=process.cwd(),out=path.resolve('artifacts/validation/translation-'+Date.now());fs.mkdirSync(out,{recursive:true});
const sourceBinary=path.resolve(process.env.GAMEOCR_APP??'artifacts/GameOCR-windows-x64-ondemand-portable/GameOCR.exe');
const appRoot=path.join(out,'app');fs.cpSync(path.dirname(sourceBinary),appRoot,{recursive:true});
const binary=path.join(appRoot,'GameOCR.exe');
const data=path.join(out,'data'),control=path.join(out,'control.txt');
// Reuse only archives and installed receipts verified by previous real official
// downloads. RuntimeManager re-verifies every file before the GPU worker starts.
const seed=path.resolve(process.env.GAMEOCR_GPU_CACHE??'.cache/cuda-download-validation/data/runtimes');
if(fs.existsSync(seed))fs.cpSync(seed,path.join(data,'runtimes'),{recursive:true});
const assets=JSON.parse(fs.readFileSync('artifacts/gpu-backends.json','utf8')).packages;
fs.mkdirSync(path.join(appRoot,'runtime-downloads'),{recursive:true});
for(const p of assets){const asset=path.basename(new URL(p.url).pathname);fs.copyFileSync(path.resolve('artifacts',asset),path.join(appRoot,'runtime-downloads',asset));}
const checks=[],samples=[],skipped=[],failures=[];
const log=fs.openSync(path.join(out,'desktop.log'),'w');
const pause=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn,timeout=60000){let last;const end=Date.now()+timeout;while(Date.now()<end){try{const v=await fn();if(v)return v;}catch(e){last=e;}await pause(100);}throw Error('Timed out: '+(last?.message??''));}
const pass=(name,details={})=>{checks.push({name,...details});console.log('PASS',name,JSON.stringify(details));};
let app,fixture,browser,page,invoke,settings,tsettings;
const modelId='hy-mt2-1.8b-q4-k-m';
async function launch(){
  const port=await new Promise(resolve=>{const s=createServer();s.listen(0,'127.0.0.1',()=>{const port=s.address().port;s.close(()=>resolve(port));});});
  const env={...process.env,NO_PROXY:'localhost,127.0.0.1',GAMEOCR_DATA_DIR:data,WEBVIEW2_USER_DATA_FOLDER:path.join(data,'webview'),WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--disable-gpu --remote-debugging-port='+port};
  for(const key of Object.keys(env))if(key.toLowerCase()==='path'||key.startsWith('CUDA_PATH')||key==='ORT_DYLIB_PATH')delete env[key];
  env.Path=`${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`;
  app=spawn(binary,[],{windowsHide:true,stdio:['ignore',log,log],env});
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));
  page=await until(()=>browser.contexts()[0]?.pages().find(p=>/tauri.localhost|tauri:\/\//.test(p.url())));
  page.on('pageerror',e=>failures.push(e.message));await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await page.evaluate(async()=>{
    window.__translationEvents=[];
    for(const event of ['translation-invalidated','translation-result','cuda-runtime-progress']) {
      const handler=window.__TAURI_INTERNALS__.transformCallback(value=>window.__translationEvents.push({event,payload:value.payload}));
      await window.__TAURI_INTERNALS__.invoke('plugin:event|listen',{event,target:{kind:'Any'},handler});
    }
  });
  if(!settings)assert.equal(path.resolve((await invoke('get_settings')).modelDir),path.join(data,'models'));
}
async function close(){if(invoke)await invoke('plugin:window|close',{label:'main'}).catch(()=>{});await until(()=>app.exitCode!==null,10000);await browser?.close();browser=null;invoke=null;}
async function show(text){fs.writeFileSync(control+'.tmp','text:'+text);fs.renameSync(control+'.tmp',control);await until(()=>!fs.existsSync(control));await pause(180);}
async function recognize(){await invoke('recognize_once');return (await invoke('get_history'))[0];}
async function translated(snapshot,timeout=60000){return until(async()=>{const event=(await invoke('translation_history')).find(t=>t.frameId===snapshot.result.frameId&&t.generation===snapshot.preview.generation);if(event?.results.some(r=>r.status==='error'))throw Error(JSON.stringify(event));return event;},timeout);}
async function updateT(patch){tsettings={...tsettings,...patch};await invoke('update_translation_settings',{settings:tsettings});tsettings=await invoke('translation_settings');}
async function target(){const w=await until(async()=>(await invoke('list_windows',{showAll:false})).find(w=>w.title==='GameOCR Capture Fixture'));await invoke('select_window',{id:w.id});await invoke('start_capture');await until(()=>invoke('get_capture_preview'));return w;}
function ownedWorkers(){return JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',`ConvertTo-Json -Compress -InputObject @(Get-CimInstance Win32_Process -Filter "ParentProcessId=${app.pid} AND Name='translation-worker.exe'" | Select-Object ProcessId,ExecutablePath)`],{encoding:'utf8',windowsHide:true}));}
try {
  await launch();tsettings=await invoke('translation_settings');assert.equal(tsettings.enabled,false);assert.equal((await invoke('translation_status')).modelId,null);
  fixture=spawn(path.resolve('target/debug/capture-fixture.exe'),[control],{windowsHide:true,stdio:['ignore',log,log]});
  settings={...await invoke('get_settings'),modelDir:path.resolve('.cache/test-models'),device:'cpu',executionMode:'single',enableParallelInference:false,changeDetection:false,ocrIntervalMs:200,downloadMirror:'https://huggingface.co'};
  // Exercise the actual game-content ROI, excluding the fixture's Windows title bar/buttons.
  settings.onnx={...settings.onnx,detectionRegion:[0,0.12,1,0.88]};
  await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});const window=await target();
  await show('Where are you going?');const raw=await recognize();assert(raw.result.blocks.some(b=>/Where/.test(b.text)));assert.equal((await invoke('translation_history')).length,0);pass('Pure OCR works without installed translation model');
  if(process.env.GAMEOCR_TEST_DOWNLOAD==='1') {
    let outcome;const attempt=invoke('download_translation_model').then(()=>outcome={ok:true},error=>outcome={error:String(error)});
    const model=(await invoke('translation_models')).find(m=>m.id===modelId),part=path.join(data,'translation/models',model.file+'.part');
    await until(()=>{if(outcome?.error)throw Error(outcome.error);return fs.existsSync(part)&&fs.statSync(part).size>0;},120000);await invoke('cancel_translation_download');const cancelled=await attempt;assert(cancelled.error);assert(fs.statSync(part).size>0);
    console.log('Download cancellation preserved',fs.statSync(part).size,'bytes');
    // Seed a verified prefix to keep this repeatable integration test small;
    // the final tail is received with a real HTTP Range request and full SHA check.
    const source=fs.openSync(path.resolve('.cache/translation/Hy-MT2-1.8B-Q4_K_M.gguf'),'r'),target=fs.openSync(part,'w');
    const buffer=Buffer.alloc(1024*1024),prefix=model.size-1024*1024;
    try {for(let offset=0;offset<prefix;){const n=fs.readSync(source,buffer,0,Math.min(buffer.length,prefix-offset),offset);assert(n>0);fs.writeSync(target,buffer,0,n);offset+=n;}}finally{fs.closeSync(source);fs.closeSync(target);}
    console.log('Resuming final 1 MiB through official endpoint');
    await invoke('download_translation_model');assert((await invoke('translation_models')).find(m=>m.id===modelId).installed);assert(!fs.existsSync(part));pass('Real official model download cancellation, Range resume and full SHA validation');
    await invoke('delete_translation_model',{id:modelId});
  }
  await invoke('import_translation_model',{path:path.resolve('.cache/translation/Hy-MT2-1.8B-Q4_K_M.gguf')});
  await updateT({enabled:true,device:'cpu',grouping:'paragraph',resourcePolicy:'manual',debounceMs:80,sourceLanguage:'en'});await invoke('load_translation_model',{id:modelId});
  const first=await recognize(),translatedFirst=await translated(first);assert(translatedFirst.results.some(r=>/[\u3400-\u9fff]/u.test(r.translatedText)));assert(translatedFirst.results.every(r=>r.sourceBlocks.length));
  samples.push({combination:'OCR CPU + translation CPU',ocr:first.result.metrics,translation:translatedFirst});
  assert.equal((await invoke('get_model_status')).status,'ready');assert.equal((await invoke('get_history'))[0].result.rawOutput,first.result.rawOutput);
  await page.getByLabel('翻译结果',{exact:true}).waitFor();await page.screenshot({path:path.join(out,'cpu-translation.png')});pass('WGC OCR CPU -> real translation CPU -> visible Chinese UI; OCR result unchanged');
  const pid=ownedWorkers()[0].ProcessId,calls=(await invoke('translation_status')).calls;
  const next=await recognize(),cached=await translated(next);assert(cached.results.every(r=>r.cacheHit));assert.equal((await invoke('translation_status')).calls,calls);assert.equal(ownedWorkers()[0].ProcessId,pid);assert.equal(cached.frameId,next.result.frameId);pass('Repeated OCR uses cache with current identity and same worker');
  samples.push({case:'repeated OCR cache',translation:cached,additionalModelCalls:0});
  const normalGeneration={...tsettings.generation};
  await updateT({generation:{...normalGeneration,maxTokens:1}});await invoke('load_translation_model',{id:modelId});
  const limitedFrame=await recognize(),limited=await translated(limitedFrame);
  assert(limited.results.every(r=>r.truncated&&r.stopReason==='maxTokens'&&r.status==='incomplete'));
  const limitedCalls=(await invoke('translation_status')).calls;
  const retryLimited=await recognize(),retryLimitedResult=await translated(retryLimited);
  assert(retryLimitedResult.results.every(r=>!r.cacheHit&&r.truncated));assert.equal((await invoke('translation_status')).calls,limitedCalls+1);
  pass('Truncated real model output remains incomplete and never hits cache');
  await updateT({generation:normalGeneration});await invoke('load_translation_model',{id:modelId});
  const exportPath=path.join(out,'translation-export.json');await invoke('export_translation',{frameId:next.result.frameId,path:exportPath});assert.equal(JSON.parse(fs.readFileSync(exportPath,'utf8')).schemaVersion,1);
  await page.getByRole('button',{name:'翻译',exact:true}).click();const dialog=page.getByRole('dialog');await dialog.waitFor();
  const box=await dialog.boundingBox(),viewport=await page.evaluate(()=>({w:innerWidth,h:innerHeight}));assert(Math.abs(box.x+box.width/2-viewport.w/2)<3);assert(Math.abs(box.y+box.height/2-viewport.h/2)<3);await page.screenshot({path:path.join(out,'translation-settings.png')});
  await page.getByRole('button',{name:'关闭翻译设置'}).click();pass('Separate JSON export and centered translation settings');
  await updateT({grouping:'block',batchTranslation:true});await invoke('clear_translation_cache');await show('New Game\nLoad Game\nSettings');
  const batchBefore=await invoke('translation_status'),batchFrame=await recognize(),batchResult=await translated(batchFrame),batchAfter=await invoke('translation_status');
  assert(batchResult.results.length>=2);assert.equal(batchAfter.batchCalls-batchBefore.batchCalls,1);
  assert(batchResult.results.every(r=>r.sourceBlockIds.length===1&&r.translatedText));
  assert(batchResult.results.every(r=>r.metrics.batchSize===batchResult.results.length)||batchAfter.batchFallbacks>batchBefore.batchFallbacks);
  samples.push({case:'structured batch',translation:batchResult,modelCalls:batchAfter.calls-batchBefore.calls,fallbacks:batchAfter.batchFallbacks-batchBefore.batchFallbacks});
  pass('Real multi-block translation uses validated IDs or safe per-group fallback');
  fs.writeFileSync(control,'resize');await until(()=>!fs.existsSync(control));await pause(250);
  await invoke('unload_model');settings={...settings,onnx:{...settings.onnx,detectionRegion:[0,0.08,1,0.92]}};await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});
  await invoke('clear_translation_cache');await show('New Game\nLoad Game\nSettings');await translated(await recognize());
  await show('New Game\nLoad Game\nSettings\nInventory\nContinue\nExit Game');
  const partialBefore=await invoke('translation_status'),partialFrame=await recognize(),partial=await translated(partialFrame),partialAfter=await invoke('translation_status');
  assert.equal(partial.results.length,6);assert.equal(partial.results.filter(r=>r.cacheHit).length,3);
  assert.equal(partialAfter.batchCalls-partialBefore.batchCalls,1);assert.equal(partialAfter.calls-partialBefore.calls,1);
  assert.equal(partialAfter.cacheHits-partialBefore.cacheHits,3);
  assert.equal(partialAfter.batchFallbacks,partialBefore.batchFallbacks);
  assert(partial.results.filter(r=>!r.cacheHit).every(r=>r.metrics.batchSize===3));
  assert.equal(partial.results.filter(r=>r.metrics.generatedTokens>0).length,1);
  samples.push({case:'three cached plus three new',translation:partial,modelCalls:1});pass('Three cache hits plus three new menus use one model call for only the three misses');
  await updateT({grouping:'paragraph'});
  await updateT({debounceMs:2000});await invoke('clear_translation_cache');const before=(await invoke('translation_status')).calls;
  const started=performance.now();for(const line of ['Where','Where are','Where are you','Where are you going?']){await show(line);await recognize();}
  const final=(await invoke('get_history'))[0];await translated(final);const after=(await invoke('translation_status')).calls;assert.equal(after-before,1);samples.push({case:'typewriter',elapsedMs:performance.now()-started,modelCalls:after-before});pass('Four real OCR typewriter updates produce one final model call');
  await show('The northern bridge is still closed.');const cancelled=await recognize();await invoke('pause_capture');await pause(2200);assert(!(await invoke('translation_history')).some(t=>t.frameId===cancelled.result.frameId));pass('Pause invalidates pending translation');await invoke('resume_capture');
  await updateT({debounceMs:0});await invoke('clear_translation_cache');await show('Before you leave for the fortress,\nspeak to Captain Rowan at the gate.\nBring the sealed letter and return\nbefore the bridge closes at sunset.');const obsolete=await recognize();await until(async()=>(await invoke('translation_status')).status==='translating');
  await page.evaluate(()=>window.__translationEvents=[]);
  await invoke('select_window',{id:window.id});await pause(1500);assert.equal((await invoke('get_history')).length,0);
  await until(async()=>!['translating','discarding'].includes((await invoke('translation_status')).status));
  const transitionEvents=await page.evaluate(()=>window.__translationEvents),invalidated=transitionEvents.findIndex(e=>e.event==='translation-invalidated');
  assert(invalidated>=0);assert(!transitionEvents.slice(invalidated+1).some(e=>e.event==='translation-result'&&e.payload.frameId===obsolete.result.frameId&&e.payload.generation===obsolete.preview.generation));
  samples.push({case:'target invalidation event order',events:transitionEvents});
  pass('Target change discards running translation');await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  await until(async()=>!['translating','discarding'].includes((await invoke('translation_status')).status));
  await updateT({style:'保留角色语气。'});const workers=ownedWorkers();assert.equal(workers.length,1);execFileSync('powershell.exe',['-NoProfile','-Command',`Stop-Process -Id ${workers[0].ProcessId} -Force`],{windowsHide:true});
  const failed=await recognize();await until(async()=>(await invoke('translation_status')).status==='error');await invoke('translate_once',{frameId:failed.result.frameId});await until(async()=>(await invoke('translation_history')).some(t=>t.frameId===failed.result.frameId&&t.results.every(r=>r.status==='complete')));pass('Translation worker crash is isolated and next request recovers');
  await updateT({device:'cuda',resourcePolicy:'manual',gpuReserveMb:128,generation:{...tsettings.generation,contextSize:1024,maxTokens:128},style:''});
  let gpuReady=true;
  try {await invoke('load_translation_model',{id:modelId});} catch(error) {
    const status=await invoke('translation_status');
    if(!String(error).includes('显存不足'))throw error;
    gpuReady=false;skipped.push({name:'Desktop CPU/GPU and GPU/GPU combinations',reason:String(error),status});
    console.log('SKIP GPU combinations: insufficient available VRAM',status.freeVramMb);
    assert.equal(status.device,null);pass('Explicit CUDA admission reports insufficient resources without silently using CPU');
    await updateT({device:'auto'});await invoke('load_translation_model',{id:modelId});assert.equal((await invoke('translation_status')).device,'cpu');
    const fallbackFrame=await recognize();assert((await translated(fallbackFrame)).results.every(r=>r.device==='cpu'));pass('Auto falls back to real CPU inference under insufficient GPU memory');
  }
  if(gpuReady) {
  assert.equal((await invoke('translation_status')).device,'cuda');await invoke('clear_translation_cache');
  const gpu=await recognize(),gpuT=await translated(gpu);assert(gpuT.results.every(r=>r.device==='cuda'));samples.push({combination:'OCR CPU + translation GPU',ocr:gpu.result.metrics,translation:gpuT});pass('Independent OCR CPU + translation CUDA');
  const runtimeEventsBefore=await page.evaluate(()=>window.__translationEvents.filter(e=>e.event==='cuda-runtime-progress').length);
  await updateT({device:'auto'});await invoke('load_translation_model',{id:modelId});
  assert.equal((await invoke('translation_status')).device,'cuda');await invoke('clear_translation_cache');
  const autoWorker=ownedWorkers();assert.equal(autoWorker.length,1);
  const crashing=await recognize();await until(async()=>(await invoke('translation_status')).status==='translating');
  process.kill(autoWorker[0].ProcessId);
  await until(async()=>(await invoke('translation_status')).status==='error');
  await invoke('translate_once',{frameId:crashing.result.frameId});
  const recovered=await until(async()=>(await invoke('translation_history')).find(e=>e.frameId===crashing.result.frameId&&e.results.every(r=>r.status==='complete'&&r.device==='cpu')));
  const recoveredStatus=await invoke('translation_status');assert.equal(recoveredStatus.device,'cpu');assert.equal(recoveredStatus.status,'ready');
  const cpuRecoveryPid=ownedWorkers()[0].ProcessId;
  await invoke('clear_translation_cache');const sticky=await recognize();assert((await translated(sticky)).results.every(r=>r.device==='cpu'));assert.equal(ownedWorkers()[0].ProcessId,cpuRecoveryPid);
  assert.equal(await page.evaluate(()=>window.__translationEvents.filter(e=>e.event==='cuda-runtime-progress').length),runtimeEventsBefore);
  samples.push({case:'Auto CUDA crash recovers to CPU',translation:recovered,status:recoveredStatus});pass('Real Auto CUDA worker crash recovers to CPU and stays there without runtime downloads');
  await updateT({device:'cuda'});await invoke('load_translation_model',{id:modelId});
  await invoke('unload_model');settings={...settings,device:'cuda'};await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});await invoke('clear_translation_cache');
  const both=await recognize(),bothT=await translated(both);assert.equal(both.result.device,'CUDA');assert(bothT.results.every(r=>r.device==='cuda'));samples.push({combination:'OCR GPU + translation GPU',ocr:both.result.metrics,translation:bothT});pass('OCR CUDA + translation CUDA coexist on actual GPU');
  await page.screenshot({path:path.join(out,'cuda-translation.png')});
  }
  await invoke('unload_translation_model');const installed=(await invoke('translation_models')).find(m=>m.id===modelId),file=path.join(data,'translation/models',installed.file);
  const fd=fs.openSync(file,'r+'),byte=Buffer.alloc(1);fs.readSync(fd,byte,0,1,65536);const original=byte[0];byte[0]^=1;fs.writeSync(fd,byte,0,1,65536);fs.closeSync(fd);
  try {await assert.rejects(invoke('load_translation_model',{id:modelId}),/SHA|校验/);}finally{const fd=fs.openSync(file,'r+');fs.writeSync(fd,Buffer.from([original]),0,1,65536);fs.closeSync(fd);}pass('Same-size model corruption rejected before loading');
  const profile={id:'fixture-game',name:'Fixture Game',sourceLanguage:'en',targetLanguage:'zh',style:'',background:'',glossary:[{source:'messenger',target:'信使',category:'character'}],version:1,windowProcess:window.processName};
  await updateT({device:'cpu',persistentCache:true,contextEnabled:true,profiles:[profile],profileId:profile.id});await invoke('load_translation_model',{id:modelId});
  await page.getByRole('button',{name:'翻译',exact:true}).click();await page.getByRole('dialog').waitFor();
  const sourceLanguage=page.getByRole('combobox',{name:/^源语言/});
  assert.equal(await sourceLanguage.inputValue(),'');
  await sourceLanguage.selectOption('auto');await page.getByRole('button',{name:'保存翻译设置',exact:true}).click();
  await until(async()=>(await invoke('translation_settings')).sourceLanguageOverride==='auto');
  assert.equal((await invoke('translation_settings')).profiles[0].version,1);
  await sourceLanguage.selectOption('');await page.getByRole('button',{name:'保存翻译设置',exact:true}).click();
  await until(async()=>(await invoke('translation_settings')).sourceLanguageOverride===null);
  assert.equal((await invoke('translation_settings')).profiles[0].version,1);
  await page.getByRole('button',{name:'关闭翻译设置'}).click();tsettings=await invoke('translation_settings');
  pass('UI manual language override and return to Profile defaults persist without version churn');
  const contextFrame=await recognize();await translated(contextFrame);await invoke('confirm_translation_context',{frameId:contextFrame.result.frameId});
  const withContextFrame=await recognize(),withContext=await translated(withContextFrame);
  assert(withContext.results.every(r=>!r.cacheHit&&r.contextVersion>0));samples.push({case:'explicit confirmed context',translation:withContext});
  const otherProfile={...profile,id:'other-fixture-game'};await updateT({profiles:[profile,otherProfile],profileId:otherProfile.id});
  await assert.rejects(invoke('confirm_translation_context',{frameId:contextFrame.result.frameId}),/其他游戏/);
  await updateT({profiles:[profile],profileId:profile.id});pass('Confirmed context is translated explicitly and cannot cross game profiles');
  const profilePath=path.join(out,'profiles.json');await invoke('export_translation_profiles',{path:profilePath});assert.deepEqual(await invoke('import_translation_profiles',{path:profilePath}),[profile]);
  await updateT({contextEnabled:false});const persistedFrame=await recognize();await translated(persistedFrame);pass('Profile JSON roundtrip, manual context confirmation and bounded cache opt-in');
  assert.deepEqual(failures,[]);await invoke('unload_model');await invoke('stop_capture');
  await close();assert.equal(ownedWorkers().length,0);pass('Application exit cleans up translation child');
  await launch();tsettings=await invoke('translation_settings');assert.equal(tsettings.profileId,profile.id);assert.equal(tsettings.persistentCache,true);assert.equal(tsettings.contextEnabled,false);
  settings={...await invoke('get_settings'),device:'cpu'};await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});await target();await invoke('load_translation_model',{id:modelId});
  const restoredFrame=await recognize(),restored=await translated(restoredFrame);assert(restored.results.every(r=>r.cacheHit));assert.equal((await invoke('translation_status')).calls,0);pass('Restart restores bounded cache without a neural translation call');
  await invoke('clear_translation_cache');assert(!fs.existsSync(path.join(data,'translation/cache.json')));await updateT({persistentCache:false});await close();
  fs.writeFileSync(path.resolve(process.env.GAMEOCR_TRANSLATION_DESKTOP_REPORT??'docs/test-results/translation-desktop.json'),JSON.stringify({executedAt:new Date().toISOString(),appSha256:createHash('sha256').update(fs.readFileSync(binary)).digest('hex'),downloadRequested:process.env.GAMEOCR_TEST_DOWNLOAD==='1',checks,samples,skipped,failures,out},null,2));
}catch(e){const status=await invoke?.('translation_status').catch(()=>null);fs.writeFileSync(path.join(out,'failure.json'),JSON.stringify({error:String(e),stack:e.stack,checks,samples,failures,status},null,2));throw e;}
finally{if(app?.exitCode===null)await close().catch(()=>{});await browser?.close().catch(()=>{});fixture?.kill();if(app?.exitCode===null)app.kill();fs.closeSync(log);}
