import {chromium} from '@playwright/test';
import {spawn,execFileSync} from 'node:child_process';import {createHash} from 'node:crypto';import fs from 'node:fs';import path from 'node:path';import assert from 'node:assert/strict';
const root=process.cwd(),out=path.resolve(process.env.GAMEOCR_TEST_OUTPUT??'artifacts/validation/multi-runtime');fs.mkdirSync(out,{recursive:true});
const log=fs.openSync(path.join(out,'desktop.log'),'w');
const vite=process.env.GAMEOCR_APP?null:spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port','1420','--strictPort'],{windowsHide:true,stdio:['ignore',log,log]});
if(vite){
  let ready=false;
  for(let i=0;i<100;i++){try{if((await fetch('http://127.0.0.1:1420')).ok){ready=true;break;}}catch{}await new Promise(resolve=>setTimeout(resolve,200));}
  if(!ready){vite.kill();throw Error('Vite did not become ready');}
}
const appPath=path.resolve(process.env.GAMEOCR_APP??'target/debug/gameocr.exe');
const reportPath=path.resolve(process.env.GAMEOCR_TEST_REPORT??'docs/test-results/desktop-runtimes.json');
const hasCudaWorker=fs.existsSync(path.join(path.dirname(appPath),'workers/onnx/cuda/onnx-worker.exe'))||!process.env.GAMEOCR_APP;
const appEnv={...process.env,GAMEOCR_DATA_DIR:path.join(out,'data'),CUDA_VISIBLE_DEVICES:'-1',WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS:'--remote-debugging-port=9226'};
for(const key of Object.keys(appEnv))if(key.toLowerCase()==='path'||key.startsWith('CUDA_PATH')||key==='ORT_DYLIB_PATH')delete appEnv[key];
appEnv.Path=`${process.env.SystemRoot}\\System32;${process.env.SystemRoot}`;
const app=spawn(appPath,[],{windowsHide:true,stdio:['ignore',log,log],env:appEnv});
let fixture,invoke,importedId;
const delay=ms=>new Promise(r=>setTimeout(r,ms));let browser;
async function until(fn){for(let i=0;i<100;i++){try{const result=await fn();if(result)return result;}catch{}await delay(200);}throw Error('WebView initialization timed out');}
const checks=[];
try{
  browser=await until(()=>chromium.connectOverCDP('http://127.0.0.1:9226'));
  const page=await until(()=>browser.contexts()[0]?.pages().find(p=>/1420|tauri.localhost|tauri:\/\//.test(p.url())));
  await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  await page.waitForFunction(()=>!!window.__TAURI_INTERNALS__,{timeout:30000});
  invoke=(command,args={})=>page.evaluate(({command,args})=>window.__TAURI_INTERNALS__.invoke(command,args),{command,args});
  await invoke('unload_model');let settings=await invoke('get_settings');
  settings={...settings,modelDir:path.resolve('.cache/test-models'),device:'auto'};await invoke('update_settings',{settings});
  await page.reload();await page.getByRole('heading',{name:/窗口列表/}).waitFor();
  for(const variant of ['small','medium'])await until(async()=>{const label=await page.getByRole('option',{name:`PP-OCRv6 ${variant} · 已下载`,exact:true}).innerText();return !!label;});
  const models=await invoke('list_models');for(const id of ['ppocrv6-small','ppocrv6-medium'])assert(models.some(m=>m.id===id&&m.installed));checks.push('Multi-runtime catalog / verified files');
  await invoke('load_model',{id:'ppocrv6-small'});let status=await invoke('get_model_status');assert.equal(status.device,'CPU');assert.match(status.message,/回退 CPU/);checks.push(hasCudaWorker?'Actual CUDA initialization failure falls back to CPU':'Missing CUDA worker in CPU-only distribution falls back to CPU');
  await assert.rejects(invoke('update_settings',{settings:{...settings,onnx:{...settings.onnx,batchSize:2}}}));checks.push('Loaded-model parameter changes rejected');
  await invoke('load_model',{id:'ppocrv6-medium'});status=await invoke('get_model_status');assert.equal(status.id,'ppocrv6-medium');assert.equal(status.status,'ready');checks.push('Model switch disposes old worker');
  await page.getByRole('button',{name:'打开设置'}).click();await page.getByText('Detection Threshold',{exact:true}).waitFor();assert.equal(await page.getByText('上下文大小',{exact:true}).count(),0);await page.screenshot({path:path.join(out,'onnx-settings.png')});await page.getByRole('button',{name:'关闭设置'}).click();checks.push('ONNX capability-driven settings render');
  await invoke('unload_model');settings={...settings,device:'cuda'};await invoke('update_settings',{settings});await assert.rejects(invoke('load_model',{id:'ppocrv6-small'}));assert.equal((await invoke('get_model_status')).status,'error');checks.push('Explicit CUDA failure remains an error');
  await invoke('unload_model');settings={...settings,device:'cpu'};await invoke('update_settings',{settings});await invoke('load_model',{id:'ppocrv6-small'});assert.equal((await invoke('get_model_status')).status,'ready');await invoke('unload_model');checks.push('Recovery after worker initialization failure');
  const control=path.join(out,'fixture-control.txt');fs.rmSync(control,{force:true});
  fixture=spawn(path.resolve('target/debug/capture-fixture.exe'),[control],{windowsHide:true,stdio:['ignore',log,log]});
  const target=await until(async()=>(await invoke('list_windows',{showAll:false})).find(w=>w.title==='GameOCR Capture Fixture'));
  await page.getByRole('button',{name:'刷新窗口'}).click();await page.getByPlaceholder(/搜索窗口/).fill(target.title);
  await page.locator('.window-item').filter({hasText:target.title}).click();await until(async()=>(await invoke('get_status')).selected?.id===target.id);
  await invoke('start_capture');await until(()=>invoke('get_capture_preview'));
  for(const id of ['ppocrv6-small','ppocrv6-medium']){
    await invoke('load_model',{id});await invoke('recognize_once');const snapshot=(await invoke('get_history'))[0];
    assert.equal(snapshot.result.modelId,id);assert.equal(snapshot.result.runtime,'onnx');assert.equal(snapshot.preview.frameId,snapshot.result.frameId);
    assert(snapshot.result.blocks.some(b=>b.text.includes('Health')&&b.polygon.length===4&&b.confidence>0.5));
    const exported=path.join(out,`${id}-wgc.json`);await invoke('export_result',{frameId:snapshot.result.frameId,path:exported});assert.deepEqual(JSON.parse(fs.readFileSync(exported,'utf8')),snapshot.result);
    await until(async()=>(await page.locator('.result-footer').innerText()).includes(id==='ppocrv6-small'?'PP-OCRv6 small':'PP-OCRv6 medium'));
    if(id==='ppocrv6-medium'){
      const errorClose=page.getByRole('button',{name:'关闭错误'});if(await errorClose.count())await errorClose.click();
      await page.screenshot({path:path.join(out,'onnx-wgc.png')});
    }
    if(id==='ppocrv6-small'){
      // Kill only this test app's direct ONNX child, never an unrelated worker.
      assert(Number.isInteger(app.pid));
      const terminate=`$owned=@(Get-CimInstance Win32_Process -Filter "ParentProcessId=${app.pid} AND Name='onnx-worker.exe'"); if($owned.Count -ne 1){throw 'Expected one owned ONNX child'}; Stop-Process -Id $owned[0].ProcessId -Force`;
      execFileSync('powershell.exe',['-NoProfile','-Command',terminate],{windowsHide:true,stdio:'pipe'});
      await assert.rejects(invoke('recognize_once'));assert.equal((await invoke('get_model_status')).status,'error');assert.equal(app.exitCode,null);
      await invoke('load_model',{id});await invoke('recognize_once');assert.equal((await invoke('get_history'))[0].result.modelId,id);
      checks.push('Abrupt worker exit stays isolated; reload performs real OCR again');
    }
    await invoke('unload_model');
  }
  await invoke('stop_capture');fixture.kill();checks.push('Both ONNX models: real WGC frame, OCR, matching preview, model label and JSON export');
  const dir=path.resolve('.cache/test-models');const args={variant:'small',detector:path.join(dir,'ppocrv6-small-det.onnx'),recognizer:path.join(dir,'ppocrv6-small-rec.onnx'),dictionary:path.join(dir,'ppocrv6_dict.txt')};
  const imported=await invoke('import_onnx_model',args);importedId=imported.id;assert(imported.imported&&imported.installed);await invoke('load_model',{id:imported.id});await invoke('unload_model');
  const dict=imported.files.find(f=>f.role==='dictionary');const corrupt=fs.readFileSync(path.join(dir,dict.path));corrupt[20]^=1;fs.writeFileSync(path.join(dir,dict.path),corrupt);assert.equal((await invoke('list_models')).find(m=>m.id===imported.id).installed,false);
  await page.reload();await page.getByRole('button',{name:'打开模型管理',exact:true}).click();await page.getByRole('button',{name:`删除 ${imported.name}`,exact:true}).waitFor();await page.getByRole('button',{name:'关闭模型管理'}).click();checks.push('Corrupted imported model retains a visible delete action');
  await invoke('delete_model',{id:imported.id});importedId=null;assert(imported.files.every(f=>!fs.existsSync(path.join(dir,f.path))));checks.push('Real multi-file import, inference validation, same-size corruption and deletion');
  const before=fs.readdirSync(dir).length;await assert.rejects(invoke('import_onnx_model',{...args,recognizer:args.detector}));assert.equal(fs.readdirSync(dir).length,before);checks.push('Invalid ONNX graph rolls back every copied file');
  fs.writeFileSync(reportPath,JSON.stringify({executedAt:new Date().toISOString(),app:path.relative(root,appPath),appSha256:createHash('sha256').update(fs.readFileSync(appPath)).digest('hex'),checks},null,2)+'\n');console.log(`Passed ${checks.length} real Tauri runtime checks.`);
}finally{if(importedId&&invoke){await invoke('unload_model').catch(()=>{});await invoke('delete_model',{id:importedId}).catch(()=>{});}await browser?.close();fixture?.kill();app.kill();vite?.kill();fs.closeSync(log);}
