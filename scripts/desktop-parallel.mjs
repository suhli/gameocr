import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
const out=path.resolve('artifacts/validation/parallel-desktop');fs.mkdirSync(out,{recursive:true});
const dataDir=path.join(out,'data-'+Date.now());
const log=fs.openSync(path.join(out,'desktop.log'),'w');
const vite=process.env.GAMEOCR_APP?null:spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1420','--strictPort'],{windowsHide:true,stdio:['ignore',log,log]});
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn){for(let i=0;i<300;i++){try{const result=await fn();if(result)return result;}catch{}await delay(100);}throw Error('Condition timed out');}
if(vite)await until(async()=>(await fetch('http://127.0.0.1:1420')).ok);
const binary=path.resolve(process.env.GAMEOCR_APP??'target/debug/gameocr.exe');
const app=spawn(binary,[],{windowsHide:true,stdio:['ignore',log,log],env:{...process.env,GAMEOCR_DATA_DIR:dataDir,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9227'}});
let browser,fixture,invoke;const checks=[];
try{
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9227'));
  const page=await until(()=>browser.contexts()[0]?.pages().find(p=>/1420|tauri.localhost|tauri:\/\//.test(p.url())));
  await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await invoke('unload_model');let settings={...await invoke('get_settings'),modelDir:path.resolve('.cache/test-models'),device:'cpu',threads:4,changeDetection:false,ocrIntervalMs:100,captureFps:20,executionMode:'single',enableParallelInference:false,cpuWorkerCount:1};await invoke('update_settings',{settings});
  await page.reload();await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  await page.locator('select').filter({has:page.getByRole('option',{name:'PP-OCRv6 small · 已下载',exact:true})}).selectOption('ppocrv6-small');
  await page.getByRole('button',{name:'打开设置'}).click();
  assert.equal(await page.getByLabel('CPU Worker 数量').count(),0);
  await page.getByLabel('OCR 执行策略',{exact:true}).selectOption('manual');await page.getByLabel('CPU Worker 数量').fill('2');
  await page.getByRole('button',{name:'保存设置',exact:true}).click();
  await until(async()=>!(await page.locator('dialog').count()));
  settings=await invoke('get_settings');assert.equal(settings.enableParallelInference,true);assert.equal(settings.cpuWorkerCount,2);
  await page.reload();await page.getByRole('heading',{name:/窗口列表/}).waitFor();assert.equal((await invoke('get_settings')).cpuWorkerCount,2);checks.push('Parallel settings hidden by default, editable through UI and persisted across reload');
  await invoke('load_model',{id:'ppocrv6-small'});await assert.rejects(invoke('update_settings',{settings:{...settings,cpuWorkerCount:1}}));
  const control=path.join(out,'fixture-control.txt');fs.rmSync(control,{force:true});
  fixture=spawn(path.resolve('target/debug/capture-fixture.exe'),[control],{windowsHide:true,stdio:['ignore',log,log]});
  const target=await until(async()=>(await invoke('list_windows',{showAll:false})).find(w=>w.title==='GameOCR Capture Fixture'));
  await page.getByRole('button',{name:'刷新窗口'}).click();await page.getByPlaceholder(/搜索窗口/).fill(target.title);await page.locator('.window-item').filter({hasText:target.title}).click();await until(async()=>(await invoke('get_status')).selected?.id===target.id);
  fs.writeFileSync(control,'large');await delay(300);await invoke('start_capture');
  await until(async()=>{const p=await invoke('get_capture_preview');return p?.width>=1280;});
  await invoke('recognize_once');const snapshot=(await invoke('get_history'))[0];assert.equal(snapshot.result.metrics.execution.tileCount,4);assert.equal(snapshot.result.frameId,snapshot.preview.frameId);assert(snapshot.result.blocks.some(b=>b.text.includes('Health')));assert(snapshot.result.blocks.some(b=>b.text.includes('12345')));
  const exported=path.join(out,'parallel-wgc.json');await invoke('export_result',{frameId:snapshot.result.frameId,path:exported});assert.deepEqual(JSON.parse(fs.readFileSync(exported)),snapshot.result);
  const png=path.join(out,'parallel-frame.png');await invoke('save_screenshot',{path:png,recognized:true});const bytes=fs.readFileSync(png);assert.equal(bytes.readUInt32BE(16),snapshot.result.sourceWidth);assert.equal(bytes.readUInt32BE(20),snapshot.result.sourceHeight);
  await until(async()=>/CPU 2/.test(await page.locator('.execution-stats summary').innerText()));await page.locator('.execution-stats summary').click();await page.screenshot({path:path.join(out,'parallel-wgc.png')});checks.push('Large real WGC frame: four tiles, one merged history entry, coordinates and JSON export, live worker statistics');
  // Capture the invoke promise so these tests wait for native work to drain,
  // independently of the immediate UI idle/paused status.
  for(const action of ['pause_capture','stop_ocr','select_window','stop_capture']){
    const pending=invoke('recognize_once');await delay(30);await invoke(action,action==='select_window'?{id:target.id}:{});
    const before=await invoke('get_history');await pending;assert.deepEqual(await invoke('get_history'),before);
    if(action==='pause_capture')await invoke('resume_capture');
    if(action==='select_window'){await invoke('start_capture');await until(()=>invoke('get_capture_preview'));}
  }
  checks.push('Pause, stop and capture-generation invalidation discard in-flight batch without publishing late results');
  const children=()=>JSON.parse(execFileSync('powershell.exe',['-NoProfile','-Command',`ConvertTo-Json -Compress -InputObject @((Get-CimInstance Win32_Process -Filter "ParentProcessId=${app.pid} AND Name='onnx-worker.exe'").ProcessId | Where-Object { $_ })`],{windowsHide:true,encoding:'utf8'}).trim()||'[]');
  assert.equal(children().length,2);await invoke('unload_model');assert.equal(children().length,0);
  settings={...settings,executionMode:'single',enableParallelInference:false};await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-medium'});assert.equal(children().length,1);await invoke('unload_model');assert.equal(children().length,0);checks.push('Unload removes every old process; disabling parallel restores one Worker on model switch');
  await invoke('update_settings',{settings:{...settings,executionMode:'manual',enableParallelInference:true}});await invoke('load_model',{id:'ppocrv6-small'});const owned=children();assert.equal(owned.length,2);await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  const pendingExit=invoke('recognize_once').catch(()=>{});await delay(30);await invoke('plugin:window|close',{label:'main'}).catch(()=>{});await until(()=>app.exitCode!==null);await pendingExit;assert.equal(app.exitCode,0);invoke=null;
  const remaining=execFileSync('powershell.exe',['-NoProfile','-Command',`Get-Process -Id @(${owned.join(',')}) -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id; exit 0`],{windowsHide:true,encoding:'utf8'}).trim();assert.equal(remaining,'');assert.deepEqual(fs.readdirSync(path.join(dataDir,'sessions')),[]);checks.push('Normal application exit during parallel inference removes owned workers and session files');
  fs.writeFileSync(process.env.GAMEOCR_PARALLEL_DESKTOP_REPORT??'docs/test-results/desktop-parallel.json',JSON.stringify({executedAt:new Date().toISOString(),app:path.relative(process.cwd(),binary),appSha256:createHash('sha256').update(fs.readFileSync(binary)).digest('hex'),checks},null,2)+'\n');console.log('Passed '+checks.length+' real Tauri parallel checks.');
}finally{await invoke?.('stop_capture').catch(()=>{});await invoke?.('unload_model').catch(()=>{});await browser?.close();fixture?.kill();app.kill();vite?.kill();fs.closeSync(log);}
