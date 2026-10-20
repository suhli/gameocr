import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';
import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {createServer} from 'node:net';
const port=await new Promise((resolve,reject)=>{const server=createServer();server.once('error',reject);server.listen(0,'127.0.0.1',()=>{const port=server.address().port;server.close(()=>resolve(port));});});
const out=path.resolve('artifacts/validation/parallel-desktop');fs.mkdirSync(out,{recursive:true});
const dataDir=path.join(out,'data-'+Date.now());
const log=fs.openSync(path.join(out,'desktop.log'),'w');
const vite=process.env.GAMEOCR_APP?null:spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1420','--strictPort'],{windowsHide:true,stdio:['ignore',log,log]});
const delay=ms=>new Promise(r=>setTimeout(r,ms));
async function until(fn){for(let i=0;i<300;i++){try{const result=await fn();if(result)return result;}catch{}await delay(100);}throw Error('Condition timed out');}
if(vite)await until(async()=>(await fetch('http://127.0.0.1:1420')).ok);
const binary=path.resolve(process.env.GAMEOCR_APP??'target/debug/gameocr.exe');
const app=spawn(binary,[],{windowsHide:true,stdio:['ignore',log,log],env:{...process.env,GAMEOCR_DATA_DIR:dataDir,WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port='+port}});
let browser,fixture,invoke,page,controlled=false;const checks=[];
try{
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:'+port));
  page=await until(()=>browser.contexts()[0]?.pages().find(p=>/1420|tauri.localhost|tauri:\/\//.test(p.url())));
  await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  assert.equal(path.resolve((await invoke('get_settings')).modelDir),path.join(dataDir,'models'),'Test must control its isolated application instance');
  controlled=true;
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
  if(process.env.GAMEOCR_TEST_ADAPTIVE==='1'){
    await page.getByRole('button',{name:'打开设置'}).click();await page.getByLabel('OCR 执行策略',{exact:true}).selectOption('adaptive');
    assert.equal(await page.getByLabel('启用在线学习',{exact:true}).isChecked(),true);assert.equal(await page.getByLabel('允许低频策略探索',{exact:true}).isChecked(),true);
    assert.equal(await page.getByLabel('最大 CPU Worker 数',{exact:true}).inputValue(),'2');assert.equal(await page.getByLabel('最大 GPU Worker 数',{exact:true}).inputValue(),'0');
    await page.getByLabel('允许低频策略探索',{exact:true}).uncheck();await page.getByRole('button',{name:'查看学习统计',exact:true}).click();await page.getByLabel('自适应学习统计').waitFor();
    const bounds=await page.locator('dialog').boundingBox(),viewport=await page.evaluate(()=>({width:innerWidth,height:innerHeight}));assert(Math.abs(bounds.x+bounds.width/2-viewport.width/2)<3);assert(Math.abs(bounds.y+bounds.height/2-viewport.height/2)<3);
    await page.screenshot({path:path.join(out,'adaptive-settings.png')});await page.getByRole('button',{name:'保存设置',exact:true}).click();await until(async()=>!(await page.locator('dialog').count()));
    settings=await invoke('get_settings');assert.equal(settings.executionMode,'adaptive');await invoke('clear_adaptive_profiles');await invoke('load_model',{id:'ppocrv6-small'});assert.equal(children().length,1);await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
    await invoke('recognize_once');const learned=(await invoke('get_history'))[0];assert.equal(learned.result.metrics.execution.adaptive.strategy,'cpu-single');assert(learned.result.metrics.execution.adaptive.analysisMs>0);assert((await invoke('get_adaptive_statistics')).classes>0);
    await until(async()=>/自适应优化/.test(await page.locator('.execution-stats summary').innerText()));await page.screenshot({path:path.join(out,'adaptive-wgc.png')});checks.push('Adaptive UI defaults and centered statistics modal, persisted selection, actual WGC OCR with measured feature/decision timings');
    const stats=await invoke('get_adaptive_statistics');const pending=invoke('recognize_once');await delay(25);await invoke('pause_capture');await pending;assert.deepEqual(await invoke('get_adaptive_statistics'),stats);await invoke('resume_capture');
    checks.push('Pause during adaptive OCR commits no learning observation');
    const clearing=invoke('recognize_once');await until(async()=>(await invoke('get_status')).ocr==='processing');await invoke('clear_adaptive_profiles');await clearing;assert.equal((await invoke('get_adaptive_statistics')).classes,0);checks.push('Clearing history during analysis/inference invalidates the in-flight learning event');
    await invoke('stop_capture');await invoke('unload_model');assert.equal(children().length,0);
  }
  if(process.env.GAMEOCR_TEST_EFFICIENCY==='1'){
    const originalOnnx=structuredClone(settings.onnx);
    await page.getByRole('button',{name:'打开设置'}).click();
    for(const label of ['保留高分辨率小字（重叠分块检测）','对疑似漏字区域补做高分辨率检测','增量识别：复用像素完全未变的区域与文字','按文字宽度分批，限制每批像素量','只识别指定区域']){
      assert.equal(await page.getByLabel(label,{exact:true}).isChecked(),false);
    }
    await page.getByLabel('保留高分辨率小字（重叠分块检测）',{exact:true}).check();
    assert(await page.getByLabel('对疑似漏字区域补做高分辨率检测',{exact:true}).isDisabled());
    await page.getByLabel('保留高分辨率小字（重叠分块检测）',{exact:true}).uncheck();
    await page.getByLabel('对疑似漏字区域补做高分辨率检测',{exact:true}).check();
    await page.getByLabel('每帧最多补检区域',{exact:true}).fill('8');
    await page.getByLabel('增量识别：复用像素完全未变的区域与文字',{exact:true}).check();
    await page.getByLabel('按文字宽度分批，限制每批像素量',{exact:true}).check();
    await page.getByLabel('每批像素预算（不含颜色通道）',{exact:true}).fill('196608');
    await page.getByLabel('只识别指定区域',{exact:true}).check();
    for(const [label,value] of [['左边比例','0'],['顶部比例','0'],['宽度比例','1'],['高度比例','1']]){
      await page.getByLabel(label,{exact:true}).fill(value);
    }
    await page.getByLabel('跳过没有显著变化的画面',{exact:true}).check();
    await page.getByLabel('变化检测阈值',{exact:true}).fill('1');
    const bounds=await page.locator('dialog').boundingBox(),viewport=await page.evaluate(()=>({width:innerWidth,height:innerHeight}));
    assert(Math.abs(bounds.x+bounds.width/2-viewport.width/2)<3);assert(Math.abs(bounds.y+bounds.height/2-viewport.height/2)<3);
    await page.screenshot({path:path.join(out,'efficiency-settings.png')});
    await page.getByRole('button',{name:'保存设置',exact:true}).click();await until(async()=>!(await page.locator('dialog').count()));
    settings=await invoke('get_settings');assert(settings.onnx.incremental&&settings.onnx.widthBuckets&&settings.onnx.selectiveDetection);assert.equal(settings.onnx.refinementRegions,8);assert.deepEqual(settings.onnx.detectionRegion,[0,0,1,1]);
    await page.reload();await page.getByRole('heading',{name:/窗口列表/}).waitFor();assert.deepEqual((await invoke('get_settings')).onnx,settings.onnx);
    checks.push('Efficiency settings default off, full tiling disables selective control, ROI/budget/cache values persist, dialog centered');
    await invoke('load_model',{id:'ppocrv6-small'});assert.equal(children().length,1);
    await assert.rejects(invoke('update_settings',{settings:{...settings,onnx:{...settings.onnx,incremental:false}}}));
    await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
    await invoke('recognize_once');const first=(await invoke('get_history'))[0];assert(first.result.blocks.some(b=>b.text.replace(/\s/g,'').includes('Health:100/100')));assert(first.result.metrics.detectionRegions<=9);
    await delay(200);await invoke('recognize_once');const reused=(await invoke('get_history'))[0];
    assert(reused.result.frameId>first.result.frameId);assert(reused.result.metrics.cache.reusedLines>0);assert.equal(reused.result.metrics.inferenceMs,0);
    assert.deepEqual(reused.result.blocks,first.result.blocks);
    checks.push('Real WGC optimized PPM path: one Worker, bounded refinement, new frame identity, identical repeated-frame output with inference skipped');
    await invoke('start_ocr');fs.writeFileSync(control,'number');
    const changed=await until(async()=>{const entry=(await invoke('get_history'))[0];return entry?.result.blocks.some(b=>b.text.replace(/\s/g,'').includes('Health:101/100'))&&entry;});
    assert(changed.result.frameId>reused.result.frameId);
    await invoke('stop_ocr');await invoke('stop_capture');await invoke('unload_model');assert.equal(children().length,0);
    checks.push('Automatic OCR sees a single-number update despite maximum thumbnail change threshold; no stale text and workers cleaned on unload');
    settings={...settings,onnx:originalOnnx,changeDetection:false};
  }
  await invoke('update_settings',{settings:{...settings,executionMode:'manual',enableParallelInference:true}});await invoke('load_model',{id:'ppocrv6-small'});const owned=children();assert.equal(owned.length,2);await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  const pendingExit=invoke('recognize_once').catch(()=>{});await delay(30);await invoke('plugin:window|close',{label:'main'}).catch(()=>{});await until(()=>app.exitCode!==null);await pendingExit;assert.equal(app.exitCode,0);invoke=null;
  const remaining=execFileSync('powershell.exe',['-NoProfile','-Command',`Get-Process -Id @(${owned.join(',')}) -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Id; exit 0`],{windowsHide:true,encoding:'utf8'}).trim();assert.equal(remaining,'');assert.deepEqual(fs.readdirSync(path.join(dataDir,'sessions')),[]);checks.push('Normal application exit during parallel inference removes owned workers and session files');
  fs.writeFileSync(process.env.GAMEOCR_PARALLEL_DESKTOP_REPORT??'docs/test-results/desktop-parallel.json',JSON.stringify({executedAt:new Date().toISOString(),app:path.relative(process.cwd(),binary),appSha256:createHash('sha256').update(fs.readFileSync(binary)).digest('hex'),checks},null,2)+'\n');console.log('Passed '+checks.length+' real Tauri parallel checks.');
}catch(error){await page?.screenshot({path:path.join(out,'failure.png')}).catch(()=>{});throw error;}
finally{if(controlled){await invoke?.('stop_capture').catch(()=>{});await invoke?.('unload_model').catch(()=>{});}await browser?.close();fixture?.kill();app.kill();vite?.kill();fs.closeSync(log);}
