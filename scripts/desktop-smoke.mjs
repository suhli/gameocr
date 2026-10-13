// Integration test against the actual Tauri WebView, IPC, Win32 capture, and model worker.
// No mocked model results or synthetic IPC responses.
import { chromium } from '@playwright/test';
import { spawn,execFileSync } from 'node:child_process';
import fs from 'node:fs/promises';
import path from 'node:path';
import assert from 'node:assert/strict';
const root=process.cwd();
const out=path.resolve(process.env.GAMEOCR_TEST_OUTPUT||path.join(root,'artifacts/validation/desktop'));
const data=path.join(out,'user-data');
const control=path.join(out,'fixture-control.txt');
await fs.mkdir(out,{recursive:true});
await fs.rm(control,{force:true});
const log=await fs.open(path.join(out,'native.log'),'w');
const fixture=spawn(path.join(root,'target/debug/capture-fixture.exe'),[control],{stdio:['ignore',log.fd,log.fd],windowsHide:false});
fixture.on('exit',(code,signal)=>console.log('FIXTURE EXIT',JSON.stringify({code,signal,at:new Date().toISOString()})));
const appEnv={...process.env,GAMEOCR_DATA_DIR:data,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9223'};
for(const key of Object.keys(appEnv))if(key.toLowerCase()==='path'||key.startsWith('CUDA_PATH'))delete appEnv[key];
appEnv.Path=`${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`;
const app=spawn(path.resolve(process.env.GAMEOCR_APP||path.join(root,'target/debug/gameocr.exe')),[],{env:appEnv,stdio:['ignore',log.fd,log.fd],windowsHide:true});
app.on('exit',(code,signal)=>console.log('APP EXIT',JSON.stringify({code,signal,at:new Date().toISOString()})));
const delay=ms=>new Promise(resolve=>setTimeout(resolve,ms));
let browser,page,invoke;
const checks=[];
function memorySample(){
  const script=`$ids=@(${app.pid}); $all=Get-CimInstance Win32_Process; for($i=0;$i -lt $ids.Count;$i++){ $parent=$ids[$i]; $all | Where-Object ParentProcessId -eq $parent | ForEach-Object { if($ids -notcontains $_.ProcessId){$ids += $_.ProcessId} } }; Get-Process -Id $ids -ErrorAction SilentlyContinue | Select-Object Id,ProcessName,WorkingSet64,PrivateMemorySize64 | ConvertTo-Json -Compress`;
  try{return JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',script],{encoding:'utf8',windowsHide:true,timeout:10000}));}catch{return null;}
}
const passed=(name,detail={})=>{checks.push({name,status:'passed',...detail});console.log(`PASS ${name}`,JSON.stringify(detail));};
async function until(fn,timeout=30000){const end=Date.now()+timeout;while(Date.now()<end){try{const v=await fn();if(v)return v;}catch{}await delay(250);}throw Error('Condition timed out');}
// Publish a complete command atomically: the fixture must never consume an
// empty file between writeFile's truncate and write operations.
async function controlFixture(action){await fs.writeFile(`${control}.tmp`,action);await fs.rename(`${control}.tmp`,control);await until(async()=>!(await fs.stat(control).catch(()=>null)));}
try{
  browser=await until(async()=>chromium.connectOverCDP('http://127.0.0.1:9223'));
  page=await until(async()=>browser.contexts().flatMap(c=>c.pages()).find(p=>/1420|tauri.localhost|tauri:\/\//.test(p.url())));
  const consoleErrors=[];page.on('pageerror',e=>consoleErrors.push(e.message));
  await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  invoke=(command,args={})=>page.evaluate(([command,args])=>window.__TAURI_INTERNALS__.invoke(command,args),[command,args]);
  await page.screenshot({path:path.join(out,'01-startup.png')});passed('Tauri startup + frontend mount');
  const settings=await invoke('get_settings');
  const modelDir=process.env.GAMEOCR_MODEL_DIR||path.join(data,'models');
  await invoke('update_settings',{settings:{...settings,modelDir,device:'auto',maxTokens:256,maxImageEdge:800,ocrIntervalMs:1500,changeDetection:true,historyLimit:30,theme:'dark'}});
  let imported=(await invoke('list_models')).find(m=>m.imported&&m.installed);
  if(!imported)imported=await invoke('import_model',{model:path.join(root,'models/LightOnOCR-3-0.8B-Q8_0.gguf'),mmproj:path.join(root,'models/LightOnOCR-3-0.8B-mmproj-f16.gguf')});
  passed('Import + SHA-256 verified model pair',{id:imported.id});
  await page.reload();await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  await invoke('load_model',{id:imported.id});const loaded=await invoke('get_model_status');assert.equal(loaded.status,'ready');passed('Model load',{device:loaded.device,loadMs:loaded.loadMs});
  await until(async()=>await page.getByRole('combobox',{name:'选择模型'}).inputValue()===imported.id);
  assert(await page.getByRole('combobox',{name:'选择模型'}).isDisabled());passed('Model selector matches resident model');
  if(process.env.GAMEOCR_EXPECT_DEVICE){assert.equal(loaded.device,process.env.GAMEOCR_EXPECT_DEVICE);assert.match(loaded.message,/CUDA/);passed('Automatic CUDA → CPU fallback',{message:loaded.message});}
  assert.equal(fixture.exitCode,null,'Test fixture exited before capture');
  const target=await until(async()=>(await invoke('list_windows',{showAll:false})).find(w=>w.title==='GameOCR Capture Fixture'));
  await page.getByRole('button',{name:'刷新窗口'}).click();await page.getByPlaceholder(/搜索窗口/).fill('GameOCR Capture Fixture');await page.locator('.window-item').filter({hasText:'GameOCR Capture Fixture'}).click();await until(async()=>(await invoke('get_status')).selected?.id===target.id);await invoke('start_capture');await delay(2000);console.log('CAPTURE STATUS',JSON.stringify((await invoke('get_status')).capture));
  const preview=await until(()=>invoke('get_capture_preview'));assert(preview.width>500&&preview.height>250);passed('WGC capture of Win32 window',{width:preview.width,height:preview.height});
  await invoke('recognize_once');const first=(await invoke('get_history'))[0];
  assert(first.result.blocks.some(b=>/Health/i.test(b.text)));assert(first.result.blocks.some(b=>b.bbox));
  assert.equal(first.preview.frameId,first.result.frameId);passed('WGC → model → Grounding → matching preview',{device:first.result.device,ms:first.result.metrics.totalMs,blocks:first.result.blocks.length});
  await page.screenshot({path:path.join(out,'02-grounding.png')});
  await page.getByRole('button',{name:'查看原始截图'}).click();const original=page.getByAltText('完整分辨率的原始截图');await original.waitFor();assert.equal(await original.evaluate(img=>img.naturalWidth),first.result.sourceWidth);await page.getByRole('button',{name:'关闭原始截图'}).click();passed('Original PNG viewer');
  const health=first.result.blocks.find(b=>/Health/i.test(b.text)&&b.bbox);
  await page.locator('.bbox').filter({has:page.locator('span')}).nth(first.result.blocks.filter(b=>b.bbox).findIndex(b=>b.id===health.id)).click();assert.match(await page.locator('.result-block.active').innerText(),/Health/);passed('Bounding-box / text selection');
  await page.getByRole('button',{name:'复制全部文字'}).click();
  const exportPath=path.join(out,'result.json');await invoke('export_result',{frameId:first.result.frameId,path:exportPath});
  assert.equal(JSON.parse(await fs.readFile(exportPath,'utf8')).frameId,first.result.frameId);passed('Export real OCR JSON');
  await invoke('save_screenshot',{path:path.join(out,'capture.png'),recognized:true});passed('Export full-resolution PNG');
  await invoke('start_ocr');await delay(4000);await invoke('stop_ocr');
  assert.equal((await invoke('get_history')).length,1);passed('Unchanged frame does not repeat inference');
  await invoke('pause_capture');assert.equal((await invoke('get_status')).capture,'paused');
  await invoke('recognize_once');const count=(await invoke('get_history')).length;await invoke('recognize_once');const rerun=await invoke('get_history');assert.equal(rerun.length,count);assert.equal(new Set(rerun.map(s=>s.result.frameId)).size,count);passed('Forced rerun of paused frame keeps unique export key');
  const rerunPath=path.join(out,'paused-rerun.json');await invoke('export_result',{frameId:rerun[0].result.frameId,path:rerunPath});assert.deepEqual(JSON.parse(await fs.readFile(rerunPath,'utf8')),rerun[0].result);passed('Paused rerun exports the exact latest result');
  await invoke('resume_capture');assert.equal((await invoke('get_status')).capture,'running');passed('Pause / resume');
  await controlFixture('resize');await until(async()=>{const p=await invoke('get_capture_preview');return p&&p.width!==preview.width;});passed('WGC resize');
  const work=invoke('recognize_once');await delay(100);await invoke('stop_capture');await work;
  assert.equal((await invoke('get_history')).length,0);passed('Stop during inference discards stale result');
  await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  await controlFixture('minimize');await until(async()=>(await invoke('get_status')).capture==='error');passed('Minimized target stops capture with real error');
  await controlFixture('restore');await delay(1000);await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  const soakSeconds=Number(process.env.GAMEOCR_SOAK_SECONDS||0);
  if(soakSeconds>0){
    const s=await invoke('get_settings');await invoke('update_settings',{settings:{...s,changeDetection:false,historyLimit:5}});await invoke('start_ocr');
    const start=Date.now();const samples=[];
    while(Date.now()-start<soakSeconds*1000){await delay(10000);const p=await invoke('get_performance');const history=await invoke('get_history');assert(history.length<=5);assert.notEqual((await invoke('get_model_status')).status,'error');samples.push({elapsedMs:Date.now()-start,...p,history:history.length,memory:samples.length%6===0?memorySample():null});await fs.writeFile(path.join(out,'soak-progress.json'),JSON.stringify(samples,null,2));console.log(`SOAK ${Math.round((Date.now()-start)/1000)}s: ${p.queue}, ${history.length} results, ${p.ocrPerMinute} OCR/min`);}
    await invoke('stop_ocr');passed('Continuous capture + OCR soak',{seconds:soakSeconds,samples:samples.length});
  }
  await controlFixture('close');await until(()=>fixture.exitCode!==null);assert.equal(fixture.exitCode,0);await until(async()=>(await invoke('get_status')).capture==='error');passed('Destroyed target stops capture');
  await invoke('unload_model');assert.equal((await invoke('get_model_status')).status,'unloaded');passed('Unload releases worker');
  await page.getByRole('button',{name:'切换主题'}).click();await page.screenshot({path:path.join(out,'03-light.png')});passed('Light theme');
  assert.deepEqual(consoleErrors,[]);passed('No JavaScript runtime errors');
  await invoke('plugin:window|close',{label:'main'}).catch(()=>{});await until(()=>app.exitCode!==null,10000);assert.equal(app.exitCode,0);passed('Graceful application exit');
  await fs.writeFile(path.join(out,'report.json'),JSON.stringify({checks},null,2));
}catch(error){checks.push({name:'failure',status:'failed',message:error.stack});await page?.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});await fs.writeFile(path.join(out,'report.json'),JSON.stringify({checks},null,2));throw error;}
finally{if(invoke)await Promise.race([invoke('unload_model').catch(()=>{}),delay(5000)]);await browser?.close();app.kill();fixture.kill();await log.close();}
